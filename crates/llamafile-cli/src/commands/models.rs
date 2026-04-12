use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::models::{format_size, Model};

pub fn list(config: &BuilderConfig, search: Option<&str>, tag: Option<&str>, json: bool) {
    let models = filter_models(&config.models, search, tag);

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
            if let Some(url) = &model.huggingface_url {
                println!("  HuggingFace:  {}", url);
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

fn filter_models<'a>(models: &'a [Model], search: Option<&str>, tag: Option<&str>) -> Vec<&'a Model> {
    models
        .iter()
        .filter(|m| {
            if let Some(q) = search {
                let q = q.to_lowercase();
                let name_match = m.name.to_lowercase().contains(&q)
                    || m.organization.to_lowercase().contains(&q)
                    || m.id.to_lowercase().contains(&q);
                if !name_match {
                    return false;
                }
            }
            if let Some(t) = tag {
                if !m.tags.iter().any(|mt| mt.eq_ignore_ascii_case(t)) {
                    return false;
                }
            }
            true
        })
        .collect()
}
