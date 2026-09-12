use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{
    ActiveTurn, MAX_PENDING_REQUESTS, OmpTranslator, PendingKind, TranslationError, validated_id,
};
use crate::acp::bindings::omp::protocol::{Operation, Outbound};

impl OmpTranslator {
    pub(crate) fn begin(
        &mut self,
        operation: Operation,
    ) -> Result<Vec<Outbound>, TranslationError> {
        match operation {
            Operation::Initialize => self.begin_initialize(),
            Operation::NewSession {
                native_cwd,
                root_count,
            } => self.begin_new_session(native_cwd, root_count),
            Operation::LoadSession {
                external_session_id,
                native_cwd,
                root_count,
            } => self.begin_load_session(external_session_id, native_cwd, root_count),
            Operation::Prompt { turn_key, message } => self.begin_prompt(turn_key, message),
            Operation::Cancel => self.begin_cancel(),
            Operation::ResumeSession => {
                Err(TranslationError::UnsupportedOperation("session/resume"))
            }
            Operation::CloseSession => Err(TranslationError::UnsupportedOperation("session/close")),
        }
    }

    fn begin_initialize(&mut self) -> Result<Vec<Outbound>, TranslationError> {
        if !self.capabilities.initialize()
            || self.initialized
            || self
                .pending
                .values()
                .any(|pending| matches!(pending, PendingKind::Initialize))
        {
            return Err(TranslationError::InvalidLifecycle);
        }
        self.one_request(
            PendingKind::Initialize,
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {"readTextFile": true, "writeTextFile": false},
                    "terminal": false
                },
                "clientInfo": {"name": "Sage", "title": "Sage", "version": "0"}
            }),
        )
    }

    fn begin_new_session(
        &mut self,
        native_cwd: String,
        root_count: usize,
    ) -> Result<Vec<Outbound>, TranslationError> {
        self.require_initialized()?;
        self.require_idle_session_transition()?;
        if !self.capabilities.new_session() {
            return Err(TranslationError::UnsupportedOperation("session/new"));
        }
        self.validate_roots(root_count)?;
        validate_native_absolute_path(&native_cwd)?;
        self.one_request(
            PendingKind::NewSession,
            "session/new",
            json!({"cwd": native_cwd, "mcpServers": []}),
        )
    }

    fn begin_load_session(
        &mut self,
        external_session_id: String,
        native_cwd: String,
        root_count: usize,
    ) -> Result<Vec<Outbound>, TranslationError> {
        self.require_initialized()?;
        self.require_idle_session_transition()?;
        self.validate_roots(root_count)?;
        if !self.negotiated_load || !self.capabilities.load_session_ceiling() {
            return Err(TranslationError::UnsupportedOperation("session/load"));
        }
        validate_native_absolute_path(&native_cwd)?;
        let session_id = validated_id("externalSessionId", external_session_id)?;
        self.one_request(
            PendingKind::LoadSession {
                expected_session: session_id.as_str().to_owned(),
            },
            "session/load",
            json!({"sessionId":session_id.as_str(),"cwd":native_cwd,"mcpServers":[]}),
        )
    }

    fn begin_prompt(
        &mut self,
        turn_key: String,
        message: String,
    ) -> Result<Vec<Outbound>, TranslationError> {
        self.require_initialized()?;
        if !self.capabilities.prompt() || self.capabilities.write_tools() {
            return Err(TranslationError::ReadOnlyUnproven);
        }
        let session_id = self
            .session_id
            .as_ref()
            .ok_or(TranslationError::InvalidLifecycle)?
            .as_str()
            .to_owned();
        let turn_key = validated_id("turnKey", turn_key)?;
        if message.is_empty() || message.len() > 100_000 || message.contains('\0') {
            return Err(TranslationError::Limit("prompt bytes"));
        }
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_none())
        {
            return Err(TranslationError::PromptAlreadyActive);
        }
        let outbound = self.request(
            PendingKind::Prompt,
            "session/prompt",
            json!({"sessionId":session_id,"prompt":[{"type":"text","text":message}]}),
        )?;
        self.active = Some(ActiveTurn {
            turn_key: turn_key.as_str().to_owned(),
            request_id: outbound
                .request_id
                .clone()
                .ok_or(TranslationError::Correlation)?,
            cancel_requested: false,
            cancel_dispatch_required: false,
            running_emitted: false,
            terminal: None,
            tools: BTreeMap::new(),
            tool_categories: BTreeMap::new(),
            command_snapshots: BTreeMap::new(),
            command_snapshot_bytes: 0,
        });
        Ok(vec![outbound])
    }

    fn begin_cancel(&mut self) -> Result<Vec<Outbound>, TranslationError> {
        if !self.capabilities.cancel() {
            return Err(TranslationError::UnsupportedOperation("session/cancel"));
        }
        let session_id = self
            .session_id
            .as_ref()
            .ok_or(TranslationError::InvalidLifecycle)?;
        let turn = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        if turn.terminal.is_some() {
            return Err(TranslationError::StaleIntent);
        }
        if turn.cancel_requested {
            return Ok(Vec::new());
        }
        turn.cancel_requested = true;
        Ok(vec![notification(
            "session/cancel",
            json!({"sessionId":session_id.as_str()}),
        )?])
    }

    fn require_initialized(&self) -> Result<(), TranslationError> {
        self.initialized
            .then_some(())
            .ok_or(TranslationError::InvalidLifecycle)
    }

    fn require_idle_session_transition(&self) -> Result<(), TranslationError> {
        if !self.pending.is_empty()
            || self
                .active
                .as_ref()
                .is_some_and(|turn| turn.terminal.is_none())
        {
            Err(TranslationError::SessionTransitionBusy)
        } else {
            Ok(())
        }
    }

    fn validate_roots(&self, count: usize) -> Result<(), TranslationError> {
        if count == 1 {
            Ok(())
        } else if count > 1 && !self.capabilities.multi_root() {
            Err(TranslationError::UnsupportedOperation("multi-root"))
        } else {
            Err(TranslationError::Limit("project root count"))
        }
    }

    fn one_request(
        &mut self,
        kind: PendingKind,
        method: &'static str,
        params: Value,
    ) -> Result<Vec<Outbound>, TranslationError> {
        Ok(vec![self.request(kind, method, params)?])
    }

    pub(super) fn request(
        &mut self,
        kind: PendingKind,
        method: &'static str,
        params: Value,
    ) -> Result<Outbound, TranslationError> {
        if self.pending.len() >= MAX_PENDING_REQUESTS {
            return Err(TranslationError::RequestCapacity);
        }
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(TranslationError::RequestCapacity)?;
        let id = format!("omp-{}-{}", self.epoch, self.next_request);
        let line = encode_line(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        self.pending.insert(id.clone(), kind);
        Ok(Outbound {
            request_id: Some(id),
            line,
        })
    }

    #[cfg(test)]
    pub(crate) fn issue_probe(&mut self) -> Result<(), TranslationError> {
        self.request(PendingKind::NewSession, "session/new", json!({}))?;
        Ok(())
    }
}

fn notification(method: &'static str, params: Value) -> Result<Outbound, TranslationError> {
    Ok(Outbound {
        request_id: None,
        line: encode_line(json!({"jsonrpc":"2.0","method":method,"params":params}))?,
    })
}

pub(super) fn encode_line(value: Value) -> Result<String, TranslationError> {
    let mut line = serde_json::to_string(&value).map_err(|_| TranslationError::MalformedFrame)?;
    line.push('\n');
    Ok(line)
}

fn validate_native_absolute_path(path: &str) -> Result<(), TranslationError> {
    let bytes = path.as_bytes();
    let absolute = path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'));
    let aliases = path
        .split(['/', '\\'])
        .any(|segment| matches!(segment, "." | ".."));
    if absolute && !aliases && !path.contains('\0') && path.len() <= 32_768 {
        Ok(())
    } else {
        Err(TranslationError::NativePath)
    }
}
