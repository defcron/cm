use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;

use crate::config::Config;

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct Metadata<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<&'a str>,
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Metadata<'a>>,
}

pub struct SendResult {
    /// Full assistant reply text (already printed to stdout as it streamed,
    /// or about to be printed once if non-streaming).
    pub reply: String,
    /// The upstream Mirror conversation id, if this call is threadable
    /// (store:true, which is always true for cm - "new" just omits the
    /// existing id so the server mints a fresh one).
    pub conversation_id: Option<String>,
}

pub async fn send_message(
    cfg: &Config,
    message: &str,
    system: Option<&str>,
    conversation_id: Option<&str>,
    model_override: Option<&str>,
    stream: bool,
) -> Result<SendResult> {
    let client = reqwest::Client::builder()
        .build()
        .context("building HTTP client")?;

    let model = model_override.unwrap_or(&cfg.model);

    let mut messages = Vec::new();
    if let Some(sys) = system {
        messages.push(ChatMessage {
            role: "system",
            content: sys,
        });
    }
    messages.push(ChatMessage {
        role: "user",
        content: message,
    });

    let body = CompletionRequest {
        model,
        messages,
        stream,
        store: true,
        metadata: Some(Metadata { conversation_id }),
    };

    let url = format!("{}/v1/chat/completions", cfg.base_url);
    let mut req = client.post(&url).json(&body);
    if let Some(key) = &cfg.api_key {
        req = req.bearer_auth(key);
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("sending request to {url}"))?;

    let status = resp.status();
    let new_conversation_id = resp
        .headers()
        .get("x-mirror-conversation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("Mirror API returned {status}: {text}");
    }

    if stream {
        let (reply, stream_conversation_id) = read_sse_stream(resp).await?;
        Ok(SendResult {
            reply,
            // Mirror can't set the x-mirror-conversation-id HTTP header on a
            // streamed reply (headers are already flushed by the time the
            // conversation id is known for a brand-new thread), so it sends
            // an SSE comment line instead - see read_sse_stream. Prefer the
            // header if we somehow got one, else fall back to that.
            conversation_id: new_conversation_id.or(stream_conversation_id),
        })
    } else {
        let json: Value = resp.json().await.context("parsing JSON response")?;
        let reply = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("unexpected response shape: {json}"))?
            .to_string();
        Ok(SendResult {
            reply,
            conversation_id: new_conversation_id,
        })
    }
}

/// Reads an OpenAI-style `data: {...}` SSE stream of chat.completion.chunk
/// objects, printing each delta's content to stdout as it arrives (flushed
/// per chunk for a responsive feel) and returning the full accumulated
/// reply text once `data: [DONE]` is seen or the stream ends.
///
/// Also watches for Mirror's out-of-band `: mirror-conversation-id <id>` SSE
/// comment line - a plain HTTP header can't be used here because, for a
/// brand-new conversation, Mirror doesn't know the id yet when it has to
/// flush the response headers to open the stream, so it sends the id as an
/// SSE comment (ignored by any spec-compliant SSE client) once it's known.
async fn read_sse_stream(resp: reqwest::Response) -> Result<(String, Option<String>)> {
    let mut byte_stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full_reply = String::new();
    let mut conversation_id: Option<String> = None;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();

    while let Some(chunk) = byte_stream.next().await {
        let chunk: Bytes = chunk.context("reading response stream")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete lines; SSE frames are separated by blank lines
        // but each data line is independently parseable, so line-by-line is
        // sufficient here.
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            buf.drain(..=pos);

            if let Some(rest) = line.strip_prefix(':') {
                if let Some(id) = rest.trim().strip_prefix("mirror-conversation-id ") {
                    conversation_id = Some(id.trim().to_string());
                }
                continue;
            }

            let Some(data) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:"))
            else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                let _ = lock.flush();
                return Ok((full_reply, conversation_id));
            }
            let Ok(json): Result<Value, _> = serde_json::from_str(data) else {
                continue;
            };
            if let Some(piece) = json["choices"][0]["delta"]["content"].as_str() {
                full_reply.push_str(piece);
                let _ = lock.write_all(piece.as_bytes());
                let _ = lock.flush();
            }
        }
    }
    let _ = lock.flush();
    Ok((full_reply, conversation_id))
}
