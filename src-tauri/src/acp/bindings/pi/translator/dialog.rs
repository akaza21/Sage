use sage_acp_protocol::intent::{PermissionOptionKind, user_input::UserInputRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingUiRequest {
    pub id: String,
    pub method: String,
    pub form: Option<Box<UserInputRequest>>,
    pub received_at: std::time::Instant,
    pub timeout_ms: Option<u64>,
    pub digest: [u8; 32],
}

impl PendingUiRequest {
    /// Pi has no standing approval, so "always allow" is never offered.
    pub(crate) fn offered_option_kinds(&self) -> Vec<PermissionOptionKind> {
        use PermissionOptionKind::{AllowOnce, RejectOnce};
        let allows = matches!(self.method.as_str(), super::BRIDGE_WRITE_METHOD | "confirm");
        if allows {
            vec![AllowOnce, RejectOnce]
        } else {
            vec![RejectOnce]
        }
    }
}

impl PendingUiRequest {
    pub(crate) fn deadline(&self) -> Option<std::time::Instant> {
        self.received_at
            .checked_add(std::time::Duration::from_millis(self.timeout_ms?))
    }
    pub(crate) fn remaining_ms(&self) -> Option<u64> {
        let deadline = self.deadline()?;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        Some(u64::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX))
    }
}
