use sha2::{Digest, Sha256};
use std::path::Path;

pub async fn file_exists(path: &str) -> bool {
    Path::new(path).exists()
}

pub async fn file_exists_with_sha256(path: &str, expected_sha256: &str) -> bool {
    match calculate_sha256(path).await {
        Ok(actual) => actual == expected_sha256,
        Err(_) => false,
    }
}

pub async fn delete_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.exists() {
        tokio::fs::remove_file(p)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn copy_file(src: &str, dest: &str) -> Result<(), String> {
    let dest_path = Path::new(dest);
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    tokio::fs::copy(src, dest)
        .await
        .map_err(|e| format!("Failed to copy file: {}", e))?;
    Ok(())
}

pub async fn write_temp_file(content: &str, filename: &str) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("llamafile-builder");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;
    let file_path = temp_dir.join(filename);
    tokio::fs::write(&file_path, content)
        .await
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    Ok(file_path.to_string_lossy().to_string())
}

pub async fn make_executable(path: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = Path::new(path);
        let metadata = tokio::fs::metadata(p)
            .await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(p, perms)
            .await
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }
    let _ = path;
    Ok(())
}

pub async fn calculate_sha256(path: &str) -> Result<String, String> {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024]; // 256KB buffer
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|e| format!("SHA256 task failed: {e}"))?
}
