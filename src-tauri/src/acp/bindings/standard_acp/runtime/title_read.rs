use super::StandardAcpRuntime;
use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(super) struct TitleRead {
    session: String,
    ids: Vec<u64>,
    waiting: Option<u64>,
    read_at: Instant,
    pub(super) finished: bool,
    pub(super) result: Option<Option<String>>,
}

impl StandardAcpRuntime {
    pub(super) fn begin_title_read(&mut self) -> Result<(), AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() || !self.pending.is_empty() {
            return Err(AdapterRuntimeError::Operation);
        }
        self.title_read = Some(TitleRead {
            session: active.external_id.as_str().to_owned(),
            ids: Vec::with_capacity(2),
            waiting: None,
            read_at: Instant::now(),
            finished: false,
            result: None,
        });
        Ok(())
    }

    pub(super) fn poll_title_read(&mut self) -> Option<AdapterEgress> {
        let read = self.title_read.as_mut()?;
        if read.finished || read.waiting.is_some() || Instant::now() < read.read_at {
            return None;
        }
        let id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)?;
        let cwd = self.workspace_root.to_str()?;
        let frame = super::outbound(
            json!({"jsonrpc":"2.0","id":id,"method":"session/list","params":{"cwd":cwd}}),
        )
        .ok()?;
        self.next_request_id = id;
        read.ids.push(id);
        read.waiting = Some(id);
        Some(AdapterEgress::new(
            frame,
            EgressToken::native_rpc_continuation(id),
        ))
    }

    pub(super) fn read_title_response(&mut self, value: &Value, bytes: usize) -> bool {
        let Some(read) = &mut self.title_read else {
            return false;
        };
        if value.get("method").is_some()
            || !value["id"]
                .as_u64()
                .is_some_and(|id| read.ids.contains(&id))
        {
            return false;
        }
        if read.finished || value["id"].as_u64() != read.waiting {
            return true;
        }
        read.waiting = None;
        match select_title(value, &read.session, bytes) {
            Selection::Title(title) => {
                read.result = Some(Some(title));
                read.finished = true;
            }
            Selection::Default if read.ids.len() == 1 => {
                read.read_at = Instant::now() + Duration::from_millis(500);
            }
            Selection::Default | Selection::Rejected => {
                read.result = Some(None);
                read.finished = true;
            }
        }
        true
    }

    pub(super) fn title_read_write(
        &mut self,
        id: u64,
        resolution: crate::acp::bindings::adapter::EgressResolution,
    ) -> bool {
        let Some(read) = &mut self.title_read else {
            return false;
        };
        if !read.ids.contains(&id) {
            return false;
        }
        use crate::acp::bindings::adapter::EgressResolution::*;
        if matches!(resolution, QueueRejected | WriteZeroBytes | DeliveryUnknown) {
            read.finished = true;
        }
        if resolution == DeliveryUnknown {
            self.delivery_unknown = Some(id);
        }
        true
    }
}

enum Selection {
    Title(String),
    Default,
    Rejected,
}

fn select_title(value: &Value, session: &str, bytes: usize) -> Selection {
    if bytes > 64 * 1024 || value.get("error").is_some() {
        return Selection::Rejected;
    }
    let Some(sessions) = value
        .pointer("/result/sessions")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() <= 100)
    else {
        return Selection::Rejected;
    };
    let mut exact = sessions
        .iter()
        .filter(|row| row["sessionId"].as_str() == Some(session));
    let Some(row) = exact.next() else {
        return Selection::Rejected;
    };
    if exact.next().is_some() {
        return Selection::Rejected;
    }
    let Some(title) = row["title"].as_str().filter(|title| title.len() <= 1024) else {
        return Selection::Rejected;
    };
    if default_title(title) {
        return Selection::Default;
    }
    crate::chat_lifecycle::generated_title(title).map_or(Selection::Rejected, Selection::Title)
}

fn default_title(title: &str) -> bool {
    let title = title.trim();
    ["New session", "Untitled", "New chat"]
        .iter()
        .any(|default| title.eq_ignore_ascii_case(default))
        || ["New session - ", "Child session - "].iter().any(|prefix| {
            title.strip_prefix(prefix).is_some_and(|stamp| {
                stamp.len() >= 19
                    && stamp.as_bytes().get(4) == Some(&b'-')
                    && stamp.as_bytes().get(10) == Some(&b'T')
                    && stamp
                        .chars()
                        .all(|c| c.is_ascii_digit() || "-:TZ.+".contains(c))
            })
        })
}
