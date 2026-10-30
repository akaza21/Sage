pub(crate) mod budget;
pub(crate) mod delivery;
pub(crate) mod history;
pub(crate) mod render;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Omissions {
    pub unsupported_kind: u64,
    pub oversize: u64,
    pub scan_limit: u64,
    pub budget: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HandoffState {
    Prepared,
    Pending,
    Inline,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffSummary {
    pub id: String,
    pub revision: crate::acp::contract::ProjectionRevision,
    pub target_session: String,
    pub turn_id: Option<String>,
    pub state: HandoffState,
    pub carried_items: usize,
    pub carried_bytes: usize,
    pub omissions: Omissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HandoffPlan {
    pub source_watermark: i64,
    pub source_first: Option<i64>,
    pub total_items: u64,
    pub selected_ids: Vec<String>,
    pub selected_bytes: usize,
    pub estimated_cost_bytes: usize,
    pub omissions: Omissions,
    pub rendered_text: String,
    pub budget_version: u8,
}

#[cfg(test)]
mod tests;
