use crate::acp::{contract::RuntimeEpoch, limits::LIMITS};

use super::{ActivationIntent, ActivationMode, ActivationPumpError};

pub(super) fn validate_intent(intent: &ActivationIntent) -> Result<(), ActivationPumpError> {
    if intent.binding_digest == [0; 32]
        || intent.runtime_epoch == RuntimeEpoch::ZERO
        || !valid_digest(&intent.artifact_digest)
    {
        return Err(ActivationPumpError::Intent);
    }
    let root = match &intent.mode {
        ActivationMode::New { native_root, .. } | ActivationMode::Resume { native_root, .. } => {
            Some(native_root)
        }
        ActivationMode::Load { .. } => None,
    };
    if root.is_some_and(|value| {
        value.is_empty()
            || value.len() > LIMITS.wire.absolute_path_bytes_native_only as usize
            || value.as_bytes().contains(&0)
    }) {
        return Err(ActivationPumpError::Intent);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn digest(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    value
}
