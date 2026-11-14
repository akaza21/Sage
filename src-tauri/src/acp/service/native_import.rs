use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    acp::{
        bindings::codex::NativeSessionsPage,
        contract::{RuntimeEpoch, SageAgentSessionId, SageProjectId},
        goal::{NativeRead, NativeRpcReply},
        runtime::RuntimeHost,
        scheduler::RuntimeOwner,
        store::native_import::{held_threads, import_lister, imported_threads},
    },
    acp_protocol::codex_protocol::{ChildTurn, MAX_SNAPSHOT_BYTES},
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

/// The runtime that listed a page of sessions: only it reads one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportLister {
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListedSessions {
    #[serde(flatten)]
    pub(crate) page: NativeSessionsPage,
    pub(crate) lister: ImportLister,
}

impl AcpService {
    fn import_runtime(
        &self,
        project: &SageProjectId,
        pinned: Option<&ImportLister>,
    ) -> Result<(RuntimeOwner, Arc<RuntimeHost>), NativeReadError> {
        let pinned = pinned.map(|lister| (&lister.session_id, lister.runtime_epoch));
        let owner = import_lister(&self.database.connection(), project, pinned)
            .map_err(|_| NativeReadError::RecoveryRequired)?
            .ok_or(NativeReadError::Unavailable)?;
        let host = self
            .host_for(&owner)
            .map_err(|_| NativeReadError::Unavailable)?;
        Ok((owner, host))
    }

    pub(crate) fn list_importable_sessions(
        &self,
        project: &SageProjectId,
        cursor: Option<String>,
        lister: Option<&ImportLister>,
    ) -> Result<ListedSessions, NativeReadError> {
        let (owner, host) = self.import_runtime(project, lister)?;
        let excluded = imported_threads(&self.database.connection(), project)
            .map_err(|_| NativeReadError::RecoveryRequired)?;
        let read = NativeRead::ImportableSessions { cursor, excluded };
        match self.native_page(&owner, &host, read)? {
            NativeRpcReply::SessionsPage(mut page) => {
                // The harness leaves out only the newest imports; every
                // thread a session holds would be refused, so none shows.
                let listed: Vec<_> = page
                    .sessions
                    .iter()
                    .map(|session| session.thread_id.as_str())
                    .collect();
                let held = held_threads(&self.database.connection(), project, &listed)
                    .map_err(|_| NativeReadError::RecoveryRequired)?;
                page.sessions
                    .retain(|session| !held.contains(&session.thread_id));
                Ok(ListedSessions {
                    page,
                    lister: ImportLister {
                        session_id: owner.session_id,
                        runtime_epoch: owner.runtime_epoch,
                    },
                })
            }
            _ => Err(NativeReadError::RecoveryRequired),
        }
    }

    pub(crate) fn read_import_history(
        &self,
        project: &SageProjectId,
        lister: &ImportLister,
        thread_id: &str,
    ) -> Result<(RuntimeOwner, String), NativeReadError> {
        let (owner, host) = self.import_runtime(project, Some(lister))?;
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
    use crate::{acp::lifecycle::ItemLifecycle, acp_protocol::codex_protocol::ChildItem};

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
