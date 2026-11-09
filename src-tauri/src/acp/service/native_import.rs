//! Lists a Project's native sessions started outside Sage and reads the
//! history of one to import, on the Project's lister runtime (see
//! `store::native_import::import_lister`). Nothing durable changes here.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::acp::{
    bindings::codex::{ChildTurn, MAX_SNAPSHOT_BYTES, NativeSessionsPage},
    contract::SageProjectId,
    goal::{NativeRead, NativeRpcReply},
    runtime::RuntimeHost,
    scheduler::RuntimeOwner,
    store::native_import::import_lister,
};

use super::{AcpService, native_read::NativeReadError};

/// An imported session's history as it is kept and shown: its newest Turns
/// within the child transcript bounds, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportedHistory {
    pub(crate) turns: Vec<ChildTurn>,
    /// Older Turns were left out.
    pub(crate) retention_limited: bool,
}

impl ImportedHistory {
    /// Drops the oldest Turns until the encoded history fits the bound.
    fn encode_within_bounds(mut self) -> Option<String> {
        let sizes: Vec<usize> = self
            .turns
            .iter()
            .map(|turn| serde_json::to_vec(turn).map_or(usize::MAX, |bytes| bytes.len() + 1))
            .collect();
        let mut total = sizes
            .iter()
            .fold(64usize, |sum, size| sum.saturating_add(*size));
        let dropped = sizes
            .iter()
            .take_while(|size| {
                let over = total > MAX_SNAPSHOT_BYTES;
                total = total.saturating_sub(**size);
                over
            })
            .count();
        if dropped > 0 {
            self.turns.drain(..dropped);
            self.retention_limited = true;
        }
        serde_json::to_string(&self)
            .ok()
            .filter(|json| json.len() <= MAX_SNAPSHOT_BYTES)
    }
}

impl AcpService {
    fn import_runtime(
        &self,
        project: &SageProjectId,
    ) -> Result<(RuntimeOwner, Arc<RuntimeHost>), NativeReadError> {
        let owner = import_lister(&self.database.connection(), project)
            .map_err(|_| NativeReadError::RecoveryRequired)?
            .ok_or(NativeReadError::Unavailable)?;
        let host = self
            .host_for(&owner)
            .map_err(|_| NativeReadError::Unavailable)?;
        Ok((owner, host))
    }

    /// One page of the Project's importable sessions.
    pub(crate) fn list_importable_sessions(
        &self,
        project: &SageProjectId,
        cursor: Option<String>,
    ) -> Result<NativeSessionsPage, NativeReadError> {
        let (owner, host) = self.import_runtime(project)?;
        match self.native_page(&owner, &host, NativeRead::ImportableSessions { cursor })? {
            NativeRpcReply::SessionsPage(page) => Ok(page),
            _ => Err(NativeReadError::RecoveryRequired),
        }
    }

    /// Reads a listed session's history for import, and returns it encoded
    /// with the lister whose binding will resume the session.
    pub(crate) fn read_import_history(
        &self,
        project: &SageProjectId,
        thread_id: &str,
    ) -> Result<(RuntimeOwner, String), NativeReadError> {
        let (owner, host) = self.import_runtime(project)?;
        let paged = self.read_turn_pages(
            &owner,
            &host,
            |cursor| NativeRead::ImportTurns {
                thread_id: thread_id.to_owned(),
                cursor,
            },
            true,
        )?;
        let history = ImportedHistory {
            turns: paged.turns,
            retention_limited: paged.truncated,
        };
        let json = history
            .encode_within_bounds()
            .ok_or(NativeReadError::TooLarge)?;
        Ok((owner, json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{bindings::codex::ChildItem, lifecycle::ItemLifecycle};

    fn turn(id: &str, text_bytes: usize) -> ChildTurn {
        ChildTurn {
            id: id.into(),
            status: "completed".into(),
            items: vec![ChildItem {
                id: "item".into(),
                role: None,
                observed_sequence: None,
                lifecycle: ItemLifecycle::Completed,
                tool_state: None,
                payload: crate::acp::intent::NormalizedPayload::Text {
                    text: crate::acp::contract::SafeText::new("text", "x".repeat(text_bytes))
                        .unwrap(),
                },
            }],
        }
    }

    #[test]
    fn an_oversized_history_keeps_its_newest_turns() {
        let small = ImportedHistory {
            turns: vec![turn("a", 10), turn("b", 10)],
            retention_limited: false,
        };
        let json = small.clone().encode_within_bounds().unwrap();
        assert_eq!(
            serde_json::from_str::<ImportedHistory>(&json).unwrap(),
            small
        );

        let chunk = 8_000;
        let count = MAX_SNAPSHOT_BYTES / chunk + 4;
        let large = ImportedHistory {
            turns: (0..count)
                .map(|index| turn(&format!("t{index}"), chunk))
                .collect(),
            retention_limited: false,
        };
        let json = large.encode_within_bounds().unwrap();
        assert!(json.len() <= MAX_SNAPSHOT_BYTES);
        let kept: ImportedHistory = serde_json::from_str(&json).unwrap();
        assert!(kept.retention_limited);
        assert!(kept.turns.len() < count);
        assert_eq!(kept.turns.last().unwrap().id, format!("t{}", count - 1));
    }
}
