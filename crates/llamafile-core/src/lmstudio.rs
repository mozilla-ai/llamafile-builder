use crate::models::{LMStudioFile, LMStudioModel};
use std::path::PathBuf;

// Default LM Studio data directory.
// On macOS/Linux: ~/.cache/lm-studio
// On Windows: %USERPROFILE%\.cache\lm-studio
fn get_default_lmstudio_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".cache").join("lm-studio"))
}

fn get_lmstudio_models_dir(custom_dir: Option<&str>) -> Option<PathBuf> {
    match custom_dir {
        Some(dir) if !dir.is_empty() => {
            Some(PathBuf::from(dir).join("models").join("lmstudio-community"))
        }
        _ => get_default_lmstudio_dir().map(|d| d.join("models").join("lmstudio-community")),
    }
}

pub fn check_lmstudio_available(custom_dir: Option<&str>) -> bool {
    let models_dir = match get_lmstudio_models_dir(custom_dir) {
        Some(dir) => dir,
        None => return false,
    };

    if !models_dir.exists() {
        return false;
    }

    match std::fs::read_dir(&models_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Ok(files) = std::fs::read_dir(entry.path()) {
                        for file in files.flatten() {
                            let path = file.path();
                            if path.is_file() {
                                if let Some(ext) = path.extension() {
                                    if ext.to_string_lossy().to_lowercase() == "gguf" {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            false
        }
        Err(_) => false,
    }
}

fn infer_gguf_role(filename: &str) -> String {
    let lower = filename.to_lowercase();
    if lower.contains("mmproj") || lower.contains("clip") || lower.contains("vision") {
        "mmproj".to_string()
    } else {
        "weights".to_string()
    }
}

pub fn get_lmstudio_models(custom_dir: Option<&str>) -> Vec<LMStudioModel> {
    let models_dir = match get_lmstudio_models_dir(custom_dir) {
        Some(dir) => dir,
        None => return vec![],
    };

    if !models_dir.exists() {
        return vec![];
    }

    let mut lmstudio_models = Vec::new();

    let model_dirs = match std::fs::read_dir(&models_dir) {
        Ok(dirs) => dirs,
        Err(_) => return vec![],
    };

    for model_entry in model_dirs.flatten() {
        let model_path = model_entry.path();
        if !model_path.is_dir() {
            continue;
        }

        let dir_name = match model_path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let mut files = Vec::new();
        let mut total_size: u64 = 0;

        let file_entries = match std::fs::read_dir(&model_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for file_entry in file_entries.flatten() {
            let file_path = file_entry.path();
            if !file_path.is_file() {
                continue;
            }

            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext.to_lowercase() != "gguf" {
                continue;
            }

            let filename = match file_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            let size_bytes = match std::fs::metadata(&file_path) {
                Ok(meta) => meta.len(),
                Err(_) => continue,
            };

            let role = infer_gguf_role(&filename);
            total_size += size_bytes;

            files.push(LMStudioFile {
                filename,
                path: file_path.to_string_lossy().to_string(),
                size_bytes,
                role,
            });
        }

        if files.is_empty() {
            continue;
        }

        files.sort_by(|a, b| {
            if a.role == "weights" && b.role != "weights" {
                std::cmp::Ordering::Less
            } else if a.role != "weights" && b.role == "weights" {
                std::cmp::Ordering::Greater
            } else {
                a.filename.cmp(&b.filename)
            }
        });

        lmstudio_models.push(LMStudioModel {
            dir_name: dir_name.clone(),
            display_name: dir_name,
            dir_path: model_path.to_string_lossy().to_string(),
            files,
            total_size_bytes: total_size,
        });
    }

    lmstudio_models.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    lmstudio_models
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_lmstudio_dir(base: &std::path::Path, models: &[(&str, &[&str])]) {
        let community = base.join("models").join("lmstudio-community");
        for (dir_name, files) in models {
            let model_dir = community.join(dir_name);
            fs::create_dir_all(&model_dir).unwrap();
            for filename in *files {
                fs::write(model_dir.join(filename), "fake gguf data").unwrap();
            }
        }
    }

    #[test]
    fn check_available_returns_false_for_nonexistent_dir() {
        assert!(!check_lmstudio_available(Some("/tmp/no-lmstudio-xyz")));
    }

    #[test]
    fn check_available_returns_false_for_empty_models_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let community = tmp.path().join("models").join("lmstudio-community");
        fs::create_dir_all(&community).unwrap();
        assert!(!check_lmstudio_available(Some(
            tmp.path().to_str().unwrap()
        )));
    }

    #[test]
    fn check_available_returns_true_with_gguf_files() {
        let tmp = tempfile::tempdir().unwrap();
        make_lmstudio_dir(tmp.path(), &[("MyModel", &["model-Q4.gguf"])]);
        assert!(check_lmstudio_available(Some(
            tmp.path().to_str().unwrap()
        )));
    }

    #[test]
    fn get_models_returns_parsed_entries_with_roles() {
        let tmp = tempfile::tempdir().unwrap();
        make_lmstudio_dir(
            tmp.path(),
            &[("Llava-7B", &["model-Q4.gguf", "mmproj-BF16.gguf"])],
        );

        let models = get_lmstudio_models(Some(tmp.path().to_str().unwrap()));
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.dir_name, "Llava-7B");
        assert_eq!(m.files.len(), 2);

        assert_eq!(m.files[0].role, "weights");
        assert_eq!(m.files[0].filename, "model-Q4.gguf");
        assert_eq!(m.files[1].role, "mmproj");
        assert_eq!(m.files[1].filename, "mmproj-BF16.gguf");
    }

    #[test]
    fn get_models_ignores_non_gguf_files() {
        let tmp = tempfile::tempdir().unwrap();
        make_lmstudio_dir(tmp.path(), &[("M1", &["weights.gguf"])]);
        let m1_dir = tmp
            .path()
            .join("models")
            .join("lmstudio-community")
            .join("M1");
        fs::write(m1_dir.join("config.json"), "{}").unwrap();

        let models = get_lmstudio_models(Some(tmp.path().to_str().unwrap()));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].files.len(), 1);
    }

    #[test]
    fn get_models_skips_empty_model_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        make_lmstudio_dir(tmp.path(), &[("WithFiles", &["a.gguf"])]);
        let empty_dir = tmp
            .path()
            .join("models")
            .join("lmstudio-community")
            .join("Empty");
        fs::create_dir_all(&empty_dir).unwrap();

        let models = get_lmstudio_models(Some(tmp.path().to_str().unwrap()));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].dir_name, "WithFiles");
    }

    #[test]
    fn infer_gguf_role_detects_mmproj_keywords() {
        assert_eq!(infer_gguf_role("model-mmproj-BF16.gguf"), "mmproj");
        assert_eq!(infer_gguf_role("clip-vit-large.gguf"), "mmproj");
        assert_eq!(infer_gguf_role("vision-encoder.gguf"), "mmproj");
        assert_eq!(infer_gguf_role("llama3-Q4_K_M.gguf"), "weights");
    }

    #[test]
    fn get_models_returns_empty_for_nonexistent_dir() {
        let models = get_lmstudio_models(Some("/tmp/no-lmstudio-xyz"));
        assert!(models.is_empty());
    }
}
