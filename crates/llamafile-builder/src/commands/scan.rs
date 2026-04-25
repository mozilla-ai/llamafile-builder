use llamafile_builder_core::models::format_size;

pub fn ollama_list(custom_dir: Option<&str>, json: bool) {
    let available = llamafile_builder_core::ollama::check_ollama_available(custom_dir);
    if !available {
        if json {
            println!("[]");
        } else {
            let dir = custom_dir.unwrap_or("~/.ollama");
            println!("No Ollama models found at {}", dir);
        }
        return;
    }

    let models = llamafile_builder_core::ollama::get_ollama_models(custom_dir);
    if json {
        println!("{}", serde_json::to_string_pretty(&models).unwrap());
        return;
    }

    if models.is_empty() {
        println!("No Ollama models found.");
        return;
    }

    for model in &models {
        println!(
            "  {:<30} {}",
            model.display_name,
            format_size(model.size_bytes)
        );
    }
    println!("\n{} model(s) found.", models.len());
}

pub fn lmstudio_list(custom_dir: Option<&str>, json: bool) {
    let available = llamafile_builder_core::lmstudio::check_lmstudio_available(custom_dir);
    if !available {
        if json {
            println!("[]");
        } else {
            let dir = custom_dir.unwrap_or("~/.cache/lm-studio");
            println!("No LM Studio models found at {}", dir);
        }
        return;
    }

    let models = llamafile_builder_core::lmstudio::get_lmstudio_models(custom_dir);
    if json {
        println!("{}", serde_json::to_string_pretty(&models).unwrap());
        return;
    }

    if models.is_empty() {
        println!("No LM Studio models found.");
        return;
    }

    for model in &models {
        println!(
            "  {:<30} {} ({} file{})",
            model.display_name,
            format_size(model.total_size_bytes),
            model.files.len(),
            if model.files.len() == 1 { "" } else { "s" }
        );
    }
    println!("\n{} model(s) found.", models.len());
}
