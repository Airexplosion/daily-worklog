//! OpenAI 兼容的 chat/completions 客户端（普通 + 流式）。
//! 走 Windows 系统代理（reqwest system-proxy），TLS 用系统证书（native-tls）。

use crate::model::ModelConfig;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::fmt;
use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP 客户端初始化失败")
});

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    NoKey,
    Network(String),
    Timeout,
    Http(u16, String),
    BadResponse(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NoKey => write!(f, "还没填接口信息（接口地址、API Key、模型）"),
            AiError::Network(_) => write!(f, "连不上 AI 服务，检查一下网络或接口地址"),
            AiError::Timeout => write!(f, "AI 服务响应超时"),
            AiError::Http(code, msg) => write!(f, "AI 服务返回错误 {code}：{msg}"),
            AiError::BadResponse(e) => write!(f, "AI 返回的内容看不懂：{e}"),
        }
    }
}

pub fn endpoint(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

/// choices[0].message.content，兼容字符串和分段数组两种写法
pub fn extract_content(v: &Value) -> Option<String> {
    let content = v.get("choices")?.get(0)?.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect()),
        _ => None,
    }
}

/// 从错误响应里取出可读的说明
pub fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str().map(|s| s.to_string()))
        })
        .unwrap_or_else(|| body.chars().take(200).collect())
}

fn check_config(cfg: &ModelConfig) -> Result<(), AiError> {
    if cfg.base_url.trim().is_empty() || cfg.api_key.trim().is_empty() || cfg.model.trim().is_empty() {
        return Err(AiError::NoKey);
    }
    Ok(())
}

fn map_err(e: reqwest::Error) -> AiError {
    log::warn!("AI 请求出错：{e}");
    if e.is_timeout() {
        AiError::Timeout
    } else {
        AiError::Network(e.without_url().to_string())
    }
}

async fn send(cfg: &ModelConfig, body: Value, timeout: Duration) -> Result<reqwest::Response, AiError> {
    check_config(cfg)?;
    let resp = CLIENT
        .post(endpoint(&cfg.base_url))
        .bearer_auth(cfg.api_key.trim())
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .map_err(map_err)?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AiError::Http(status.as_u16(), error_message(&text)));
    }
    Ok(resp)
}

pub async fn chat(cfg: &ModelConfig, messages: Value, timeout: Duration) -> Result<String, AiError> {
    let body = json!({ "model": cfg.model.trim(), "messages": messages, "temperature": 0.2, "stream": false });
    let resp = send(cfg, body, timeout).await?;
    let v: Value = resp.json().await.map_err(|e| AiError::BadResponse(e.to_string()))?;
    extract_content(&v).ok_or_else(|| AiError::BadResponse("没有 choices[0].message.content".into()))
}

/// 流式：每收到一段文字回调一次，返回完整文字。服务商不支持流式时按普通响应处理
pub async fn chat_stream(
    cfg: &ModelConfig,
    messages: Value,
    timeout: Duration,
    mut on_delta: impl FnMut(&str),
) -> Result<String, AiError> {
    let body = json!({ "model": cfg.model.trim(), "messages": messages, "temperature": 0.3, "stream": true });
    let resp = send(cfg, body, timeout).await?;
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("event-stream"));
    if !is_sse {
        let v: Value = resp.json().await.map_err(|e| AiError::BadResponse(e.to_string()))?;
        let text = extract_content(&v).ok_or_else(|| AiError::BadResponse("没有内容".into()))?;
        on_delta(&text);
        return Ok(text);
    }
    let mut parser = SseParser::default();
    let mut full = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_err)?;
        for delta in parser.push(&String::from_utf8_lossy(&chunk)) {
            full.push_str(&delta);
            on_delta(&delta);
        }
        if parser.done {
            break;
        }
    }
    Ok(full)
}

/// 逐块喂入 SSE 文本，吐出 choices[0].delta.content；忽略思考内容
#[derive(Default)]
pub struct SseParser {
    buf: String,
    pub done: bool,
}

impl SseParser {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buf.push_str(&chunk.replace("\r\n", "\n"));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            let Some(data) = line.trim().strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                self.done = true;
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(s) = v.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_joins_path() {
        assert_eq!(endpoint("https://api.x.com/v1/"), "https://api.x.com/v1/chat/completions");
        assert_eq!(endpoint("https://api.x.com/v1"), "https://api.x.com/v1/chat/completions");
        assert_eq!(endpoint("https://api.x.com/v1/chat/completions"), "https://api.x.com/v1/chat/completions");
    }

    #[test]
    fn extract_content_variants() {
        let v = json!({"choices":[{"message":{"content":"你好"}}]});
        assert_eq!(extract_content(&v).as_deref(), Some("你好"));
        let parts = json!({"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]});
        assert_eq!(extract_content(&parts).as_deref(), Some("ab"));
        assert_eq!(extract_content(&json!({"choices":[]})), None);
    }

    #[test]
    fn sse_parser_handles_split_chunks_and_done() {
        let mut p = SseParser::default();
        let mut out = Vec::new();
        out.extend(p.push("data: {\"choices\":[{\"delta\":{\"content\":\"1.拼\"}}]}\n\nda"));
        out.extend(p.push("ta: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\n\n"));
        out.extend(p.push("data: {\"choices\":[{\"delta\":{\"content\":\"多多\"}}]}\n\ndata: [DONE]\n\n"));
        assert_eq!(out, vec!["1.拼".to_string(), "多多".to_string()]);
        assert!(p.done);
    }

    #[test]
    fn api_error_message_extracted() {
        let body = r#"{"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}"#;
        assert_eq!(error_message(body), "Incorrect API key provided");
        assert_eq!(error_message("<html>bad gateway</html>"), "<html>bad gateway</html>");
    }

    #[test]
    fn error_display_is_chinese() {
        assert!(AiError::NoKey.to_string().contains("接口"));
        assert!(AiError::Timeout.to_string().contains("超时"));
        assert!(AiError::Http(401, "bad key".into()).to_string().contains("401"));
    }
}
