use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::error::Error;

/// Find the zipalign binary. Checks (in order):
/// 1. Next to the running binary (for distributed builds)
/// 2. The system PATH
pub fn find_zipalign() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("zipalign");
            if adjacent.exists() {
                return adjacent;
            }
        }
    }

    // Fall back to PATH
    PathBuf::from("zipalign")
}

/// Run zipalign once with all files to bundle.
/// Args: zipalign -j0 <output_llamafile> <file1> <file2> ...
pub async fn zipalign_bundle(
    llamafile_path: &Path,
    files_to_add: &[&Path],
    custom_zipalign: Option<&Path>,
) -> Result<Vec<String>, Error> {
    let zipalign = match custom_zipalign {
        Some(p) => p.to_path_buf(),
        None => find_zipalign(),
    };
    let mut args = vec![
        "-j0".to_string(),
        llamafile_path.to_string_lossy().to_string(),
    ];
    for f in files_to_add {
        args.push(f.to_string_lossy().to_string());
    }

    let cmd_line = format!("$ {} {}", zipalign.display(), args.join(" "));
    tracing::info!("{cmd_line}");

    // Try direct exec first; if it fails with ENOEXEC (Exec format error),
    // retry via `sh`. This handles APE (Actually Portable Executable) binaries
    // on Linux systems without binfmt_misc support.
    let mut child = match Command::new(&zipalign)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.raw_os_error() == Some(8) => {
            // ENOEXEC -- retry via sh
            tracing::info!("Direct exec failed (ENOEXEC), retrying via sh");
            let mut sh_args = vec![zipalign.to_string_lossy().to_string()];
            sh_args.extend(args.iter().cloned());
            Command::new("sh")
                .args(&sh_args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        }
        Err(e) => return Err(e.into()),
    };

    let mut log_lines = Vec::new();
    log_lines.push(cmd_line);

    if let Some(stdout) = child.stdout.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!("zipalign: {line}");
            log_lines.push(line);
        }
    }

    if let Some(stderr) = child.stderr.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::warn!("zipalign: {line}");
            log_lines.push(line);
        }
    }

    let status = child.wait().await?;
    let code = status.code().unwrap_or(-1);
    log_lines.push(format!("zipalign exited with code {code}"));

    if !status.success() {
        let output = log_lines.join("\n");
        return Err(Error::Build(format!(
            "zipalign failed with exit code: {code}\n{output}"
        )));
    }

    Ok(log_lines)
}

/// Make a file executable (Unix: chmod +x, Windows: no-op).
pub async fn make_executable(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(path).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(path, perms).await?;
    }
    let _ = path;
    Ok(())
}
