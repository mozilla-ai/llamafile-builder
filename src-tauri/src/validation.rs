use crate::models::{ConfigSchemaItem, LlamafileVersionGroup, Model};

/// Validates all embedded JSON configuration files at startup.
/// Panics with a clear error message if any file is malformed.
pub fn validate_config_files() {
    validate_config_schema();
    validate_models();
    validate_versions();
}

fn validate_config_schema() {
    let content = include_str!("../config.json");
    match serde_json::from_str::<Vec<ConfigSchemaItem>>(content) {
        Ok(_) => {}
        Err(e) => {
            panic!(
                "\n\n\
                ==========================================\n\
                ERROR: config.json is invalid\n\
                ==========================================\n\
                {}\n\
                \n\
                Location: line {}, column {}\n\
                ==========================================\n\n",
                e,
                e.line(),
                e.column()
            );
        }
    }
}

fn validate_models() {
    let content = include_str!("../models.json");
    match serde_json::from_str::<Vec<Model>>(content) {
        Ok(_) => {}
        Err(e) => {
            panic!(
                "\n\n\
                ==========================================\n\
                ERROR: models.json is invalid\n\
                ==========================================\n\
                {}\n\
                \n\
                Location: line {}, column {}\n\
                ==========================================\n\n",
                e,
                e.line(),
                e.column()
            );
        }
    }
}

fn validate_versions() {
    let content = include_str!("../versions.json");
    match serde_json::from_str::<Vec<LlamafileVersionGroup>>(content) {
        Ok(_) => {}
        Err(e) => {
            panic!(
                "\n\n\
                ==========================================\n\
                ERROR: versions.json is invalid\n\
                ==========================================\n\
                {}\n\
                \n\
                Location: line {}, column {}\n\
                ==========================================\n\n",
                e,
                e.line(),
                e.column()
            );
        }
    }
}
