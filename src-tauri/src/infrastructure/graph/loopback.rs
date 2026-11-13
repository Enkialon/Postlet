//! 本机 OAuth 回跳监听（按配置中的 redirect_uri 绑定）

use crate::application::error::PortError;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use url::Url;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// 按配置的 `redirect_uri` 绑定本机监听器（主机 + 端口必须一致）。
pub async fn bind_redirect_listener(redirect_uri: &Url) -> Result<TcpListener, PortError> {
    let host = redirect_uri.host_str().unwrap_or("127.0.0.1");
    let bind_host = if host == "localhost" {
        "127.0.0.1"
    } else {
        host
    };
    let port = redirect_uri.port().ok_or_else(|| PortError::Auth {
        message: "redirect_uri 缺少显式端口".into(),
    })?;
    let addr = SocketAddr::new(
        bind_host.parse().map_err(|_| PortError::Auth {
            message: format!("无法解析回跳主机: {bind_host}"),
        })?,
        port,
    );
    TcpListener::bind(addr).await.map_err(|e| PortError::Network {
        message: format!("无法绑定回跳地址 {addr}: {e}"),
    })
}

/// 等待一次 HTTP GET 回跳，解析 `code` 或 `error`
pub async fn wait_for_authorization_code(listener: TcpListener) -> Result<String, PortError> {
    let accept = timeout(LOGIN_TIMEOUT, listener.accept())
        .await
        .map_err(|_| PortError::Auth {
            message: "登录超时：未在时限内完成浏览器授权".into(),
        })?
        .map_err(|e| PortError::Network {
            message: format!("等待浏览器回跳失败: {e}"),
        })?;

    let (mut socket, _) = accept;
    let mut buf = vec![0u8; 8192];
    let n = timeout(Duration::from_secs(30), socket.read(&mut buf))
        .await
        .map_err(|_| PortError::Network {
            message: "读取回跳请求超时".into(),
        })?
        .map_err(|e| PortError::Network {
            message: format!("读取回跳请求失败: {e}"),
        })?;

    let request = String::from_utf8_lossy(&buf[..n]);
    let path_and_query = request
        .lines()
        .next()
        .and_then(|line| {
            let mut parts = line.split_whitespace();
            let _method = parts.next()?;
            parts.next()
        })
        .unwrap_or("/");

    let full = format!("http://localhost{path_and_query}");
    let parsed = Url::parse(&full).map_err(|_| PortError::Auth {
        message: "无法解析授权回跳 URL".into(),
    })?;

    let error = parsed
        .query_pairs()
        .find(|(k, _)| k == "error")
        .map(|(_, v)| v.into_owned());
    if let Some(err) = error {
        let desc = parsed
            .query_pairs()
            .find(|(k, _)| k == "error_description")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let _ = write_html_response(
            &mut socket,
            400,
            "<html><body><h1>登录失败</h1><p>可以关闭此窗口返回 Postlet。</p></body></html>",
        )
        .await;
        return Err(PortError::Auth {
            message: if desc.is_empty() {
                format!("微软登录失败: {err}")
            } else {
                format!("微软登录失败: {err}（{desc}）")
            },
        });
    }

    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned());

    let Some(code) = code.filter(|c| !c.is_empty()) else {
        let _ = write_html_response(
            &mut socket,
            400,
            "<html><body><h1>登录失败</h1><p>回跳缺少授权码。</p></body></html>",
        )
        .await;
        return Err(PortError::Auth {
            message: "授权回跳缺少 code".into(),
        });
    };

    let _ = write_html_response(
        &mut socket,
        200,
        "<html><body><h1>登录成功</h1><p>可以关闭此窗口返回 Postlet。</p></body></html>",
    )
    .await;

    Ok(code)
}

async fn write_html_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> Result<(), PortError> {
    let reason = match status {
        200 => "OK",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket
        .write_all(response.as_bytes())
        .await
        .map_err(|e| PortError::Network {
            message: format!("写回跳响应失败: {e}"),
        })?;
    let _ = socket.shutdown().await;
    Ok(())
}
