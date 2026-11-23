use std::collections::HashSet;

use super::{
    AdapterBatchJournal, AdapterEvent, AdapterPump, DurableAdapterEvent, HarnessAdapter,
    ItemLifecycle, NormalizedFact, NormalizedPayload, ResourceMapper, TurnState,
};
use crate::acp::resource_mapper::ContentState;
use sage_acp_protocol::intent::{SafeResourceLocation, SafeResourceRef};

/// Bytes one batch may hash. Files past it are recorded unverified, which a
/// later file rewind refuses.
const BATCH_HASH_BYTES: u64 = 32 * 1024 * 1024;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// The file a finished agent change in the open Turn wrote.
    pub(in crate::acp::runtime::adapter_pump) fn written_file(
        &mut self,
        event: &AdapterEvent,
        terminal: Option<TurnState>,
    ) -> Option<SafeResourceRef> {
        let AdapterEvent::Item {
            id,
            lifecycle: ItemLifecycle::Completed,
            payload:
                NormalizedPayload::FileChangeDiff {
                    resource: SafeResourceLocation::Resource { resource },
                    ..
                },
            ..
        } = event
        else {
            return None;
        };
        (terminal.is_none() && self.exact_turn(id)).then(|| resource.clone())
    }

    /// One write record per file the batch wrote, at most `room`, only for a
    /// session whose file rewinds check them. The files are hashed on the
    /// blocking pool, never on the pump's thread.
    pub(in crate::acp::runtime::adapter_pump) async fn agent_writes(
        &mut self,
        mut files: Vec<SafeResourceRef>,
        room: usize,
        now_millis: i64,
    ) -> Vec<NormalizedFact> {
        if files.is_empty()
            || !self
                .adapter
                .capabilities()
                .is_some_and(|capabilities| capabilities.rewind.effects.file_preview)
        {
            return Vec::new();
        }
        // The last write of each file in the batch is the one that counts.
        let mut seen = HashSet::new();
        files.reverse();
        files.retain(|file| {
            seen.insert((
                file.root_id.as_str().to_owned(),
                file.relative_path.as_str().to_owned(),
            ))
        });
        files.reverse();
        files.truncate(room);
        let probes: Vec<_> = files
            .iter()
            .map(|file| self.resources.content_probe(file))
            .collect();
        let digests = tokio::task::spawn_blocking(move || {
            let mut budget = BATCH_HASH_BYTES;
            probes
                .into_iter()
                .map(|probe| match probe?.state(&mut budget) {
                    ContentState::Present(digest) => Some(digest),
                    ContentState::Missing | ContentState::Unreadable => None,
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let scope = self.scope();
        files
            .into_iter()
            .enumerate()
            .map(|(index, resource)| {
                let bytes = resource.root_id.as_str().len() + resource.relative_path.as_str().len();
                NormalizedFact::new(
                    DurableAdapterEvent::AgentWrite {
                        scope: scope.clone(),
                        resource,
                        digest: digests.get(index).copied().flatten(),
                        now_millis,
                    },
                    bytes + 32,
                )
            })
            .collect()
    }
}
