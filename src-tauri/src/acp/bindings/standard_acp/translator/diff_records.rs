use crate::acp::{
    contract::{RequiredText, SafeText},
    file_diff::{DIFF_SIDE_BYTES_MAX, DIFFS_PER_TURN, PairAdmission, admit_pair},
    intent::{
        ExternalId, FileChangeFacts, FileChangeOperation, NormalizedPayload, SafeResourceLocation,
        SafeResourceRef, UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState},
};

use super::decode::unsupported_event;
use super::tools::FILE_CHANGE_SUMMARY;
use super::{
    AdapterEvent, MAX_EVENTS_PER_FRAME, ResourceMapper, StandardAcpTranslator, TranslationError,
};

const DIFF_OVERFLOW_KIND: &str = "fileChangeOverflow";

#[derive(Debug, Clone, Copy)]
pub(super) struct FrameState {
    pub(super) settling: bool,
    pub(super) late: bool,
}

struct Settlement {
    resolved: Option<SafeResourceRef>,
    deleted_display: Option<SafeResourceRef>,
    missing_deletion: bool,
    pair: Option<(u64, u64, String)>,
    slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffFacts {
    pub(super) native_path: String,
    pub(super) additions: Option<u64>,
    pub(super) deletions: Option<u64>,
    pub(super) operation: FileChangeOperation,
    pub(super) unavailable: Option<UnavailableResourceReason>,
}

pub(super) struct DiffObservation {
    pub(super) facts: DiffFacts,
    pub(super) old_text: String,
    pub(super) new_text: String,
    pub(super) late: bool,
    pub(super) streamed: bool,
    pub(super) emitted: Option<SafeResourceRef>,
    pub(super) emitted_reason: Option<UnavailableResourceReason>,
}

#[derive(Debug, Clone)]
pub(super) struct DiffRecord {
    tool_id: String,
    bound_path: String,
    facts: DiffFacts,
    old_text: String,
    new_text: String,
    reason: UnavailableResourceReason,
    oversized: bool,
    retained: bool,
    settled: bool,
    streamed: Option<DiffFacts>,
    emitted: Option<SafeResourceRef>,
    emitted_reason: Option<UnavailableResourceReason>,
}

impl DiffRecord {
    pub(super) fn settled(&self) -> bool {
        self.settled
    }

    pub(super) fn bound_path(&self) -> &str {
        &self.bound_path
    }

    pub(super) fn reason(&self) -> UnavailableResourceReason {
        self.reason
    }

    pub(super) fn streamed(&self) -> Option<&DiffFacts> {
        self.streamed.as_ref()
    }

    pub(super) fn frozen(&self) -> Option<&SafeResourceRef> {
        self.emitted.as_ref()
    }

    pub(super) fn frozen_reason(&self) -> Option<UnavailableResourceReason> {
        self.emitted_reason
    }

    pub(super) fn poisoned(&self) -> bool {
        self.bound_path != self.facts.native_path
    }
}

impl StandardAcpTranslator {
    pub(super) fn diff_record(&self, external_key: &str) -> Option<&DiffRecord> {
        self.diff_records.get(external_key)
    }

    pub(super) fn record_diffs(
        &mut self,
        tool_id: &str,
        observed: Vec<(String, DiffObservation)>,
    ) -> bool {
        let mut changed = false;
        for (external_key, observation) in observed {
            if self
                .diff_records
                .get(&external_key)
                .is_some_and(DiffRecord::settled)
            {
                continue;
            }
            let oversized = observation.old_text.len() > DIFF_SIDE_BYTES_MAX
                || observation.new_text.len() > DIFF_SIDE_BYTES_MAX;
            if !self.diff_records.contains_key(&external_key) {
                let retained =
                    !observation.late && self.retained_diff_count() < DIFFS_PER_TURN as usize;
                let record = DiffRecord {
                    tool_id: tool_id.to_owned(),
                    bound_path: observation.facts.native_path.clone(),
                    facts: observation.facts.clone(),
                    old_text: String::new(),
                    new_text: String::new(),
                    reason: UnavailableResourceReason::Unsupported,
                    oversized,
                    retained,
                    settled: false,
                    streamed: None,
                    emitted: None,
                    emitted_reason: None,
                };
                self.diff_records.insert(external_key.clone(), record);
                changed = true;
            }
            let Some(record) = self.diff_records.get_mut(&external_key) else {
                continue;
            };
            if record.facts != observation.facts
                || record.old_text != observation.old_text
                || record.new_text != observation.new_text
            {
                changed = true;
            }
            if let Some(reason) = observation.facts.unavailable {
                record.reason = reason;
            }
            record.facts = observation.facts;
            record.oversized = oversized;
            if record.poisoned() {
                record.retained = false;
            }
            if record.retained && !oversized {
                record.old_text = observation.old_text;
                record.new_text = observation.new_text;
            } else {
                record.old_text = String::new();
                record.new_text = String::new();
            }
            if observation.streamed {
                record.streamed = Some(record.facts.clone());
            }
            if record.emitted.is_none() {
                record.emitted = observation.emitted;
            }
            if record.emitted_reason.is_none() {
                record.emitted_reason = observation.emitted_reason;
            }
        }
        changed
    }

    pub(super) fn settle_diffs(
        &mut self,
        tool_id: &str,
        state: ToolState,
        resources: Option<&dyn ResourceMapper>,
        events: &mut Vec<AdapterEvent>,
    ) -> Result<(), TranslationError> {
        if !state.is_terminal() {
            return Ok(());
        }
        let keys = self.unsettled_keys(Some(tool_id));
        let promotable = state == ToolState::Completed;
        self.settle_keys(keys, promotable, resources, MAX_EVENTS_PER_FRAME, events)?;
        Ok(())
    }

    pub(super) fn park_settled_diffs(
        &mut self,
        tool_id: &str,
        state: ToolState,
    ) -> Result<(), TranslationError> {
        let mut parked = Vec::new();
        self.settle_diffs(tool_id, state, None, &mut parked)?;
        self.carry.append(&mut parked);
        Ok(())
    }

    pub(super) fn drain_diffs(
        &mut self,
        promotable: bool,
        limit: usize,
        events: &mut Vec<AdapterEvent>,
    ) -> Result<(), TranslationError> {
        let keys = self.unsettled_keys(None);
        let required = self
            .carry
            .len()
            .saturating_add(self.settlement_slots(&keys, promotable));
        let budget = if required > limit.saturating_sub(events.len()) {
            limit.saturating_sub(1)
        } else {
            limit
        };
        let mut omitted = 0_usize;
        for event in std::mem::take(&mut self.carry) {
            if omitted == 0 && events.len() < budget {
                events.push(event);
            } else {
                omitted = omitted.saturating_add(1);
            }
        }
        omitted = omitted.saturating_add(self.settle_keys(keys, promotable, None, budget, events)?);
        if omitted == 0 {
            return Ok(());
        }
        events.push(self.overflow_event(omitted)?);
        Ok(())
    }

    fn overflow_event(&self, omitted: usize) -> Result<AdapterEvent, TranslationError> {
        unsupported_event(
            DIFF_OVERFLOW_KIND,
            &format!("{omitted} file change records could not be delivered in this turn"),
            self.turn_scope(),
        )
    }

    fn settlement_slots(&self, keys: &[String], promotable: bool) -> usize {
        keys.iter()
            .filter_map(|key| self.settlement(key, promotable, None))
            .map(|settlement| settlement.slots)
            .sum()
    }

    fn unsettled_keys(&self, tool_id: Option<&str>) -> Vec<String> {
        self.diff_records
            .iter()
            .filter(|(_, record)| !record.settled && tool_id.is_none_or(|id| record.tool_id == id))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn settle_keys(
        &mut self,
        keys: Vec<String>,
        promotable: bool,
        resources: Option<&dyn ResourceMapper>,
        limit: usize,
        events: &mut Vec<AdapterEvent>,
    ) -> Result<usize, TranslationError> {
        let mut omitted = 0_usize;
        for key in keys {
            let Some(settlement) = self.settlement(&key, promotable, resources) else {
                continue;
            };
            if omitted > 0 || events.len().saturating_add(settlement.slots) > limit {
                omitted = omitted.saturating_add(1);
                continue;
            }
            self.emit_settlement(&key, promotable, settlement, events)?;
        }
        Ok(omitted)
    }

    fn settlement(
        &self,
        key: &str,
        promotable: bool,
        resources: Option<&dyn ResourceMapper>,
    ) -> Option<Settlement> {
        let record = self.diff_records.get(key)?;
        let completion_mapping = resources.map(|mapper| mapper.map_absolute(&record.bound_path));
        let missing_deletion = promotable
            && !record.poisoned()
            && record.frozen().is_none()
            && !record.old_text.is_empty()
            && record.new_text.is_empty()
            && matches!(
                &completion_mapping,
                Some(Err(UnavailableResourceReason::Missing))
            );
        let deleted_display = missing_deletion
            .then(|| resources.and_then(|mapper| mapper.map_missing_display(&record.bound_path)))
            .flatten();
        let resolved = if missing_deletion {
            None
        } else {
            match record.frozen() {
                Some(resource) => Some(resource.clone()),
                None if record.poisoned() => None,
                None => completion_mapping.and_then(Result::ok),
            }
        };
        let admits = record.retained && !record.oversized && promotable && resolved.is_some();
        let pair = match admits.then(|| admit_pair(&record.old_text, &record.new_text)) {
            Some(PairAdmission::Admitted {
                old_bytes,
                new_bytes,
                digest,
            }) => Some((old_bytes as u64, new_bytes as u64, digest)),
            Some(PairAdmission::SummaryOnly) | None => None,
        };
        let slots = if pair.is_some() { 2 } else { 1 };
        Some(Settlement {
            resolved,
            deleted_display,
            missing_deletion,
            pair,
            slots,
        })
    }

    fn emit_settlement(
        &mut self,
        key: &str,
        promotable: bool,
        settlement: Settlement,
        events: &mut Vec<AdapterEvent>,
    ) -> Result<(), TranslationError> {
        let Some(record) = self.diff_records.get(key) else {
            return Ok(());
        };
        let external_key: ExternalId = RequiredText::new("externalItemId", key)?;
        let facts = FileChangeFacts {
            resource: match &settlement.resolved {
                Some(resource) => SafeResourceLocation::Resource {
                    resource: resource.clone(),
                },
                None => match &settlement.deleted_display {
                    Some(display) => SafeResourceLocation::unavailable_with_display(
                        UnavailableResourceReason::Missing,
                        display.clone(),
                    ),
                    None => SafeResourceLocation::unavailable(
                        record.frozen_reason().unwrap_or(record.reason),
                    ),
                },
            },
            summary: SafeText::new("fileSummary", FILE_CHANGE_SUMMARY)?,
            additions: record.facts.additions,
            deletions: record.facts.deletions,
            operation: if settlement.missing_deletion {
                FileChangeOperation::Deleted
            } else {
                record.facts.operation
            },
        };
        match settlement.pair {
            Some((old_bytes, new_bytes, digest)) => {
                let old_text = record.old_text.clone();
                let new_text = record.new_text.clone();
                events.push(AdapterEvent::Item {
                    external_key: external_key.clone(),
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::file_admitted(
                        facts,
                        0,
                        (old_bytes, new_bytes),
                        digest,
                    ),
                });
                events.push(AdapterEvent::FilePair {
                    external_key,
                    old_text,
                    new_text,
                });
            }
            None => {
                let truncated = promotable && settlement.resolved.is_some();
                events.push(AdapterEvent::Item {
                    external_key,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::file_summary_only(facts, truncated),
                });
            }
        }
        if let Some(record) = self.diff_records.get_mut(key) {
            record.settled = true;
            record.old_text = String::new();
            record.new_text = String::new();
        }
        Ok(())
    }

    fn retained_diff_count(&self) -> usize {
        self.diff_records.values().filter(|r| r.retained).count()
    }
}
