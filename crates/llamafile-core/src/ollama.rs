use crate::models::OllamaModel;
use std::path::PathBuf;

// Default Ollama data directory.
// On macOS/Linux: ~/.ollama
// On Windows: %USERPROFILE%\.ollama (Ollama uses the same convention)
fn get_default_ollama_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ollama"))
}

fn get_ollama_models_dir(custom_dir: Option<&str>) -> Option<PathBuf> {
    match custom_dir {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir).join("models")),
        _ => get_default_ollama_dir().map(|d| d.join("models")),
    }
}

pub fn get_default_ollama_cache_dir() -> Option<String> {
    get_default_ollama_dir().map(|p| p.to_string_lossy().to_string())
}

pub fn check_ollama_available(custom_dir: Option<&str>) -> bool {
    let models_dir = match get_ollama_models_dir(custom_dir) {
        Some(dir) => dir,
        None => return false,
    };

    let manifests_dir = models_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library");

    if !manifests_dir.exists() {
        return false;
    }

    match std::fs::read_dir(&manifests_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Ok(tags) = std::fs::read_dir(entry.path()) {
                        if tags.into_iter().flatten().any(|t| t.path().is_file()) {
                            return true;
                        }
                    }
                }
            }
            false
        }
        Err(_) => false,
    }
}

#[derive(serde::Deserialize)]
struct OllamaManifestLayer {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
}

#[derive(serde::Deserialize)]
struct OllamaManifest {
    layers: Vec<OllamaManifestLayer>,
}

pub fn get_ollama_models(custom_dir: Option<&str>) -> Vec<OllamaModel> {
    let models_dir = match get_ollama_models_dir(custom_dir) {
        Some(dir) => dir,
        None => return vec![],
    };

    let manifests_dir = models_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library");

    let blobs_dir = models_dir.join("blobs");

    if !manifests_dir.exists() {
        return vec![];
    }

    let mut ollama_models = Vec::new();

    let model_dirs = match std::fs::read_dir(&manifests_dir) {
        Ok(dirs) => dirs,
        Err(_) => return vec![],
    };

    for model_entry in model_dirs.flatten() {
        let model_path = model_entry.path();
        if !model_path.is_dir() {
            continue;
        }

        let model_name = match model_path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let tag_files = match std::fs::read_dir(&model_path) {
            Ok(files) => files,
            Err(_) => continue,
        };

        for tag_entry in tag_files.flatten() {
            let tag_path = tag_entry.path();
            if !tag_path.is_file() {
                continue;
            }

            let tag = match tag_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            let manifest_content = match std::fs::read_to_string(&tag_path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            let manifest: OllamaManifest = match serde_json::from_str(&manifest_content) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let model_layer = manifest
                .layers
                .iter()
                .find(|l| l.media_type == "application/vnd.ollama.image.model");

            let model_layer = match model_layer {
                Some(layer) => layer,
                None => continue,
            };

            let hash = match model_layer.digest.strip_prefix("sha256:") {
                Some(h) => h,
                None => continue,
            };

            let blob_path = blobs_dir.join(format!("sha256-{}", hash));
            if !blob_path.exists() {
                continue;
            }

            let size_bytes = match std::fs::metadata(&blob_path) {
                Ok(meta) => meta.len(),
                Err(_) => continue,
            };

            ollama_models.push(OllamaModel {
                model_name: model_name.clone(),
                tag: tag.clone(),
                display_name: format!("{}:{}", model_name, tag),
                weights_path: blob_path.to_string_lossy().to_string(),
                size_bytes,
            });
        }
    }

    ollama_models.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    ollama_models
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_ollama_dir(
        base: &std::path::Path,
        entries: &[(&str, &str, &str)],
    ) {
        let blobs = base.join("models").join("blobs");
        fs::create_dir_all(&blobs).unwrap();

        for (model_name, tag, hash) in entries {
            let manifest_dir = base
                .join("models")
                .join("manifests")
                .join("registry.ollama.ai")
                .join("library")
                .join(model_name);
            fs::create_dir_all(&manifest_dir).unwrap();

            let manifest = serde_json::json!({
                "layers": [
                    {
                        "mediaType": "application/vnd.ollama.image.model",
                        "digest": format!("sha256:{hash}")
                    }
                ]
            });
            fs::write(
                manifest_dir.join(tag),
                serde_json::to_string(&manifest).unwrap(),
            )
            .unwrap();

            let blob_path = blobs.join(format!("sha256-{hash}"));
            fs::write(&blob_path, "fake model data").unwrap();
        }
    }

    #[test]
    fn check_available_returns_false_for_nonexistent_dir() {
        assert!(!check_ollama_available(Some("/tmp/no-such-ollama-dir-xyz")));
    }

    #[test]
    fn check_available_returns_false_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let manifests = tmp
            .path()
            .join("models")
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library");
        fs::create_dir_all(&manifests).unwrap();
        assert!(!check_ollama_available(Some(
            tmp.path().to_str().unwrap()
        )));
    }

    #[test]
    fn check_available_returns_true_with_a_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        make_ollama_dir(tmp.path(), &[("llama3", "latest", "abcdef")]);
        assert!(check_ollama_available(Some(tmp.path().to_str().unwrap())));
    }

    #[test]
    fn get_models_returns_parsed_entries() {
        let tmp = tempfile::tempdir().unwrap();
        make_ollama_dir(
            tmp.path(),
            &[
                ("llama3", "latest", "aaa111"),
                ("llama3", "8b", "bbb222"),
                ("phi3", "mini", "ccc333"),
            ],
        );
        let models = get_ollama_models(Some(tmp.path().to_str().unwrap()));
        assert_eq!(models.len(), 3);

        let names: Vec<_> = models.iter().map(|m| m.display_name.as_str()).collect();
        assert_eq!(names, vec!["llama3:8b", "llama3:latest", "phi3:mini"]);

        for m in &models {
            assert!(m.weights_path.contains("sha256-"));
            assert!(m.size_bytes > 0);
        }
    }

    #[test]
    fn get_models_skips_entries_with_missing_blobs() {
        let tmp = tempfile::tempdir().unwrap();
        make_ollama_dir(tmp.path(), &[("good", "latest", "aaa")]);

        let manifest_dir = tmp
            .path()
            .join("models")
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join("bad");
        fs::create_dir_all(&manifest_dir).unwrap();
        let manifest = serde_json::json!({
            "layers": [{
                "mediaType": "application/vnd.ollama.image.model",
                "digest": "sha256:nonexistent"
            }]
        });
        fs::write(
            manifest_dir.join("latest"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let models = get_ollama_models(Some(tmp.path().to_str().unwrap()));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name, "good:latest");
    }

    #[test]
    fn get_models_returns_empty_for_nonexistent_dir() {
        let models = get_ollama_models(Some("/tmp/no-such-ollama-dir-xyz"));
        assert!(models.is_empty());
    }
}
