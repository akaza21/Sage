use sha2::{Digest, Sha256};

pub(crate) fn legacy_binding_digest(binding_id: &str, harness_version: &Option<String>) -> String {
    let mut hash = Sha256::new();
    hash.update(b"sage-legacy-binding-v1\0");
    hash.update([1]);
    hash.update((binding_id.len() as u64).to_be_bytes());
    hash.update(binding_id.as_bytes());
    match harness_version {
        Some(version) => {
            hash.update([1]);
            hash.update((version.len() as u64).to_be_bytes());
            hash.update(version.as_bytes());
        }
        None => {
            hash.update([0]);
            hash.update(0_u64.to_be_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn legacy_turn_id(session_id: &str, first_sequence: i64, last_sequence: i64) -> String {
    let mut hash = Sha256::new();
    hash.update(b"sage-legacy-turn-v1\0");
    hash.update((session_id.len() as u64).to_be_bytes());
    hash.update(session_id.as_bytes());
    hash.update(first_sequence.to_be_bytes());
    hash.update(last_sequence.to_be_bytes());
    format!("legacy-turn-{:x}", hash.finalize())
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_binding_digest_matches_the_frozen_known_answers() {
        assert_ne!(
            legacy_binding_digest("fixture", &None),
            legacy_binding_digest("fixture", &Some(String::new()))
        );
        assert_eq!(
            legacy_binding_digest("simulator", &None),
            "8db860cda854b96c7ff210345c1858668b305f4a661d03cb359dcc18be701e24"
        );
        assert_eq!(
            legacy_binding_digest("simulator", &Some("1.2.3".into())),
            "371fdf57fe765356a6326686def4e70bbeb9ccce6fe2ea587c83b1aa3fabf9df"
        );
    }

    #[test]
    fn legacy_turn_ids_are_stable_and_range_scoped() {
        assert_eq!(
            legacy_turn_id("session", 1, 2),
            legacy_turn_id("session", 1, 2)
        );
        assert_ne!(
            legacy_turn_id("session", 1, 2),
            legacy_turn_id("session", 1, 3)
        );
    }
}
