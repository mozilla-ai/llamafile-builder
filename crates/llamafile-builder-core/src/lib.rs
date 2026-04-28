pub mod agent;
pub mod args;
pub mod builder;
pub mod cache;
pub mod config_manager;
pub mod config_store;
pub mod disk_space;
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

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

// Primary API
pub use builder::{Builder, BuildPhase};
pub use cache::{Cache, CacheStatus};
pub use config_manager::{BuilderConfig, ConfigManager, UpdateMode, UpdateStatus};
pub use config_store::{BuildRecipe, ConfigStore, SavedState};
pub use disk_space::{DiskRequirement, DiskSpaceError, DiskSpaceReport, Purpose as DiskPurpose};
pub use error::{DownloadError, Error, Result};
pub use validation::ValidationMessage;

// Component model
pub use models::{
    Component, ComponentRole, ComponentSource, GpuAccel, Os,
};

// Data types used by consumers
pub use models::{
    AgentConfig, AgentTool, ConfigSchemaItem, GpuLibrary, GpuLibs,
    GpuLibsByPlatform, LMStudioFile, LMStudioModel, LlamafileRelease, LlamafileVersionGroup,
    Model, ModelFile, ModelFileQuantization, OllamaModel, SystemInfo,
};

// Query functions
pub use models::{parse_gpu_spec, search_models};
pub use system_info::system_info;
