//! Where `codex app-server` frames name their thread, for the shared host's
//! router. Verified against codex 0.167.1: every thread-scoped notification
//! and server request carries `params.threadId` (`thread/started` carries
//! `params.thread.id`), and `thread/start`, `thread/resume` and `thread/fork`
//! answer with `result.thread.id`.

use serde_json::Value;

use crate::acp::runtime::shared_host::ScopePolicy;

pub(crate) struct CodexScope;

impl ScopePolicy for CodexScope {
    fn thread_of<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        let params = frame.get("params")?;
        params
            .get("threadId")
            .or_else(|| params.get("thread").and_then(|thread| thread.get("id")))
            .and_then(Value::as_str)
    }

    fn opens_thread(&self, method: &str) -> bool {
        // `thread/read` and `thread/rollback` also answer with `result.thread`
        // but do not give the asking chat the thread.
        matches!(method, "thread/start" | "thread/resume" | "thread/fork")
    }

    fn thread_in_result<'a>(&self, result: &'a Value) -> Option<&'a str> {
        result.get("thread")?.get("id")?.as_str()
    }

    fn child_announced<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        if !matches!(
            frame.get("method").and_then(Value::as_str),
            Some("item/started" | "item/completed")
        ) {
            return None;
        }
        let item = frame.get("params")?.get("item")?;
        if item.get("type").and_then(Value::as_str) != Some("subAgentActivity") {
            return None;
        }
        item.get("agentThreadId").and_then(Value::as_str)
    }

    fn request_resolved<'a>(&self, frame: &'a Value) -> Option<&'a Value> {
        if frame.get("method").and_then(Value::as_str) != Some("serverRequest/resolved") {
            return None;
        }
        frame.get("params")?.get("requestId")
    }

    fn thread_closed<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        if frame.get("method").and_then(Value::as_str) != Some("thread/closed") {
            return None;
        }
        frame.get("params")?.get("threadId")?.as_str()
    }

    fn is_handshake_request(&self, method: &str) -> bool {
        method == "initialize"
    }

    fn is_handshake_notification(&self, method: &str) -> bool {
        method == "initialized"
    }
}

#[cfg(test)]
#[path = "shared_scope_tests.rs"]
mod tests;
