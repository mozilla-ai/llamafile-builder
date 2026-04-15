use std::path::Path;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::config_store::{ConfigStore, SavedState};
use llamafile_core::Builder;

use crate::cli::ConfigCmd;
use crate::state_file;

pub fn run(
    cmd: &ConfigCmd,
    config: &BuilderConfig,
    store: &ConfigStore,
    config_dir: &Path,
    json: bool,
) -> Result<(), String> {
    // Recipe management commands (don't need a current recipe)
    match cmd {
        ConfigCmd::New => {
            let recipe = store.create().map_err(|e| format!("{e}"))?;
            state_file::save_current_recipe_id(config_dir, recipe.id)?;
            if json {
                println!("{}", serde_json::json!({"id": recipe.id.to_string()}));
            } else {
                println!("Created new recipe: {}", recipe.id);
            }
            return Ok(());
        }
        ConfigCmd::List => {
            let recipes = store.list().map_err(|e| format!("{e}"))?;
            let current_id = state_file::load_current_recipe_id(config_dir);
            if json {
                let items: Vec<serde_json::Value> = recipes.iter().map(|r| {
                    serde_json::json!({
                        "id": r.id.to_string(),
                        "name": r.name,
                        "model": r.state.model_id,
                        "updated_at": r.updated_at.to_rfc3339(),
                        "current": Some(r.id) == current_id,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else if recipes.is_empty() {
                println!("No saved recipes.");
            } else {
                for r in &recipes {
                    let marker = if Some(r.id) == current_id { "* " } else { "  " };
                    let name = r.name.as_deref().unwrap_or("(unnamed)");
                    let model = r.state.model_id.as_deref().unwrap_or("(no model)");
                    println!("{}{} — {} — {} — {}", marker, &r.id.to_string()[..8], name, model, r.updated_at.format("%Y-%m-%d %H:%M"));
                }
            }
            return Ok(());
        }
        ConfigCmd::Load { name_or_id } => {
            let recipe = store.find_by_name_or_id(name_or_id)
                .map_err(|e| format!("{e}"))?
                .ok_or_else(|| format!("Recipe not found: {name_or_id}"))?;
            state_file::save_current_recipe_id(config_dir, recipe.id)?;
            let name = recipe.name.as_deref().unwrap_or("(unnamed)");
            println!("Loaded recipe: {} ({})", name, &recipe.id.to_string()[..8]);
            return Ok(());
        }
        ConfigCmd::Delete { name_or_id } => {
            let recipe = store.find_by_name_or_id(name_or_id)
                .map_err(|e| format!("{e}"))?
                .ok_or_else(|| format!("Recipe not found: {name_or_id}"))?;
            let current_id = state_file::load_current_recipe_id(config_dir);
            store.delete(recipe.id).map_err(|e| format!("{e}"))?;
            if Some(recipe.id) == current_id {
                state_file::clear_current_recipe(config_dir);
            }
            println!("Deleted recipe: {}", &recipe.id.to_string()[..8]);
            return Ok(());
        }
        _ => {} // Fall through to mutations that need a current recipe
    }

    // Get or create the current working recipe
    let mut recipe = state_file::get_or_create_current_recipe(store, config_dir)
        .map_err(|e| format!("{e}"))?;

    match cmd {
        ConfigCmd::SetVersion { group, version } => {
            recipe.state.version_group = Some(group.clone());
            recipe.state.version = Some(version.clone());
            recipe.state.custom_runtime = None;
            tracing::info!("version set to {} / {}", group, version);
        }
        ConfigCmd::SetModel { id } => {
            if !config.models.iter().any(|m| m.id == *id) {
                return Err(format!("Model '{}' not found in registry", id));
            }
            recipe.state.model_id = Some(id.clone());
            recipe.state.custom_model_file = None;
            recipe.state.quantizations.clear();
            tracing::info!("model set to {}", id);
        }
        ConfigCmd::SetModelFile { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            recipe.state.custom_model_file = Some(path.to_string_lossy().to_string());
            recipe.state.model_id = None;
            recipe.state.quantizations.clear();
            tracing::info!("custom model file set to {}", path.display());
        }
        ConfigCmd::SetQuantization { role, name } => {
            recipe.state.quantizations.insert(role.clone(), name.clone());
            tracing::info!("quantization for '{}' set to '{}'", role, name);
        }
        ConfigCmd::SetOutput { path } => {
            recipe.state.output_path = Some(path.to_string_lossy().to_string());
            tracing::info!("output path set to {}", path.display());
        }
        ConfigCmd::Set { key, value } => {
            let parsed: serde_json::Value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            recipe.state.config_values.insert(key.clone(), parsed);
            tracing::info!("config '{}' set", key);
        }
        ConfigCmd::SetArgsFile { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            recipe.state.args_file = Some(path.to_string_lossy().to_string());
            tracing::info!("args file set to {}", path.display());
        }
        ConfigCmd::AddGpu { spec } => {
            if !recipe.state.gpu_libs.contains(spec) {
                recipe.state.gpu_libs.push(spec.clone());
            }
            tracing::info!("GPU library added: {}", spec);
        }
        ConfigCmd::RemoveGpu { spec } => {
            recipe.state.gpu_libs.retain(|s| s != spec);
            tracing::info!("GPU library removed: {}", spec);
        }
        ConfigCmd::SetRuntime { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            recipe.state.custom_runtime = Some(path.to_string_lossy().to_string());
            recipe.state.version_group = None;
            recipe.state.version = None;
            tracing::info!("custom runtime set to {}", path.display());
        }
        ConfigCmd::SetZipalign { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            recipe.state.custom_zipalign = Some(path.to_string_lossy().to_string());
            tracing::info!("custom zipalign set to {}", path.display());
        }
        ConfigCmd::Save { name } => {
            recipe.name = Some(name.clone());
            recipe.state.resolve_defaults(config);
            recipe.updated_at = chrono::Utc::now();
            store.save(&recipe).map_err(|e| format!("{e}"))?;
            println!("Saved recipe '{}' ({})", name, &recipe.id.to_string()[..8]);
            return Ok(());
        }
        ConfigCmd::Show => {
            show_recipe(&recipe, config, json)?;
            return Ok(());
        }
        ConfigCmd::ShowArgs => {
            let cache = Cache::new(crate::get_cache_dir());
            let builder = state_file::build_from_state(&recipe.state, config.clone(), cache)?;
            println!("{}", builder.resolved_args());
            return Ok(());
        }
        ConfigCmd::Reset => {
            recipe.state = SavedState::default();
            tracing::info!("recipe state reset");
        }
        // Already handled above
        ConfigCmd::New | ConfigCmd::List | ConfigCmd::Load { .. } | ConfigCmd::Delete { .. } => {
            unreachable!()
        }
    }

    // Auto-persist after every mutation
    recipe.updated_at = chrono::Utc::now();
    store.save(&recipe).map_err(|e| format!("{e}"))?;
    let id_short = &recipe.id.to_string()[..8];
    let name = recipe.name.as_deref().unwrap_or(id_short);
    eprintln!("[recipe: {}]", name);

    Ok(())
}

fn show_recipe(
    recipe: &llamafile_core::config_store::BuildRecipe,
    config: &BuilderConfig,
    json: bool,
) -> Result<(), String> {
    let saved = &recipe.state;

    if json {
        println!("{}", serde_json::to_string_pretty(&recipe).unwrap());
        return Ok(());
    }

    // Header
    let name = recipe.name.as_deref().unwrap_or("(unnamed)");
    println!("Recipe:        {} ({})", name, &recipe.id.to_string()[..8]);
    println!();

    let cache = Cache::new(crate::get_cache_dir());
    let builder = match state_file::build_from_state(saved, config.clone(), cache) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Warning: {e}");
            Builder::new(config.clone(), Cache::new(crate::get_cache_dir()))
        }
    };

    // Version
    if let Some(ref runtime) = saved.custom_runtime {
        println!("Version:       custom ({})", runtime);
    } else if let (Some(ref g), Some(ref v)) = (&saved.version_group, &saved.version) {
        println!("Version:       {} / {}", g, v);
    } else if let Some(ref g) = builder.state().selected_version_group {
        let v = builder.state().selected_release_version.as_deref().unwrap_or("?");
        println!("Version:       {} / {} (default)", g, v);
    } else {
        println!("Version:       (none available)");
    }

    // Model
    if let Some(ref path) = saved.custom_model_file {
        println!("Model:         custom ({})", path);
    } else {
        let model_id = saved.model_id.as_deref()
            .or(builder.state().selected_model_id.as_deref());
        let is_default = saved.model_id.is_none();
        if let Some(id) = model_id {
            if let Some(model) = config.models.iter().find(|m| m.id == id) {
                let quant_info: Vec<String> = model
                    .files
                    .iter()
                    .map(|f| {
                        let selected = saved
                            .quantizations
                            .get(&f.role)
                            .map(|n| n.as_str())
                            .or_else(|| f.quantizations.first().map(|q| q.name.as_str()))
                            .unwrap_or("?");
                        format!("{}:{}", f.role, selected)
                    })
                    .collect();
                let tag = if is_default { " (default)" } else { "" };
                println!("Model:         {} ({}){}", model.name, quant_info.join(", "), tag);
            } else {
                println!("Model:         {} (not found in registry)", id);
            }
        } else {
            println!("Model:         (none available)");
        }
    }

    // Output
    if let Some(ref path) = saved.output_path {
        println!("Output:        {}", path);
    } else if !builder.state().output_path.is_empty() {
        println!("Output:        {} (auto)", builder.state().output_path);
    }

    // GPU
    if saved.gpu_libs.is_empty() {
        println!("GPU libs:      (none)");
    } else {
        println!("GPU libs:      {}", saved.gpu_libs.join(", "));
    }

    // Args override
    if let Some(ref path) = saved.args_file {
        println!("Args file:     {} (overrides config below)", path);
    }

    // Config values
    let all_values = llamafile_core::args::default_config_values(&config.config_schema);
    println!();
    println!("Configuration (.args parameters):");
    let mut keys: Vec<_> = all_values.keys().collect();
    keys.sort();
    for key in keys {
        let default_val = &all_values[key];
        let is_overridden = saved.config_values.contains_key(key.as_str());
        let effective = saved.config_values.get(key.as_str()).unwrap_or(default_val);
        if is_overridden {
            println!("  {} = {} (was: {})", key, effective, default_val);
        } else {
            println!("  {} = {}", key, effective);
        }
    }

    // Custom binaries
    if let Some(ref path) = saved.custom_zipalign {
        println!("\nZipalign:      {}", path);
    }

    // Validation
    let msgs = builder.validate();
    let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
    let warnings: Vec<_> = msgs.iter().filter(|m| !m.is_error()).collect();
    if !errors.is_empty() || !warnings.is_empty() {
        println!();
    }
    for e in &errors {
        println!("ERROR: {:?}", e);
    }
    for w in &warnings {
        println!("WARNING: {:?}", w);
    }
    if errors.is_empty() {
        println!("\nReady to build.");
    }

    Ok(())
}
