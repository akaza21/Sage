//! Resolve pre-existing Sage prompts which predate durable native turn receipts.
//! One bounded native page is held at a time; repeated text is disambiguated by
//! its position in the user-message suffix and verified against the stored text.
use super::*;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::{intent::ExternalId, session_control::ConversationTarget};
use serde_json::Value;
use sha2::{Digest, Sha256};

const MAX_TARGET_PAGES: u8 = 25;

pub(super) struct TargetRead {
    target: ConversationTarget,
    fork: bool,
    request: u64,
    remaining: u32,
    pages: u8,
    cursor: Option<String>,
}

impl CodexRuntime {
    pub(super) fn targeted_control(
        &mut self,
        target: &ConversationTarget,
        fork: bool,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.target_read.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.inner.native_target(target).is_ok() {
            return if fork {
                self.fork_before(target)
            } else {
                self.request(|adapter, id| adapter.revert_to(id, target))
            };
        }
        let egress = self.request(|adapter, id| adapter.rewind_turns_page(id, None))?;
        self.target_read = Some(TargetRead {
            target: target.clone(),
            fork,
            request: request_id(&egress)?,
            remaining: target.following_messages,
            pages: 1,
            cursor: None,
        });
        Ok(egress)
    }

    pub(super) fn discard_target_read(&mut self, id: u64) -> bool {
        if !self
            .target_read
            .as_ref()
            .is_some_and(|read| read.request == id)
        {
            return false;
        }
        self.target_read = None;
        self.inner.take_pending(id);
        self.native_rpc_reply = Some(NativeRpcReply::Rejected);
        true
    }

    pub(super) fn target_ingress(
        &mut self,
        value: &Value,
    ) -> Option<Result<AdapterBatch, AdapterRuntimeError>> {
        let id = value.get("id")?.as_u64()?;
        if value.get("method").is_some()
            || !self
                .target_read
                .as_ref()
                .is_some_and(|read| read.request == id)
        {
            return None;
        }
        Some(self.read_target_page(value, id))
    }

    fn read_target_page(
        &mut self,
        value: &Value,
        id: u64,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut read = self
            .target_read
            .take()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.inner.take_pending(id);
        let Some(result) = value.get("result").filter(|_| value.get("error").is_none()) else {
            return self.target_refused();
        };
        let Some(turns) = result
            .get("data")
            .and_then(Value::as_array)
            .filter(|rows| rows.len() <= super::super::child_transcript::TURN_PAGE)
        else {
            return self.target_refused();
        };
        for turn in turns {
            if turn["status"] == "inProgress" {
                return self.target_refused();
            }
            let Some(items) = turn
                .get("items")
                .and_then(Value::as_array)
                .filter(|rows| rows.len() <= sage_acp_protocol::codex_protocol::MAX_ITEMS)
            else {
                return self.target_refused();
            };
            for item in items
                .iter()
                .rev()
                .filter(|item| item["type"] == "userMessage")
            {
                if read.remaining > 0 {
                    read.remaining -= 1;
                    continue;
                }
                let Some(parts) = item.get("content").and_then(Value::as_array) else {
                    return self.target_refused();
                };
                let mut digest = Sha256::new();
                let mut first = true;
                for text in parts
                    .iter()
                    .filter(|part| part["type"] == "text")
                    .filter_map(|part| part["text"].as_str())
                {
                    if !first {
                        digest.update(b"\n");
                    }
                    first = false;
                    digest.update(text.as_bytes());
                }
                let digest: [u8; 32] = digest.finalize().into();
                if Some(digest) != read.target.prompt_digest {
                    return self.target_refused();
                }
                let Some(native) = turn["id"]
                    .as_str()
                    .and_then(|id| ExternalId::new("nativeTurnId", id).ok())
                else {
                    return self.target_refused();
                };
                read.target.native_turn_id = Some(native);
                let egress = if read.fork {
                    self.fork_before(&read.target)?
                } else {
                    self.request(|adapter, id| adapter.revert_to(id, &read.target))?
                };
                return AdapterBatch::new(vec![AdapterEvent::Absorbed], vec![egress]);
            }
        }
        let Some(cursor) = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty() && cursor.len() <= 512)
        else {
            return self.target_refused();
        };
        if read.pages >= MAX_TARGET_PAGES || read.cursor.as_deref() == Some(cursor) {
            return self.target_refused();
        }
        let egress = self.request(|adapter, id| adapter.rewind_turns_page(id, Some(cursor)))?;
        read.request = request_id(&egress)?;
        read.pages += 1;
        read.cursor = Some(cursor.to_owned());
        self.target_read = Some(read);
        AdapterBatch::new(vec![AdapterEvent::Absorbed], vec![egress])
    }

    fn target_refused(&mut self) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.native_rpc_reply = Some(NativeRpcReply::Rejected);
        one(AdapterEvent::Absorbed)
    }
}

fn request_id(egress: &AdapterEgress) -> Result<u64, AdapterRuntimeError> {
    serde_json::from_slice::<Value>(egress.frame().payload())
        .map_err(|_| AdapterRuntimeError::Frame)?["id"]
        .as_u64()
        .ok_or(AdapterRuntimeError::Frame)
}
