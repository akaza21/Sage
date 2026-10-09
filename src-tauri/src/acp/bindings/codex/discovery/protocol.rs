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

use super::schema::{
    NoResources, json_request, managed_chatgpt, parse_cursor, parse_model, request_frame,
    require_response_id, response_result,
};
use super::types::{
    CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryError, CodexModelSnapshot,
    ProtocolProgress,
};

const MODEL_PAGE_SIZE: u32 = 32;
const MODEL_LIMIT: usize = 128;
const MODEL_RETAINED_BYTES: usize = 131_072;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    AwaitInitialize(u64),
    Initialized,
    AwaitAccount {
        account_id: u64,
        model_id: Option<u64>,
        model_complete: bool,
    },
    AccountReady,
    AwaitModels(u64),
    Complete,
}

pub(crate) struct DiscoveryRequest {
    pub(super) id: u64,
    pub(super) frame: OutboundFrame,
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
        Self::with_adapter(
            CodexAdapter::from_metadata_packet(
                packet,
                binding_digest,
                runtime_epoch,
                SageAgentSessionId::parse(format!(
                    "session-codex-discovery-{}",
                    runtime_epoch.get()
                ))
                .expect("bounded discovery identity"),
            ),
            packet.metadata().platform,
        )
    }

    pub(crate) fn from_admitted_binding(
        binding: &crate::acp::bindings::codex::AdmittedCodexBinding,
        runtime_epoch: RuntimeEpoch,
    ) -> Self {
        Self::with_adapter(
            CodexAdapter::from_admitted_binding(
                binding,
                runtime_epoch,
                SageAgentSessionId::parse(format!(
                    "session-codex-discovery-{}",
                    runtime_epoch.get()
                ))
                .expect("bounded discovery identity"),
            ),
            binding.packet().metadata().platform,
        )
    }

    fn with_adapter(adapter: CodexAdapter, platform: Platform) -> Self {
        Self {
            adapter,
            platform,
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
        super::diagnostics::log_initialize_response(frame);
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
        self.phase = Phase::AwaitAccount {
            account_id: id,
            model_id: None,
            model_complete: false,
        };
        json_request(id, "account/read", json!({}))
    }

    pub(crate) fn accept_account(
        &mut self,
        frame: &Frame,
    ) -> Result<CodexAccountSnapshot, CodexDiscoveryError> {
        let (expected, model_id, model_complete) = match self.phase {
            Phase::AwaitAccount {
                account_id,
                model_id,
                model_complete,
            } => (account_id, model_id, model_complete),
            _ => return Err(CodexDiscoveryError::Protocol),
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
            Some(Value::Null) => CodexAccountSnapshot::ProviderManaged,
            Some(Value::Object(account)) if managed_chatgpt(account)? => {
                CodexAccountSnapshot::ManagedChatgpt
            }
            _ => return Err(CodexDiscoveryError::AccountPolicy),
        };
        self.account = Some(account);
        self.phase = match (model_id, model_complete) {
            (Some(model_id), false) => Phase::AwaitModels(model_id),
            (None, false) => Phase::AccountReady,
            (None, true) => Phase::Complete,
            (Some(_), true) => return Err(CodexDiscoveryError::Protocol),
        };
        Ok(account)
    }

    pub(crate) fn model_request(&mut self) -> Result<DiscoveryRequest, CodexDiscoveryError> {
        if self.pages >= crate::acp::limits::LIMITS.codex.model_catalog_pages as usize {
            return Err(CodexDiscoveryError::PageLimit);
        }
        let account_pending = match self.phase {
            Phase::AccountReady => None,
            Phase::AwaitAccount {
                account_id,
                model_id: None,
                model_complete: false,
            } if self.pages == 0 => Some(account_id),
            _ => return Err(CodexDiscoveryError::Protocol),
        };
        let id = self.next_id()?;
        let cursor = self.next_cursor.as_ref().map(RequiredText::as_str);
        self.phase = match account_pending {
            Some(account_id) => Phase::AwaitAccount {
                account_id,
                model_id: Some(id),
                model_complete: false,
            },
            None => Phase::AwaitModels(id),
        };
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
        let (expected, pending_account) = match self.phase {
            Phase::AwaitModels(expected) => (expected, None),
            Phase::AwaitAccount {
                account_id,
                model_id: Some(expected),
                model_complete: false,
            } => (expected, Some(account_id)),
            _ => return Err(CodexDiscoveryError::Protocol),
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
            if self.pages + 1 == crate::acp::limits::LIMITS.codex.model_catalog_pages as usize {
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
            self.phase = match pending_account {
                Some(account_id) => Phase::AwaitAccount {
                    account_id,
                    model_id: None,
                    model_complete: false,
                },
                None => Phase::AccountReady,
            };
            return Ok(ProtocolProgress::More);
        }
        if self.models.is_empty() {
            return Err(CodexDiscoveryError::EmptyCatalog);
        }
        self.next_cursor = None;
        if let Some(account_id) = pending_account {
            self.phase = Phase::AwaitAccount {
                account_id,
                model_id: None,
                model_complete: true,
            };
            return Ok(ProtocolProgress::AwaitingAccount);
        }
        self.phase = Phase::Complete;
        self.take_completed_content()
            .map(ProtocolProgress::Complete)
    }

    pub(crate) fn take_completed_content(
        &mut self,
    ) -> Result<CodexDiscoveryContent, CodexDiscoveryError> {
        if self.phase != Phase::Complete {
            return Err(CodexDiscoveryError::Protocol);
        }
        Ok(CodexDiscoveryContent {
            account: self.account.ok_or(CodexDiscoveryError::Protocol)?,
            models: std::mem::take(&mut self.models),
            default_model_id: self.default_model_id.take(),
        })
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
