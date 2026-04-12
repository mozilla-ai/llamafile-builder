use crate::models::SystemInfo;

pub fn system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        memory_gb: detect_memory_gb(),
        has_nvidia_gpu: detect_nvidia_gpu(),
        has_amd_gpu: detect_amd_gpu(),
        has_metal: cfg!(target_os = "macos"),
    }
}

fn detect_memory_gb() -> f64 {
    #[cfg(target_os = "macos")]
    {
        // sysctl hw.memsize returns total memory in bytes
        let output = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output();
        if let Ok(output) = output {
            if let Ok(s) = String::from_utf8(output.stdout) {
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    return bytes as f64 / 1_073_741_824.0;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let rest = rest.trim();
                    if let Some(kb_str) = rest.strip_suffix("kB") {
                        if let Ok(kb) = kb_str.trim().parse::<u64>() {
                            return kb as f64 / 1_048_576.0;
                        }
                    }
                }
            }
        }
    }

    // Fallback
    0.0
}

fn detect_nvidia_gpu() -> bool {
    std::process::Command::new("nvidia-smi")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn detect_amd_gpu() -> bool {
    // Check for ROCm installation
    if std::path::Path::new("/opt/rocm").exists() {
        return true;
    }
    std::process::Command::new("rocminfo")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_info_returns_valid_data() {
        let info = system_info();
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert!(info.cpu_cores >= 1);
        // Memory detection may return 0.0 on unsupported platforms
        // but should not panic.
    }

    #[test]
    fn detect_memory_gb_returns_reasonable_value() {
        let gb = detect_memory_gb();
        // On CI or real machines, memory should be positive.
        // On unsupported platforms it returns 0.0.
        assert!(gb >= 0.0);
        if gb > 0.0 {
            // Sanity check: no machine has less than 0.5 GB or more than 16 TB
            assert!(gb > 0.5);
            assert!(gb < 16_384.0);
        }
    }
}
