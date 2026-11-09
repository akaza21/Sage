use rusqlite::{OptionalExtension, params};

use crate::acp::{
    bindings::codex::ChildTranscriptSnapshot,
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    store::child_transcript::{
        ChildTranscriptScope, ChildTranscriptStoreError, load_earlier_snapshot, load_snapshot,
        persist_snapshot, verify_current_binding,
    },
};

use super::AcpService;

pub(crate) enum ChildTranscriptRead {
    Current(ChildTranscriptSnapshot),
    FromEarlierSession(ChildTranscriptSnapshot),
    TooLarge,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ChildTranscriptReadError {
    #[error("the native child transcript read was rejected")]
    Rejected,
    #[error("the child transcript is unavailable for this owner")]
    Unavailable,
    #[error("the child transcript binding changed during the read")]
    Stale,
    #[error("the native read requires recovery")]
    RecoveryRequired,
    #[error(transparent)]
    Store(#[from] ChildTranscriptStoreError),
}

impl AcpService {
    pub(crate) fn read_child_transcript(
        &self,
        scope: ChildTranscriptScope,
    ) -> Result<ChildTranscriptRead, ChildTranscriptReadError> {
        let (native, running) = {
            let connection = self.database.connection();
            let native = observed_protocol(&connection, &scope)?;
            let running = match verify_current_binding(&connection, &scope) {
                Ok(()) => true,
                Err(ChildTranscriptStoreError::Stale) => false,
                Err(error) => return Err(error.into()),
            };
            (native, running)
        };
        if !running {
            return load_earlier_snapshot(&self.database.connection(), &scope)?
                .map(ChildTranscriptRead::FromEarlierSession)
                .ok_or(ChildTranscriptReadError::Unavailable);
        }
        // A harness without a native child read streams every child item
        // to the store as it happens, so the stored transcript is current.
        if !native {
            return load_snapshot(&self.database.connection(), &scope)?
                .map(ChildTranscriptRead::Current)
                .ok_or(ChildTranscriptReadError::Unavailable);
        }
        let host = self
            .host_for(&scope.owner)
            .map_err(|_| ChildTranscriptReadError::RecoveryRequired)?;
        let result = host.native_rpc(NativeRpcIntent {
            owner: scope.owner.clone(),
            request: NativeRpcRequest::ChildThreadRead {
                child_thread_id: scope.child_thread_id.clone(),
                parent_thread_id: scope.parent_thread_id.clone(),
            },
        });
        if matches!(
            result,
            Err(crate::acp::runtime::HostError::Unavailable
                | crate::acp::runtime::HostError::Closed
                | crate::acp::runtime::HostError::Deadline
                | crate::acp::runtime::HostError::DeliveryUnknown
                | crate::acp::runtime::HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(scope.owner.clone(), host);
        }
        match result {
            Ok(NativeRpcReply::ChildTranscript(snapshot)) => {
                match persist_snapshot(
                    &mut self.database.connection(),
                    &scope,
                    &snapshot,
                    crate::persistence::now_millis(),
                ) {
                    Ok(()) => {}
                    Err(ChildTranscriptStoreError::Capacity) => {
                        return Ok(ChildTranscriptRead::TooLarge);
                    }
                    Err(ChildTranscriptStoreError::Stale) => {
                        return Err(ChildTranscriptReadError::Stale);
                    }
                    Err(error) => return Err(ChildTranscriptReadError::Store(error)),
                }
                let merged = load_snapshot(&self.database.connection(), &scope)?
                    .ok_or(ChildTranscriptReadError::RecoveryRequired)?;
                Ok(ChildTranscriptRead::Current(merged))
            }
            Ok(NativeRpcReply::TooLarge) => Ok(ChildTranscriptRead::TooLarge),
            Ok(NativeRpcReply::Rejected) => Err(ChildTranscriptReadError::Rejected),
            Ok(
                NativeRpcReply::GoalApplied
                | NativeRpcReply::Forked(_)
                | NativeRpcReply::Released
                | NativeRpcReply::UsageRead
                | NativeRpcReply::Controlled(_)
                | NativeRpcReply::ControlRefused(_),
            ) => Err(ChildTranscriptReadError::RecoveryRequired),
            Err(crate::acp::runtime::HostError::Intent | crate::acp::runtime::HostError::State) => {
                Err(ChildTranscriptReadError::Unavailable)
            }
            Err(_) => Err(ChildTranscriptReadError::RecoveryRequired),
        }
    }
}

fn observed_protocol(
    connection: &rusqlite::Connection,
    scope: &ChildTranscriptScope,
) -> Result<bool, ChildTranscriptReadError> {
    let encoded: Option<String> = connection.query_row(
        "SELECT capabilities_json FROM agent_sessions WHERE id=?1 AND project_id=?2 AND chat_id=?3",
        params![scope.owner.session_id.as_str(), scope.owner.project_id.as_str(), scope.owner.chat_id.as_str()],
        |row| row.get(0),
    ).optional().map_err(ChildTranscriptStoreError::Storage)?;
    let Some(encoded) = encoded else {
        return Ok(false);
    };
    let value: serde_json::Value =
        serde_json::from_str(&encoded).map_err(|_| ChildTranscriptReadError::RecoveryRequired)?;
    // The debug desktop fixture replays the same pinned `thread/read` wire
    // under its own identity, so native captures can read its children.
    Ok(value
        .get("protocolVersion")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|identity| {
            crate::acp::bindings::codex::speaks_pinned_protocol(identity)
                || crate::acp::bindings::codex::is_desktop_fixture_protocol(identity)
        }))
}
