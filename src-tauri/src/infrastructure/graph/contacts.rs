//! Microsoft Graph 联系人 Delta。

use crate::application::dto::remote_models::{ContactDeltaPage, RemoteContact};
use crate::application::error::PortError;
use crate::domain::value_objects::EmailAddress;
use graph_rs_sdk::{GraphClient, ODataQuery};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ODataList {
    value: Vec<Value>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

fn map_http_err(context: &str, err: impl std::fmt::Display) -> PortError {
    PortError::Network {
        message: format!("{context}: {err}"),
    }
}

async fn ensure_success(
    response: reqwest::Response,
    context: &str,
) -> Result<reqwest::Response, PortError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retryable = status.as_u16() == 429 || status.is_server_error();
    let body = response.text().await.unwrap_or_default();
    Err(PortError::Remote {
        message: format!("{context} 失败 ({status}): {body}"),
        retryable,
    })
}

/// 联系人 Delta 一页；`delta_or_next_link` 为绝对 URL 时直接续页。
pub async fn delta_contacts(
    access_token: &str,
    delta_or_next_link: Option<&str>,
) -> Result<ContactDeltaPage, PortError> {
    let response = if let Some(link) = delta_or_next_link.filter(|s| !s.is_empty()) {
        get_absolute(access_token, link).await?
    } else {
        let response = GraphClient::new(access_token)
            .me()
            .contacts()
            .delta()
            .select(&["id", "displayName", "emailAddresses"])
            .send()
            .await
            .map_err(|e| map_http_err("Graph contacts delta", e))?;
        ensure_success(response, "Graph contacts delta").await?
    };

    let page: ODataList = response.json().await.map_err(|_| PortError::Remote {
        message: "无法解析 Graph 联系人 Delta".into(),
        retryable: true,
    })?;

    let mut upserts = Vec::new();
    let mut removed_graph_ids = Vec::new();
    for item in page.value {
        if item.get("@removed").is_some() {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                removed_graph_ids.push(id.to_string());
            }
            continue;
        }
        if let Some(remote) = map_remote_contact(&item) {
            upserts.push(remote);
        }
    }

    Ok(ContactDeltaPage {
        upserts,
        removed_graph_ids,
        delta_link: page.delta_link,
        next_link: page.next_link,
    })
}

fn map_remote_contact(value: &Value) -> Option<RemoteContact> {
    let graph_id = value.get("id")?.as_str()?.to_string();
    let display_name = value
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let address = value
        .get("emailAddresses")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|e| e.get("address"))
        .and_then(|v| v.as_str())
        .and_then(EmailAddress::parse)?;
    Some(RemoteContact {
        graph_id,
        display_name: if display_name.is_empty() {
            address.as_str().to_string()
        } else {
            display_name
        },
        normalized_address: address,
    })
}

async fn get_absolute(access_token: &str, link: &str) -> Result<reqwest::Response, PortError> {
    super::http::get_absolute(access_token, link, "Graph contacts absolute GET").await
}
