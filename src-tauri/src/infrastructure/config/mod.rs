//! 应用配置（`app_config_dir`）。

pub mod azure;

pub use azure::{load_azure_auth_config, AzureAuthConfig};
