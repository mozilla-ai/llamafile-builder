pub mod agent;
pub mod args;
pub mod builder;
pub mod cache;
pub mod config_manager;
pub mod download;
pub mod error;
pub mod file_ops;
pub mod lmstudio;
pub mod models;
pub mod ollama;
pub mod state;
pub mod system_info;
pub mod validation;
pub mod zipalign;

pub use builder::Builder;
pub use config_manager::{BuilderConfig, ConfigManager, UpdateMode, UpdateStatus};
pub use error::{Error, Result};
