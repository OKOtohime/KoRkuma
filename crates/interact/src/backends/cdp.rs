use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use koakuma_core::domain::{TargetSelector, UiOp};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::backend::{InteractionBackend, ResolvedTarget, TargetInner, Tier, UiNode};
use crate::error::BackendError;

/// Chrome DevTools Protocol backend.
///
/// Connects to a browser's remote debugging port (default `9222`) to resolve
/// [`TargetSelector::BrowserTab`] targets and execute JS-based operations.
///
/// # Usage
///
/// Start the browser with `--remote-debugging-port=9222`, then register this backend:
///
/// ```rust,no_run
/// use koakuma_interact::backends::cdp::CdpBackend;
/// use koakuma_interact::negotiator::BackendRegistry;
///
/// let mut reg = BackendRegistry::new();
/// reg.register(CdpBackend::new(9222));
/// ```
pub struct CdpBackend {
    port: u16,
}

impl CdpBackend {
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

static CDP_ID: AtomicU64 = AtomicU64::new(1);

/// List browser tabs via the CDP HTTP `/json` endpoint (no external deps).
async fn fetch_tabs(port: u16) -> Result<Vec<Value>, BackendError> {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| BackendError::NotAvailable(format!("CDP port {port}: {e}")))?;

    let req = format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| BackendError::Internal(e.to_string()))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| BackendError::Internal(e.to_string()))?;

    let text = String::from_utf8_lossy(&buf);
    let body = text
        .find("\r\n\r\n")
        .map(|i| &text[i + 4..])
        .unwrap_or(&text);

    serde_json::from_str(body)
        .map_err(|e| BackendError::Internal(format!("CDP JSON parse: {e}")))
}

/// Send a single CDP command over WebSocket and return the `result` field.
async fn cdp_call(ws_url: &str, method: &str, params: Value) -> Result<Value, BackendError> {
    let id = CDP_ID.fetch_add(1, Ordering::Relaxed);
    let cmd = json!({"id": id, "method": method, "params": params});

    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| BackendError::NotAvailable(format!("CDP WS connect: {e}")))?;

    ws.send(Message::text(cmd.to_string()))
        .await
        .map_err(|e| BackendError::Internal(format!("CDP send: {e}")))?;

    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| BackendError::Internal(format!("CDP recv: {e}")))?;
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| BackendError::Internal(format!("CDP JSON: {e}")))?;
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(BackendError::Internal(format!("CDP error: {err}")));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    Err(BackendError::Internal(
        "CDP connection closed without response".into(),
    ))
}

#[async_trait]
impl InteractionBackend for CdpBackend {
    fn id(&self) -> &'static str {
        "cdp"
    }

    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError> {
        let url_pattern = match sel {
            TargetSelector::BrowserTab { url_pattern } => url_pattern.as_str(),
            _ => return Ok(vec![]),
        };

        let port = self.port;
        let tabs = fetch_tabs(port).await?;

        let targets = tabs
            .into_iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("page"))
            .filter(|t| {
                t.get("url")
                    .and_then(|v| v.as_str())
                    .map(|u| u.contains(url_pattern))
                    .unwrap_or(false)
            })
            .filter_map(|t| {
                let target_id = t.get("id")?.as_str()?.to_string();
                let ws_url = t.get("webSocketDebuggerUrl")?.as_str()?.to_string();
                let url = t.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = t.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                Some(ResolvedTarget {
                    backend_id: "cdp",
                    display_name: format!("{title} — {url}"),
                    inner: TargetInner::BrowserTab { port, target_id, ws_url, url },
                })
            })
            .collect();

        Ok(targets)
    }

    fn capability(&self, t: &ResolvedTarget) -> Tier {
        if matches!(t.inner, TargetInner::BrowserTab { .. }) {
            Tier::Background
        } else {
            Tier::Unsupported
        }
    }

    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError> {
        let ws_url = match &t.inner {
            TargetInner::BrowserTab { ws_url, .. } => ws_url.clone(),
            _ => return Err(BackendError::NotSupported("not a browser tab".into())),
        };
        let script = op_to_js(op)?;
        cdp_call(
            &ws_url,
            "Runtime.evaluate",
            json!({"expression": script, "returnByValue": true}),
        )
        .await?;
        Ok(())
    }

    async fn enumerate(&self, t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError> {
        let ws_url = match &t.inner {
            TargetInner::BrowserTab { ws_url, .. } => ws_url.clone(),
            _ => return Err(BackendError::NotSupported("not a browser tab".into())),
        };

        let script = r#"
            Array.from(document.querySelectorAll('button,input,a,select,textarea'))
                 .map(e => ({
                     path: e.id ? '#' + e.id : e.tagName.toLowerCase(),
                     name: (e.textContent||e.value||e.name||'').trim().substring(0,80),
                     type: e.tagName.toLowerCase()
                 }))
        "#;

        let result = cdp_call(
            &ws_url,
            "Runtime.evaluate",
            json!({"expression": script, "returnByValue": true}),
        )
        .await?;

        let nodes = result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|n| {
                        Some(UiNode {
                            path: n.get("path")?.as_str()?.to_string(),
                            name: n.get("name")?.as_str()?.to_string(),
                            control_type: n.get("type")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(nodes)
    }
}

fn op_to_js(op: &UiOp) -> Result<String, BackendError> {
    Ok(match op {
        UiOp::Click { node } => {
            format!("document.querySelector({}).click()", js_string(node))
        }
        UiOp::SetText { node, text } => {
            format!(
                "(function(){{var e=document.querySelector({n});e.value={v};e.dispatchEvent(new Event('input',{{bubbles:true}}));}})();",
                n = js_string(node),
                v = js_string(text)
            )
        }
        UiOp::Focus { node: Some(node) } => {
            format!("document.querySelector({}).focus()", js_string(node))
        }
        UiOp::Focus { node: None } => "window.focus()".into(),
        UiOp::ReadValue { node } => {
            format!("document.querySelector({}).value", js_string(node))
        }
        UiOp::SendKeys { keys } => {
            let repr: Vec<String> = keys
                .iter()
                .map(|k| format!("{}+{}", k.modifiers.join("+"), k.key))
                .collect();
            format!("/* SendKeys({}) not fully supported via JS */", repr.join(", "))
        }
    })
}

fn js_string(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
