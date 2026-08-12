use serde::Serialize;
use serde_json::{Value, json};

use crate::acp::{
    bindings::codex::{
        incoming::{IncomingEvent, IngressDisposition},
        manifest::{MetadataValidatedPacket, Platform},
        state::CodexAdapter,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    transport::{Frame, OutboundFrame},
};

#[path = "discovery/runner.rs"]
mod runner;
#[path = "discovery/schema.rs"]
mod schema;
#[path = "discovery/transport.rs"]
mod transport;
pub(crate) use runner::{CodexDiscoveryDeadline, CodexDiscoveryRunError};
#[cfg(test)]
pub(crate) use runner::{
    DiscoveryTransport, DiscoveryZeroProcess, RevalidationCheckpoint, run_discovery_protocol,
};
use schema::{
    NoResources, json_request, managed_chatgpt, parse_cursor, parse_model, request_frame,
    require_response_id, response_result,
};
pub(crate) use transport::{AdmittedDiscovery, discover_admitted};

const MODEL_PAGE_SIZE: u32 = 32;
const MODEL_PAGE_LIMIT: usize = 4;
const MODEL_LIMIT: usize = 128;
const MODEL_RETAINED_BYTES: usize = 131_072;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodexAccountSnapshot {
    ManagedChatgpt,
    SignedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexModelSnapshot {
    pub(crate) id: RequiredText<512>,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexDiscoveryContent {
    pub(crate) account: CodexAccountSnapshot,
    pub(crate) models: Vec<CodexModelSnapshot>,
    pub(crate) default_model_id: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProtocolProgress {
    More,
    Complete(CodexDiscoveryContent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexDiscoveryError {
    #[error("Codex discovery protocol failed")]
    Protocol,
    #[error("Codex account is outside the admitted managed ChatGPT policy")]
    AccountPolicy,
    #[error("Codex model catalog violates its bounded schema")]
    ModelCatalog,
    #[error("Codex returned an empty model catalog")]
    EmptyCatalog,
    #[error("Codex model pagination repeated a cursor")]
    CursorLoop,
    #[error("Codex model pagination exceeded four pages")]
    PageLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    AwaitInitialize(u64),
    Initialized,
    AwaitAccount(u64),
    AccountReady,
    AwaitModels(u64),
    Complete,
}

pub(crate) struct DiscoveryRequest {
    id: u64,
    frame: OutboundFrame,
}

impl DiscoveryRequest {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn payload(&self) -> &[u8] {
        self.frame.payload()
    }

    pub(crate) fn into_frame(self) -> OutboundFrame {
        self.frame
    }
}

pub(crate) struct CodexDiscoveryProtocol {
    adapter: CodexAdapter,
    platform: Platform,
    phase: Phase,
    next_request_id: u64,
    account: Option<CodexAccountSnapshot>,
    models: Vec<CodexModelSnapshot>,
    retained_model_bytes: usize,
    default_model_id: Option<RequiredText<512>>,
    next_cursor: Option<RequiredText<512>>,
    seen_cursors: Vec<RequiredText<512>>,
    pages: usize,
}

impl CodexDiscoveryProtocol {
    pub(crate) fn new(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
    ) -> Self {
        Self {
            adapter: CodexAdapter::from_metadata_packet(
                packet,
                binding_digest,
                runtime_epoch,
                SageAgentSessionId::parse(format!(
                    "session-codex-discovery-{}",
                    runtime_epoch.get()
                ))
                .expect("bounded discovery identity"),
            ),
            platform: packet.metadata().platform,
            phase: Phase::New,
            next_request_id: 0,
            account: None,
            models: Vec::new(),
            retained_model_bytes: 0,
            default_model_id: None,
            next_cursor: None,
            seen_cursors: Vec::new(),
            pages: 0,
        }
    }

    pub(crate) fn platform(&self) -> Platform {
        self.platform
    }

    pub(crate) fn initialize_request(&mut self) -> Result<DiscoveryRequest, CodexDiscoveryError> {
        if self.phase != Phase::New {
            return Err(CodexDiscoveryError::Protocol);
        }
        let id = self.next_id()?;
        let request = self
            .adapter
            .initialize(id)
            .map_err(|_| CodexDiscoveryError::Protocol)?;
        self.phase = Phase::AwaitInitialize(id);
        request_frame(id, request.bytes)
    }

    pub(crate) fn accept_initialize(
        &mut self,
        frame: &Frame,
    ) -> Result<OutboundFrame, CodexDiscoveryError> {
        let Phase::AwaitInitialize(expected) = self.phase else {
            return Err(CodexDiscoveryError::Protocol);
        };
        require_response_id(frame, expected)?;
        let disposition = self
            .adapter
            .ingest(frame.as_bytes(), &NoResources)
            .map_err(|_| CodexDiscoveryError::Protocol)?;
        let IngressDisposition::Events(events) = disposition else {
            return Err(CodexDiscoveryError::Protocol);
        };
        if !matches!(events.as_slice(), [IncomingEvent::Initialized { .. }]) {
            return Err(CodexDiscoveryError::Protocol);
        }
        let initialized = self
            .adapter
            .initialized()
            .map_err(|_| CodexDiscoveryError::Protocol)?;
        self.phase = Phase::Initialized;
        OutboundFrame::new(initialized).map_err(|_| CodexDiscoveryError::Protocol)
    }

    pub(crate) fn account_request(&mut self) -> Result<DiscoveryRequest, CodexDiscoveryError> {
        if self.phase != Phase::Initialized {
            return Err(CodexDiscoveryError::Protocol);
        }
        let id = self.next_id()?;
        self.phase = Phase::AwaitAccount(id);
        json_request(id, "account/read", json!({}))
    }

    pub(crate) fn accept_account(
        &mut self,
        frame: &Frame,
    ) -> Result<CodexAccountSnapshot, CodexDiscoveryError> {
        let Phase::AwaitAccount(expected) = self.phase else {
            return Err(CodexDiscoveryError::Protocol);
        };
        let result = response_result(frame, expected)?;
        if result.len() != 2
            || !result.contains_key("account")
            || !result.contains_key("requiresOpenaiAuth")
        {
            return Err(CodexDiscoveryError::Protocol);
        }
        let requires_auth = result
            .get("requiresOpenaiAuth")
            .and_then(Value::as_bool)
            .ok_or(CodexDiscoveryError::Protocol)?;
        let account = match result.get("account") {
            Some(Value::Null) if requires_auth => CodexAccountSnapshot::SignedOut,
            Some(Value::Object(account)) if managed_chatgpt(account)? => {
                CodexAccountSnapshot::ManagedChatgpt
            }
            _ => return Err(CodexDiscoveryError::AccountPolicy),
        };
        self.account = Some(account);
        self.phase = Phase::AccountReady;
        Ok(account)
    }

    pub(crate) fn model_request(&mut self) -> Result<DiscoveryRequest, CodexDiscoveryError> {
        if self.phase != Phase::AccountReady || self.pages >= MODEL_PAGE_LIMIT {
            return Err(if self.pages >= MODEL_PAGE_LIMIT {
                CodexDiscoveryError::PageLimit
            } else {
                CodexDiscoveryError::Protocol
            });
        }
        let id = self.next_id()?;
        let cursor = self.next_cursor.as_ref().map(RequiredText::as_str);
        self.phase = Phase::AwaitModels(id);
        json_request(
            id,
            "model/list",
            json!({
                "cursor": cursor,
                "limit": MODEL_PAGE_SIZE,
                "includeHidden": false
            }),
        )
    }

    pub(crate) fn accept_model_page(
        &mut self,
        frame: &Frame,
    ) -> Result<ProtocolProgress, CodexDiscoveryError> {
        let Phase::AwaitModels(expected) = self.phase else {
            return Err(CodexDiscoveryError::Protocol);
        };
        let result = response_result(frame, expected)?;
        if result.len() != 2 || !result.contains_key("data") || !result.contains_key("nextCursor") {
            return Err(CodexDiscoveryError::Protocol);
        }
        let data = result
            .get("data")
            .and_then(Value::as_array)
            .ok_or(CodexDiscoveryError::Protocol)?;
        if data.len() > MODEL_PAGE_SIZE as usize
            || self.models.len().saturating_add(data.len()) > MODEL_LIMIT
        {
            return Err(CodexDiscoveryError::ModelCatalog);
        }

        let mut page_models = Vec::with_capacity(data.len());
        let mut page_bytes = 0_usize;
        let mut page_default: Option<RequiredText<512>> = None;
        for value in data {
            let model = parse_model(value)?;
            if self
                .models
                .iter()
                .chain(page_models.iter())
                .any(|existing: &CodexModelSnapshot| existing.id.as_str() == model.id.as_str())
            {
                return Err(CodexDiscoveryError::ModelCatalog);
            }
            page_bytes = page_bytes
                .checked_add(model.id.as_str().len())
                .and_then(|bytes| bytes.checked_add(model.name.as_str().len()))
                .ok_or(CodexDiscoveryError::ModelCatalog)?;
            if model.is_default {
                if self.default_model_id.is_some() || page_default.is_some() {
                    return Err(CodexDiscoveryError::ModelCatalog);
                }
                page_default = Some(model.id.clone());
            }
            page_models.push(model);
        }
        let retained = self
            .retained_model_bytes
            .checked_add(page_bytes)
            .filter(|bytes| *bytes <= MODEL_RETAINED_BYTES)
            .ok_or(CodexDiscoveryError::ModelCatalog)?;
        let next_cursor = parse_cursor(result.get("nextCursor"))?;
        if let Some(cursor) = &next_cursor {
            if self
                .seen_cursors
                .iter()
                .any(|seen| seen.as_str() == cursor.as_str())
            {
                return Err(CodexDiscoveryError::CursorLoop);
            }
            if self.pages + 1 == MODEL_PAGE_LIMIT {
                return Err(CodexDiscoveryError::PageLimit);
            }
        }

        self.models.extend(page_models);
        self.retained_model_bytes = retained;
        if page_default.is_some() {
            self.default_model_id = page_default;
        }
        self.pages += 1;
        if let Some(cursor) = next_cursor {
            self.seen_cursors.push(cursor.clone());
            self.next_cursor = Some(cursor);
            self.phase = Phase::AccountReady;
            return Ok(ProtocolProgress::More);
        }
        if self.models.is_empty() {
            return Err(CodexDiscoveryError::EmptyCatalog);
        }
        self.next_cursor = None;
        self.phase = Phase::Complete;
        Ok(ProtocolProgress::Complete(CodexDiscoveryContent {
            account: self.account.ok_or(CodexDiscoveryError::Protocol)?,
            models: std::mem::take(&mut self.models),
            default_model_id: self.default_model_id.take(),
        }))
    }

    fn next_id(&mut self) -> Result<u64, CodexDiscoveryError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(CodexDiscoveryError::Protocol)?;
        Ok(self.next_request_id)
    }
}

#[cfg(test)]
#[path = "discovery/tests.rs"]
mod tests;
