use std::collections::HashMap;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[derive(Debug, Deserialize)]
struct CreateHookResponse {
    url: String,
    #[serde(rename = "ws_url")]
    ws_url: String,
}

#[derive(Debug, Deserialize)]
struct WebhookSummary {
    id: u64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct WebhookDetails {
    url: String,
    #[serde(rename = "ws_url")]
    ws_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct WsEventAck {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "Header")]
    header: HashMap<String, Vec<String>>,
    #[serde(rename = "Body")]
    body: String,
}

pub fn spawn_shared_queue_webhook(
    app: AppHandle,
    repo: String,
    path: String,
    gh_path: String,
    updates_tx: Option<tokio::sync::broadcast::Sender<()>>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = run_webhook_listener(app, repo, path, gh_path, updates_tx).await {
            crate::dlog!("[Queue] Webhook listener error: {err}");
        }
    });
}

async fn run_webhook_listener(
    app: AppHandle,
    repo: String,
    path: String,
    gh_path: String,
    updates_tx: Option<tokio::sync::broadcast::Sender<()>>,
) -> Result<(), String> {
    let host = std::env::var("GH_HOST").unwrap_or_else(|_| "github.com".to_string());
    let token = gh_auth_token(&gh_path, &host).await?;

    loop {
        let hook = match create_webhook(&gh_path, &repo).await {
            Ok(hook) => hook,
            Err(err) => {
                crate::dlog!("[Queue] Webhook create error: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        let mut ws = match connect_websocket(&hook.ws_url, &token).await {
            Ok(ws) => ws,
            Err(err) => {
                crate::dlog!("[Queue] Webhook connect error: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        if let Err(err) = activate_hook(&gh_path, &hook.url).await {
            crate::dlog!("[Queue] Webhook activate error: {err}");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        crate::dlog!("[Queue] Webhook listener connected");
        let mut ping = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = ping.tick() => {
                    if let Err(err) = ws.send(Message::Ping(Vec::new().into())).await {
                        crate::dlog!("[Queue] Webhook ping error: {err}");
                        break;
                    }
                }
                msg = ws.next() => {
                    let msg = match msg {
                        Some(Ok(msg)) => msg,
                        Some(Err(err)) => {
                            crate::dlog!("[Queue] Webhook read error: {err}");
                            break;
                        }
                        None => {
                            crate::dlog!("[Queue] Webhook closed, reconnecting");
                            break;
                        }
                    };
                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                            .map_err(|e| format!("invalid websocket utf8: {e}"))?,
                        _ => continue,
                    };
                    let event_json: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(value) => value,
                        Err(err) => {
                            crate::dlog!("[Queue] Invalid websocket payload JSON: {err}");
                            continue;
                        }
                    };
                    let body = match event_json.get("Body").and_then(|v| v.as_str()) {
                        Some(body) => body,
                        None => {
                            crate::dlog!("[Queue] Webhook payload missing Body field, skipping");
                            continue;
                        }
                    };
                    let body_bytes = base64::engine::general_purpose::STANDARD
                        .decode(body.as_bytes())
                        .map_err(|e| format!("invalid webhook body encoding: {e}"))?;
                    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes)
                        .map_err(|e| format!("invalid webhook body json: {e}"))?;
                    if queue_path_touched(&body_json, &repo, &path) {
                        crate::dlog!("[Queue] Webhook event: {}", body_json);
                        let _ = app.emit("shared-queue-updated", ());
                        if let Some(tx) = updates_tx.as_ref() {
                            let _ = tx.send(());
                        }
                    }
                    let ack = WsEventAck {
                        status: 200,
                        header: HashMap::new(),
                        body: base64::engine::general_purpose::STANDARD.encode("OK"),
                    };
                    let ack_text = serde_json::to_string(&ack)
                        .map_err(|e| format!("failed to serialize webhook ack: {e}"))?;
                    if let Err(err) = ws.send(Message::Text(ack_text.into())).await {
                        crate::dlog!("[Queue] Webhook ack error: {err}");
                        break;
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn gh_auth_token(gh_path: &str, host: &str) -> Result<String, String> {
    let output = tokio::process::Command::new(gh_path)
        .args(["auth", "token", "--hostname", host])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh auth token: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn create_webhook(gh_path: &str, repo: &str) -> Result<CreateHookResponse, String> {
    for attempt in 0..2 {
        let output = tokio::process::Command::new(gh_path)
            .args([
                "api",
                "-X",
                "POST",
                &format!("repos/{repo}/hooks"),
                "-f",
                "name=cli",
                "-F",
                "active=false",
                "-f",
                "events[]=push",
                "-f",
                "config[content_type]=json",
                "-F",
                "config[insecure_ssl]=0",
            ])
            .output()
            .await
            .map_err(|e| format!("Failed to run gh api: {e}"))?;
        if output.status.success() {
            return serde_json::from_slice(&output.stdout)
                .map_err(|e| format!("Invalid webhook response: {e}"));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if attempt == 0 && stderr.contains("Validation Failed") {
            if let Ok(hooks) = list_webhooks(gh_path, repo).await {
                if let Some(hook) = hooks.into_iter().find(|h| h.name == "cli") {
                    if let Ok(details) = get_webhook(gh_path, repo, hook.id).await {
                        if let Some(ws_url) = details.ws_url {
                            return Ok(CreateHookResponse { url: details.url, ws_url });
                        }
                    }
                }
            }
        }
        return Err(stderr);
    }
    Err("Failed to create webhook".to_string())
}

async fn list_webhooks(gh_path: &str, repo: &str) -> Result<Vec<WebhookSummary>, String> {
    let output = tokio::process::Command::new(gh_path)
        .args(["api", &format!("repos/{repo}/hooks")])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh api: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid webhooks response: {e}"))
}

async fn delete_webhook(gh_path: &str, repo: &str, hook_id: u64) -> Result<(), String> {
    let output = tokio::process::Command::new(gh_path)
        .args(["api", "-X", "DELETE", &format!("repos/{repo}/hooks/{hook_id}")])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh api: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

async fn get_webhook(gh_path: &str, repo: &str, hook_id: u64) -> Result<WebhookDetails, String> {
    let output = tokio::process::Command::new(gh_path)
        .args(["api", &format!("repos/{repo}/hooks/{hook_id}")])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh api: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid webhook response: {e}"))
}

async fn activate_hook(gh_path: &str, hook_url: &str) -> Result<(), String> {
    let output = tokio::process::Command::new(gh_path)
        .args(["api", "-X", "PATCH", hook_url, "-F", "active=true"])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh api: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

async fn connect_websocket(
    ws_url: &str,
    token: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    String,
> {
    let mut request = ws_url
        .into_client_request()
        .map_err(|e| format!("Failed to build websocket request: {e}"))?;
    request.headers_mut().insert(
        "Authorization",
        http::HeaderValue::from_str(token)
            .map_err(|e| format!("Invalid auth header: {e}"))?,
    );
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("Failed to connect websocket: {e}"))?;
    Ok(ws)
}

fn queue_path_touched(body: &serde_json::Value, repo: &str, path: &str) -> bool {
    let repo_match = body
        .get("repository")
        .and_then(|repo| repo.get("full_name"))
        .and_then(|name| name.as_str())
        .map(|name| name == repo)
        .unwrap_or(false);
    if !repo_match {
        return false;
    }

    let mut touched = false;
    if let Some(commits) = body.get("commits").and_then(|c| c.as_array()) {
        for commit in commits {
            if commit_paths_include(commit, path) {
                touched = true;
                break;
            }
        }
    }
    if !touched {
        if let Some(head_commit) = body.get("head_commit") {
            touched = commit_paths_include(head_commit, path);
        }
    }
    touched
}

fn commit_paths_include(commit: &serde_json::Value, path: &str) -> bool {
    ["added", "modified", "removed"].iter().any(|key| {
        commit
            .get(*key)
            .and_then(|v| v.as_array())
            .map(|paths| paths.iter().any(|p| p.as_str() == Some(path)))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_queue_path_from_push() {
        let body = serde_json::json!({
            "repository": { "full_name": "owner/repo" },
            "commits": [
                {
                    "added": ["events.ndjson"],
                    "modified": [],
                    "removed": []
                }
            ]
        });
        assert!(queue_path_touched(&body, "owner/repo", "events.ndjson"));
        assert!(!queue_path_touched(&body, "owner/repo", "other.ndjson"));
    }

    #[test]
    fn ignores_other_repo_events() {
        let body = serde_json::json!({
            "repository": { "full_name": "other/repo" },
            "commits": [
                {
                    "added": ["events.ndjson"],
                    "modified": [],
                    "removed": []
                }
            ]
        });
        assert!(!queue_path_touched(&body, "owner/repo", "events.ndjson"));
    }
}
