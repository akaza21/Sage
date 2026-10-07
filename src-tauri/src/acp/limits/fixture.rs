use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureLimits {
    pub steps_per_case: u64,
    pub expanded_bytes_per_case: u64,
    pub instruction_bytes_per_case: u64,
    pub raw_evidence_files_per_campaign: u64,
    pub raw_evidence_bytes_per_campaign: u64,
    pub campaigns_per_identity: u64,
    pub raw_evidence_bytes_per_identity: u64,
    pub retained_identities: u64,
    pub raw_evidence_files_global: u64,
    pub raw_evidence_bytes_global: u64,
    pub curated_evidence_bytes: u64,
    pub compacted_curated_files: u64,
    pub compacted_curated_bytes: u64,
    pub compaction_age_days_after_done: u64,
}

pub const FIXTURE_LIMITS: FixtureLimits = FixtureLimits {
    steps_per_case: 128,
    expanded_bytes_per_case: 9_437_184,
    instruction_bytes_per_case: 65_536,
    raw_evidence_files_per_campaign: 64,
    raw_evidence_bytes_per_campaign: 67_108_864,
    campaigns_per_identity: 8,
    raw_evidence_bytes_per_identity: 536_870_912,
    retained_identities: 2,
    raw_evidence_files_global: 1_024,
    raw_evidence_bytes_global: 1_073_741_824,
    curated_evidence_bytes: 16_777_216,
    compacted_curated_files: 16,
    compacted_curated_bytes: 2_097_152,
    compaction_age_days_after_done: 90,
};
