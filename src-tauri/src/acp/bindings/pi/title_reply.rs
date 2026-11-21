//! Bounded Pi RPC title output; no events enter the main translator.
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

pub(super) async fn read_title(stdout: impl AsyncRead + Unpin) -> Option<String> {
    let mut output = BufReader::new(stdout.take(64 * 1024));
    let mut title = None;
    let mut streamed_bytes = 0;
    loop {
        let mut line = Vec::new();
        (&mut output)
            .take(16 * 1024)
            .read_until(b'\n', &mut line)
            .await
            .ok()?;
        if line.last() != Some(&b'\n') {
            return None;
        }
        let value: serde_json::Value = serde_json::from_slice(&line).ok()?;
        match value.get("type")?.as_str()? {
            "message_update" if value["assistantMessageEvent"]["type"] == "text_delta" => {
                streamed_bytes += value["assistantMessageEvent"]["delta"].as_str()?.len();
                if streamed_bytes > 1024 {
                    return None;
                }
            }
            "response" if value.get("success") != Some(&serde_json::Value::Bool(true)) => {
                tracing::info!(command = ?value.get("command"), "Isolated Pi title RPC rejected");
                return None;
            }
            "message_end" if value["message"]["role"] == "assistant" => {
                let message = &value["message"];
                if message["stopReason"] != "stop" {
                    tracing::info!(reason = ?message.get("stopReason"), "Isolated Pi title assistant failed");
                    return None;
                }
                let mut text = String::new();
                for block in message["content"].as_array()? {
                    if block["type"] == "text" {
                        let part = block["text"].as_str()?;
                        if text.len() + part.len() > 1024 {
                            return None;
                        }
                        text.push_str(part);
                    }
                }
                title = crate::chat_lifecycle::generated_title(&text);
                tracing::info!(accepted = title.is_some(), "Isolated Pi title reply parsed");
            }
            "agent_settled" => return title,
            "auto_retry_start" | "auto_compaction_start" => return None,
            _ => {}
        }
    }
}
