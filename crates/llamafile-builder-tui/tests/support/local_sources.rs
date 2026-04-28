//! Scaffold fake Ollama / LM Studio install roots in a tempdir so the TUI's
//! Model Select Ollama and LMStudio tabs have reproducible content in tests.
//!
//! The on-disk layouts mirror what the core crate's scanners look for; the
//! shapes themselves are exercised by `crates/llamafile-builder-core/src/{ollama,lmstudio}.rs`
//! tests and are not re-asserted here.

use std::fs;
use std::path::Path;

use serde_json::json;

/// Create a minimal Ollama models tree under `base` containing one
/// manifest per `(model_name, tag)` entry, each pointing at a fake blob
/// in `models/blobs`. Matches `get_ollama_models`'s expected layout.
pub fn make_ollama_dir(base: &Path, entries: &[(&str, &str, &str)]) {
    let blobs = base.join("models").join("blobs");
    fs::create_dir_all(&blobs).expect("create blobs dir");

    for (model_name, tag, hash) in entries {
        let manifest_dir = base
            .join("models")
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join(model_name);
        fs::create_dir_all(&manifest_dir).expect("create manifest dir");

        let manifest = json!({
            "layers": [
                {
                    "mediaType": "application/vnd.ollama.image.model",
                    "digest": format!("sha256:{hash}"),
                }
            ]
        });
        fs::write(
            manifest_dir.join(tag),
            serde_json::to_string(&manifest).expect("serialise manifest"),
        )
        .expect("write manifest");

        let blob_path = blobs.join(format!("sha256-{hash}"));
        fs::write(&blob_path, b"fake model data").expect("write blob");
    }
}

/// Create a minimal LM Studio model directory under `base`. The scanner
/// expects `<base>/models/lmstudio-community/<dir_name>/*.gguf`; this
/// helper produces exactly that, with one fake-content GGUF per filename.
pub fn make_lmstudio_dir(base: &Path, dir_name: &str, gguf_filenames: &[&str]) {
    let model_dir = base
        .join("models")
        .join("lmstudio-community")
        .join(dir_name);
    fs::create_dir_all(&model_dir).expect("create model dir");
    for &fname in gguf_filenames {
        fs::write(model_dir.join(fname), b"fake gguf data").expect("write gguf");
    }
}
