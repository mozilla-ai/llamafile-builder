use llamafile_builder_core::config_manager::BuilderConfig;
use llamafile_builder_core::models::{format_size, search_models};

pub fn list(config: &BuilderConfig, search: Option<&str>, tag: Option<&str>, json: bool) {
    let models = search_models(&config.models, search, tag);

    if json {
        println!("{}", serde_json::to_string_pretty(&models).unwrap());
        return;
    }

    if models.is_empty() {
        println!("No models found.");
        return;
    }

    for model in &models {
        let total_size: u64 = model
            .files
            .iter()
            .flat_map(|f| f.quantizations.first())
            .map(|q| q.size_bytes)
            .sum();
        let quant_name = model
            .files
            .iter()
            .find(|f| f.role == "weights")
            .and_then(|f| f.quantizations.first())
            .map(|q| q.name.as_str())
            .unwrap_or("?");

        println!(
            "  {:<40} {:>8}  {}",
            model.id,
            format_size(total_size),
            quant_name,
        );
    }
    println!("\n{} model(s) found.", models.len());
}

pub fn show(config: &BuilderConfig, id: &str, json: bool) {
    let model = config.models.iter().find(|m| m.id == id);
    match model {
        Some(model) => {
            if json {
                println!("{}", serde_json::to_string_pretty(model).unwrap());
                return;
            }
            println!("{}", model.name);
            println!("  Organization: {}", model.organization);
            println!("  Parameters:   {}", model.parameters);
            println!("  Context:      {} tokens", model.context_length);
            println!("  License:      {}", model.license);
            println!("  Description:  {}", model.description);
            if model.source == "hf" {
                println!("  HuggingFace:  https://huggingface.co/{}", model.id);
            }
            if !model.tags.is_empty() {
                println!("  Tags:         {}", model.tags.join(", "));
            }
            println!();
            for file in &model.files {
                let label = file.label.as_deref().unwrap_or(&file.role);
                println!("  {} ({}):", label, file.role);
                for quant in &file.quantizations {
                    println!(
                        "    {} - {}",
                        quant.name,
                        format_size(quant.size_bytes)
                    );
                }
            }
        }
        None => {
            eprintln!("Model not found: {}", id);
            std::process::exit(1);
        }
    }
}

