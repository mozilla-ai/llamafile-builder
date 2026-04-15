pub mod agent;
pub mod args;
pub mod builder;
pub mod cache;
pub mod config_manager;
pub mod config_store;
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

// Primary API
pub use builder::Builder;
pub use cache::{Cache, CacheStatus};
pub use config_manager::{BuilderConfig, ConfigManager, UpdateMode, UpdateStatus};
pub use config_store::{BuildRecipe, ConfigStore, SavedState};
pub use error::{DownloadError, Error, Result};
pub use validation::ValidationMessage;

// Component model
pub use models::{
    Component, ComponentRole, ComponentSource, GpuAccel, Os,
};

// Data types used by consumers
pub use models::{
    AgentConfig, AgentTool, ComputeConfig, ComputeMode, ConfigSchemaItem, GpuLibrary, GpuLibs,
    GpuLibsByPlatform, LMStudioFile, LMStudioModel, LlamafileRelease, LlamafileVersionGroup,
    Model, ModelFile, ModelFileQuantization, OllamaModel, SystemInfo,
};

// Query functions
pub use models::search_models;
pub use system_info::system_info;
