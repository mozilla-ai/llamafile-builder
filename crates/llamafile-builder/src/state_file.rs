use std::path::Path;

// Re-export from core — single source of truth.
pub use llamafile_builder_core::config_store::{build_from_state, SavedState};
use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore};

/// Path to the file that tracks which recipe is "current" for the CLI.
pub fn current_recipe_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("current-recipe")
}

/// Load the current recipe ID from the pointer file.
pub fn load_current_recipe_id(config_dir: &Path) -> Option<uuid::Uuid> {
    let path = current_recipe_path(config_dir);
    let content = std::fs::read_to_string(path).ok()?;
    uuid::Uuid::parse_str(content.trim()).ok()
}

/// Save the current recipe ID to the pointer file.
pub fn save_current_recipe_id(config_dir: &Path, id: uuid::Uuid) -> Result<(), String> {
    let path = current_recipe_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::write(&path, id.to_string())
        .map_err(|e| format!("Failed to write current recipe pointer: {e}"))?;
    Ok(())
}

/// Clear the current recipe pointer.
pub fn clear_current_recipe(config_dir: &Path) {
    let _ = std::fs::remove_file(current_recipe_path(config_dir));
}

/// Get or create the current working recipe.
/// If a current recipe ID exists and is valid, returns it.
/// Otherwise creates a new one and sets it as current.
pub fn get_or_create_current_recipe(
    store: &ConfigStore,
    config_dir: &Path,
) -> Result<BuildRecipe, String> {
    if let Some(id) = load_current_recipe_id(config_dir) {
        if let Ok(recipe) = store.get(id) {
            return Ok(recipe);
        }
    }
    // Create new
    let recipe = store.create().map_err(|e| format!("{e}"))?;
    save_current_recipe_id(config_dir, recipe.id)?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    // build_from_state itself is now exercised by core's
    // config_store::tests (one canonical source of coverage). The CLI
    // tests here cover the recipe-pointer file management that is
    // CLI-only.

    #[test]
    fn current_recipe_pointer_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let id = uuid::Uuid::new_v4();
        save_current_recipe_id(tmp.path(), id).unwrap();
        assert_eq!(load_current_recipe_id(tmp.path()), Some(id));

        clear_current_recipe(tmp.path());
        assert_eq!(load_current_recipe_id(tmp.path()), None);
    }
}
