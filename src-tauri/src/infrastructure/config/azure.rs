//! Azure / Entra 客户端配置：`{app_config_dir}/azure.json`
//!
//! `client_id`、`redirect_uri` 必填；`client_secret` 可选（公开客户端 / PKCE 可省略）。

use crate::infrastructure::error::InfraError;
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::Path;
use url::Url;

/// 登录用 Azure 客户端配置（授权码 + PKCE；机密客户端可带 secret）
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct AzureAuthConfig {
    pub client_id: String,
    /// 机密客户端密钥；公开客户端（如复用 Mailspring Client ID）可为空
    pub client_secret: Option<String>,
    /// 与 Entra 应用注册完全一致的回跳 URI，例如 `http://localhost:12141/desktop`
    pub redirect_uri: String,
    #[serde(default = "default_tenant")]
    pub tenant: String,
}

impl AzureAuthConfig {
    pub fn is_public_client(&self) -> bool {
        self.client_secret
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    }
}

impl fmt::Debug for AzureAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureAuthConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "***"),
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("tenant", &self.tenant)
            .finish()
    }
}

fn default_tenant() -> String {
    "common".into()
}

#[derive(Debug, Deserialize)]
struct AzureAuthConfigFile {
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    tenant: Option<String>,
}

/// 从 `azure.json` 加载鉴权字段。
pub fn load_azure_auth_config(config_dir: &Path) -> Result<AzureAuthConfig, InfraError> {
    let path = config_dir.join("azure.json");
    if !path.exists() {
        return Err(InfraError::Auth {
            message: format!(
                "未找到 Azure 配置：请在 {} 写入 client_id、redirect_uri（公开客户端可省略 client_secret）",
                path.display()
            ),
        });
    }

    let raw = fs::read_to_string(&path).map_err(|e| InfraError::Storage {
        message: format!("读取 azure.json 失败: {e}"),
    })?;
    let file_cfg: AzureAuthConfigFile =
        serde_json::from_str(&raw).map_err(|e| InfraError::Storage {
            message: format!("解析 azure.json 失败: {e}"),
        })?;

    let client_id = require_field(file_cfg.client_id, "client_id", &path)?;
    let redirect_uri = require_field(file_cfg.redirect_uri, "redirect_uri", &path)?;
    validate_redirect_uri(&redirect_uri)?;

    let client_secret = file_cfg
        .client_secret
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let tenant = file_cfg
        .tenant
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_tenant);

    Ok(AzureAuthConfig {
        client_id,
        client_secret,
        redirect_uri,
        tenant,
    })
}

fn require_field(
    value: Option<String>,
    name: &str,
    path: &Path,
) -> Result<String, InfraError> {
    value
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| InfraError::Auth {
            message: format!("未配置 Azure {name}：请在 {} 中填写", path.display()),
        })
}

fn validate_redirect_uri(raw: &str) -> Result<(), InfraError> {
    let url = Url::parse(raw).map_err(|_| InfraError::Auth {
        message: format!("redirect_uri 不是合法 URL: {raw}"),
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(InfraError::Auth {
            message: "redirect_uri 仅支持 http/https".into(),
        });
    }
    let host = url.host_str().unwrap_or("");
    if host != "localhost" && host != "127.0.0.1" {
        return Err(InfraError::Auth {
            message: "桌面登录 redirect_uri 主机须为 localhost 或 127.0.0.1".into(),
        });
    }
    if url.port().is_none() {
        return Err(InfraError::Auth {
            message: "redirect_uri 必须显式包含端口，例如 http://localhost:12141/desktop".into(),
        });
    }
    Ok(())
}
