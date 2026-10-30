// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
pub(crate) struct BudgetInput<'a> {
    pub configured_byte_cap: u64,
    pub user_text: &'a str,
    pub images: u64,
    pub other_attachments: u64,
    pub model_window_bytes: Option<u64>,
    pub reported_max_bytes: Option<u64>,
    pub auto_compact_bytes: Option<u64>,
    pub native_occupancy_bytes: u64,
    pub same_native_identity: bool,
}

impl<'a> BudgetInput<'a> {
    pub(crate) fn fresh(user_text: &'a str) -> Self {
        Self {
            configured_byte_cap: 16_000,
            user_text,
            images: 0,
            other_attachments: 0,
            model_window_bytes: None,
            reported_max_bytes: None,
            auto_compact_bytes: None,
            native_occupancy_bytes: 0,
            same_native_identity: false,
        }
    }
}

pub(crate) fn handoff_byte_budget(input: &BudgetInput<'_>) -> u64 {
    let window = input
        .model_window_bytes
        .or(input.reported_max_bytes)
        .unwrap_or(128_000)
        .min(input.reported_max_bytes.unwrap_or(u64::MAX))
        .min(input.auto_compact_bytes.unwrap_or(u64::MAX));
    let current = serde_json::to_vec(input.user_text)
        .map_or(u64::MAX, |text| text.len() as u64)
        .saturating_add(input.images.saturating_mul(8_192))
        .saturating_add(input.other_attachments.saturating_mul(4_096));
    let native = if input.same_native_identity {
        input.native_occupancy_bytes
    } else {
        0
    };
    window
        .saturating_sub(native)
        .saturating_sub(current)
        .saturating_sub(16_000.max(window.div_ceil(4)))
        .min(input.configured_byte_cap.clamp(1_024, 64_000))
}
