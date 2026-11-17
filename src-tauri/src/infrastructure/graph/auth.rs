//! Graph / OAuth：授权码 + PKCE；机密客户端带 secret，公开客户端走 SPA 流。

use super::loopback::{bind_redirect_listener, wait_for_authorization_code};
use super::models::RemoteAccount;
use super::token_store::{self, TokenCache};
use crate::domain::value_objects::EmailAddress;
use crate::infrastructure::config::AzureAuthConfig;
use crate::infrastructure::error::InfraError;
use base64::Engine;
use graph_rs_sdk::GraphClient;
use graph_rs_sdk::identity::{
    AuthorizationCodeCredential, AuthorizationCodeSpaCredential, ConfidentialClientApplication,
    GenPkce, ProofKeyCodeExchange, PublicClientApplication, TokenCredentialExecutor,
};
use serde::Deserialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

const DEFAULT_SCOPES: &[&str] = &[
    "openid",
    "profile",
    "offline_access",
    "User.Read",
    "Mail.ReadWrite",
    "Mail.Send",
    "Contacts.Read",
];

#[derive(Debug, Deserialize)]
struct GraphMe {
    id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
}

#[derive(Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    client_info: Option<String>,
}

/// 打开系统浏览器完成登录，返回远端账户资料与尚未持久化的 token 缓存。
pub async fn sign_in(config: &AzureAuthConfig) -> Result<(RemoteAccount, TokenCache), InfraError> {
    let client_uuid = parse_client_id(&config.client_id)?;
    let scopes: Vec<String> = DEFAULT_SCOPES.iter().map(|s| (*s).to_owned()).collect();
    let redirect_uri = Url::parse(&config.redirect_uri).map_err(|_| InfraError::Auth {
        message: format!("redirect_uri 无效: {}", config.redirect_uri),
    })?;

    let pkce = ProofKeyCodeExchange::oneshot().map_err(|e| InfraError::Auth {
        message: format!("生成 PKCE 失败: {e}"),
    })?;

    let listener = bind_redirect_listener(&redirect_uri).await?;

    let auth_url = if config.is_public_client() {
        let mut url_builder =
            AuthorizationCodeSpaCredential::authorization_url_builder(client_uuid);
        url_builder
            .with_tenant(&config.tenant)
            .with_redirect_uri(redirect_uri.clone())
            .with_scope(scopes.clone())
            .with_pkce(&pkce);
        url_builder.url()
    } else {
        let mut url_builder = AuthorizationCodeCredential::authorization_url_builder(client_uuid);
        url_builder
            .with_tenant(&config.tenant)
            .with_redirect_uri(redirect_uri.clone())
            .with_scope(scopes.clone())
            .with_pkce(&pkce);
        url_builder.url()
    }
    .map_err(|e| InfraError::Auth {
        message: format!("构造授权 URL 失败: {e}"),
    })?;

    webbrowser::open(auth_url.as_str()).map_err(|e| InfraError::Auth {
        message: format!("无法打开系统浏览器: {e}"),
    })?;

    let authorization_code = wait_for_authorization_code(listener).await?;
    let token_response =
        exchange_authorization_code(config, &authorization_code, &redirect_uri, &pkce, &scopes)
            .await?;

    let now_ms = now_unix_ms();
    let home_account_id = home_account_id_from_client_info(token_response.client_info.as_deref());

    let scopes_vec = token_response
        .scope
        .as_deref()
        .map(|s| {
            s.split_whitespace()
                .map(|part| part.to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| scopes.clone());

    let cache = TokenCache {
        access_token: token_response.access_token.clone(),
        refresh_token: token_response.refresh_token,
        expires_at_ms: now_ms + token_response.expires_in.saturating_mul(1000),
        scopes: scopes_vec,
        home_account_id,
        client_id: config.client_id.clone(),
        tenant: config.tenant.clone(),
    };

    let remote = fetch_remote_account(&cache.access_token).await?;
    Ok((remote, cache))
}

pub async fn save_token_cache(
    credentials_dir: &Path,
    credential_ref: &crate::domain::value_objects::CredentialRef,
    cache: &TokenCache,
) -> Result<(), InfraError> {
    token_store::save(credentials_dir, credential_ref.as_str(), cache)
}

/// 读取本地凭证；必要时用 refresh_token 刷新并回写。
pub async fn get_access_token(
    credentials_dir: &Path,
    config: &AzureAuthConfig,
    auth: &crate::domain::entities::GraphAuthConfig,
) -> Result<String, InfraError> {
    let credential_ref = auth.credential_ref.as_str();
    let Some(mut cache) = token_store::load(credentials_dir, credential_ref)? else {
        return Err(InfraError::Auth {
            message: "账户未登录或凭证已清除，请重新登录".into(),
        });
    };

    let now_ms = now_unix_ms();
    if token_store::is_access_token_fresh(&cache, now_ms) {
        return Ok(cache.access_token);
    }

    // 与配置切换/孤儿回收互斥，并重新读取以免重复刷新或复活已删除的引用。
    let _lock = crate::infrastructure::mail::credentials::lock(credentials_dir)?;
    cache =
        token_store::load(credentials_dir, credential_ref)?.ok_or_else(|| InfraError::Auth {
            message: "凭据已变更，请重新登录".into(),
        })?;
    if token_store::is_access_token_fresh(&cache, now_unix_ms()) {
        return Ok(cache.access_token);
    }
    let Some(refresh_token) = cache.refresh_token.clone() else {
        return Err(InfraError::Auth {
            message: "访问令牌已过期且无刷新令牌，请重新登录".into(),
        });
    };

    let refreshed = refresh_access_token(config, &refresh_token, &cache.scopes).await?;
    cache.access_token = refreshed.access_token.clone();
    if refreshed.refresh_token.is_some() {
        cache.refresh_token = refreshed.refresh_token;
    }
    cache.expires_at_ms = now_ms + refreshed.expires_in.saturating_mul(1000);
    if let Some(scope) = refreshed.scope.as_deref() {
        let parts: Vec<String> = scope.split_whitespace().map(|s| s.to_owned()).collect();
        if !parts.is_empty() {
            cache.scopes = parts;
        }
    }
    token_store::save(credentials_dir, credential_ref, &cache)?;
    Ok(cache.access_token)
}

async fn exchange_authorization_code(
    config: &AzureAuthConfig,
    authorization_code: &str,
    redirect_uri: &Url,
    pkce: &ProofKeyCodeExchange,
    scopes: &[String],
) -> Result<TokenEndpointResponse, InfraError> {
    if config.is_public_client() {
        let mut pub_client = PublicClientApplication::builder(&config.client_id);
        pub_client.with_tenant(&config.tenant);
        pub_client.with_scope(scopes.to_vec());
        let mut auth = pub_client.with_auth_code(authorization_code);
        auth.with_redirect_uri(redirect_uri.clone())
            .with_pkce(pkce)
            .with_scope(scopes.to_vec());
        let mut app = auth.build();
        return execute_token_request(&mut app).await;
    }

    let client_id = parse_client_id(&config.client_id)?;
    let secret = config
        .client_secret
        .as_deref()
        .ok_or_else(|| InfraError::Auth {
            message: "机密客户端缺少 client_secret".into(),
        })?;
    let mut conf = ConfidentialClientApplication::builder(client_id);
    conf.with_tenant(&config.tenant);
    conf.with_scope(scopes.to_vec());
    let mut auth = conf.with_auth_code(authorization_code);
    auth.with_client_secret(secret)
        .with_redirect_uri(redirect_uri.clone())
        .with_pkce(pkce)
        .with_scope(scopes.to_vec());
    let mut app = auth.build();
    execute_token_request(&mut app).await
}

async fn refresh_access_token(
    config: &AzureAuthConfig,
    refresh_token: &str,
    scopes: &[String],
) -> Result<TokenEndpointResponse, InfraError> {
    if config.is_public_client() {
        let mut pub_client = PublicClientApplication::builder(&config.client_id);
        pub_client.with_tenant(&config.tenant);
        pub_client.with_scope(scopes.to_vec());
        let mut auth = pub_client.with_auth_code(".");
        auth.with_refresh_token(refresh_token)
            .with_scope(scopes.to_vec());
        let mut app = auth.build();
        return execute_token_request(&mut app).await;
    }

    let client_id = parse_client_id(&config.client_id)?;
    let secret = config
        .client_secret
        .as_deref()
        .ok_or_else(|| InfraError::Auth {
            message: "机密客户端缺少 client_secret".into(),
        })?;
    let mut conf = ConfidentialClientApplication::builder(client_id);
    conf.with_tenant(&config.tenant);
    conf.with_scope(scopes.to_vec());
    let mut auth = conf.with_auth_code(".");
    auth.with_client_secret(secret)
        .with_refresh_token(refresh_token)
        .with_scope(scopes.to_vec());
    let mut app = auth.build();
    execute_token_request(&mut app).await
}

async fn execute_token_request(
    app: &mut (impl TokenCredentialExecutor + Send),
) -> Result<TokenEndpointResponse, InfraError> {
    let response = app.execute_async().await.map_err(|e| InfraError::Auth {
        message: format!("向微软换取令牌失败: {e}"),
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|e| InfraError::Network {
        message: format!("读取令牌响应失败: {e}"),
    })?;
    if !status.is_success() {
        return Err(InfraError::Auth {
            message: format!("微软令牌端点返回错误 ({status})"),
        });
    }
    serde_json::from_str(&body).map_err(|_| InfraError::Auth {
        message: "无法解析微软令牌响应".into(),
    })
}

async fn fetch_remote_account(access_token: &str) -> Result<RemoteAccount, InfraError> {
    let client = GraphClient::new(access_token);
    let response = client
        .me()
        .get_user()
        .send()
        .await
        .map_err(|e| InfraError::Network {
            message: format!("拉取用户资料失败: {e}"),
        })?;
    if !response.status().is_success() {
        return Err(InfraError::Auth {
            message: format!("拉取用户资料失败 ({})", response.status()),
        });
    }
    let me: GraphMe = response.json().await.map_err(|_| InfraError::Remote {
        message: "无法解析用户资料".into(),
        retryable: true,
    })?;

    let email_raw = me
        .mail
        .filter(|s| !s.trim().is_empty())
        .or(me.user_principal_name)
        .ok_or_else(|| InfraError::Auth {
            message: "用户资料缺少邮箱".into(),
        })?;
    let email = EmailAddress::parse(&email_raw).ok_or_else(|| InfraError::Auth {
        message: "用户邮箱无效".into(),
    })?;
    let display_name = me
        .display_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| email.as_str().to_owned());
    let oid = me.id.unwrap_or_default();

    Ok(RemoteAccount {
        account_id: oid,
        email,
        display_name,
        aliases: vec![],
    })
}

fn parse_client_id(client_id: &str) -> Result<Uuid, InfraError> {
    Uuid::parse_str(client_id.trim()).map_err(|_| InfraError::Auth {
        message: "azure.json 中的 client_id 不是有效 UUID".into(),
    })
}

fn home_account_id_from_client_info(client_info: Option<&str>) -> Option<String> {
    let raw = client_info?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| {
            let padded = match raw.len() % 4 {
                2 => format!("{raw}=="),
                3 => format!("{raw}="),
                _ => raw.to_owned(),
            };
            base64::engine::general_purpose::URL_SAFE.decode(padded.as_bytes())
        })
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let uid = value.get("uid")?.as_str()?;
    let utid = value.get("utid")?.as_str()?;
    Some(format!("{uid}.{utid}"))
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
