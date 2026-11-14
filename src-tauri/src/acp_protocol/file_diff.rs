//! Bounded immutable file-diff pair admission and snapshot identity.
//!
//! Native owns raw UTF-8 byte admission for `oldText`/`newText`. An admitted
//! pair is stored as two SQLite BLOBs keyed by [`DiffRef`]; only bounded
//! metadata stays in the encoded [`NormalizedPayload`](crate::acp::intent::NormalizedPayload).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod line_counts;
pub(crate) use line_counts::line_change_counts;

/// Maximum raw UTF-8 bytes per diff side. Checked before serialization so
/// JSON escaping can never turn an admitted pair into an oversized payload.
pub(crate) use sage_core::limits::DIFF_SIDE_BYTES_MAX;
/// Maximum retained diffs per Turn. Over-cap admits summary-only metadata.
pub(crate) use sage_core::limits::DIFFS_PER_TURN;
/// Single diff ordinal per persisted item (one resource per item).
pub(crate) const DIFF_ORDINAL: u64 = 0;

const DIGEST_HEX_LEN: usize = 64;
const MAX_ID_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 16_384;

/// Outcome of harness-neutral pair admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PairAdmission {
    /// Both sides fit; bytes are the raw UTF-8 lengths.
    Admitted {
        old_bytes: usize,
        new_bytes: usize,
        digest: String,
    },
    /// Either side exceeds the cap: persist summary-only with `truncated`.
    /// No partial side is retained.
    SummaryOnly,
}

pub(crate) fn admit_pair(old_text: &str, new_text: &str) -> PairAdmission {
    let old_bytes = old_text.len();
    let new_bytes = new_text.len();
    if old_bytes > DIFF_SIDE_BYTES_MAX || new_bytes > DIFF_SIDE_BYTES_MAX {
        return PairAdmission::SummaryOnly;
    }
    PairAdmission::Admitted {
        old_bytes,
        new_bytes,
        digest: pair_digest(old_text.as_bytes(), new_text.as_bytes()),
    }
}

/// Content digest binding one immutable snapshot revision to its bytes.
pub(crate) fn pair_digest(old_bytes: &[u8], new_bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"sage-file-diff-v1\x00");
    hash.update((old_bytes.len() as u64).to_be_bytes());
    hash.update(old_bytes);
    hash.update((new_bytes.len() as u64).to_be_bytes());
    hash.update(new_bytes);
    format!("{:x}", hash.finalize())
}

/// Immutable owner-bound snapshot key. This is the cache/tab key; it is
/// validated by the native snapshot lookup and never reused across revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffRef {
    pub project_id: String,
    pub chat_id: String,
    pub session_id: String,
    #[serde(with = "wire_counter")]
    pub runtime_epoch: u64,
    pub turn_id: String,
    pub item_id: String,
    pub diff_ordinal: u64,
    #[serde(with = "wire_counter")]
    pub snapshot_revision: u64,
    pub root_id: String,
    pub relative_path: String,
    pub content_digest: String,
}

/// The desktop bridge and durable tab JSON use the existing canonical decimal
/// counter contract, never lossy JavaScript numbers for SQLite-sized identities.
pub(crate) mod wire_counter {
    use crate::acp::contract::EntityRevision;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        EntityRevision::new(*value)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        EntityRevision::deserialize(deserializer).map(|value| value.get())
    }
}

impl DiffRef {
    pub(crate) fn validate(&self) -> Result<(), SnapshotReadError> {
        for field in [
            &self.project_id,
            &self.chat_id,
            &self.session_id,
            &self.turn_id,
            &self.item_id,
            &self.root_id,
        ] {
            if field.is_empty() || field.len() > MAX_ID_BYTES || field.contains('\0') {
                return Err(SnapshotReadError::Forbidden);
            }
        }
        if self.relative_path.len() > MAX_PATH_BYTES {
            return Err(SnapshotReadError::Forbidden);
        }
        let root = crate::acp::contract::SageRootId::parse(self.root_id.clone())
            .map_err(|_| SnapshotReadError::Forbidden)?;
        crate::acp::intent::SafeResourceRef::new(root, self.relative_path.clone())
            .map_err(|_| SnapshotReadError::Forbidden)?;
        if self.diff_ordinal != DIFF_ORDINAL
            || self.runtime_epoch == 0
            || self.runtime_epoch > i64::MAX as u64
            || self.snapshot_revision == 0
            || self.snapshot_revision > i64::MAX as u64
        {
            return Err(SnapshotReadError::Stale);
        }
        if self.content_digest.len() != DIGEST_HEX_LEN
            || !self
                .content_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(SnapshotReadError::Corrupt);
        }
        Ok(())
    }
}

/// One bounded snapshot read result. Re-checked against the side cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotData {
    pub old_text: String,
    pub new_text: String,
    pub old_bytes: usize,
    pub new_bytes: usize,
    pub snapshot_revision: u64,
    pub content_digest: String,
}

/// Typed snapshot read failure. Missing, mismatched, stale, or corrupt rows
/// never fall back to disk or Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotReadError {
    /// No admitted pair exists for this ref (summary-only or unknown).
    Unavailable,
    /// Owner tuple or revision does not match the durable row.
    Stale,
    /// Row bytes fail integrity checks.
    Corrupt,
    /// Refused: foreign root, malformed ref, or wrong project/chat owner.
    Forbidden,
}

impl SnapshotReadError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "No admitted file snapshot exists for this change",
            Self::Stale => "This file snapshot belongs to a replaced session or revision",
            Self::Corrupt => "The stored file snapshot failed its integrity check",
            Self::Forbidden => "This file snapshot is not available to the current owner",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_side_cap_admits_and_plus_one_overflows_without_partial_text() {
        let exact = "x".repeat(DIFF_SIDE_BYTES_MAX);
        let plus_one = "x".repeat(DIFF_SIDE_BYTES_MAX + 1);
        assert!(matches!(
            admit_pair(&exact, ""),
            PairAdmission::Admitted { .. }
        ));
        assert!(matches!(
            admit_pair(&plus_one, ""),
            PairAdmission::SummaryOnly
        ));
        assert!(matches!(
            admit_pair("", &plus_one),
            PairAdmission::SummaryOnly
        ));
    }

    #[test]
    fn digest_is_stable_and_side_ordered() {
        assert_eq!(pair_digest(b"a", b"b"), pair_digest(b"a", b"b"));
        assert_ne!(pair_digest(b"a", b"b"), pair_digest(b"b", b"a"));
    }

    #[test]
    fn snapshot_wire_counters_round_trip_exact_sqlite_range_as_decimal_strings() {
        let value = serde_json::json!({
            "projectId":"project", "chatId":"chat", "sessionId":"session",
            "runtimeEpoch":"9223372036854775807", "turnId":"turn", "itemId":"item",
            "diffOrdinal":0, "snapshotRevision":"9223372036854775807", "rootId":"root",
            "relativePath":"src/a.ts", "contentDigest":"a".repeat(64),
        });
        let reference: DiffRef = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(reference.runtime_epoch, i64::MAX as u64);
        assert_eq!(reference.snapshot_revision, i64::MAX as u64);
        assert_eq!(serde_json::to_value(reference).unwrap(), value);
        for bad in [
            serde_json::json!(1),
            serde_json::json!("01"),
            serde_json::json!("9223372036854775808"),
        ] {
            let mut invalid = value.clone();
            invalid["snapshotRevision"] = bad;
            assert!(serde_json::from_value::<DiffRef>(invalid).is_err());
        }
    }

    #[test]
    fn diff_ref_rejects_foreign_and_malformed_identity() {
        let mut valid = DiffRef {
            project_id: "project".into(),
            chat_id: "chat".into(),
            session_id: "session".into(),
            runtime_epoch: 1,
            turn_id: "turn".into(),
            item_id: "item".into(),
            diff_ordinal: 0,
            snapshot_revision: 1,
            root_id: "root".into(),
            relative_path: "src/lib.rs".into(),
            content_digest: "a".repeat(64),
        };
        assert!(valid.validate().is_ok());
        valid.relative_path = "/absolute".into();
        assert_eq!(valid.validate(), Err(SnapshotReadError::Forbidden));
        valid.relative_path = "src/lib.rs".into();
        valid.snapshot_revision = 0;
        assert_eq!(valid.validate(), Err(SnapshotReadError::Stale));
    }

    #[test]
    fn ref_rejects_windows_absolute_paths_and_wrapping_sqlite_counters() {
        let mut reference = DiffRef {
            project_id: "project".into(),
            chat_id: "chat".into(),
            session_id: "session".into(),
            runtime_epoch: 1,
            turn_id: "turn".into(),
            item_id: "item".into(),
            diff_ordinal: 0,
            snapshot_revision: 1,
            root_id: "root".into(),
            relative_path: "C:/secret".into(),
            content_digest: "a".repeat(64),
        };
        assert_eq!(reference.validate(), Err(SnapshotReadError::Forbidden));
        reference.relative_path = "src/lib.rs".into();
        reference.snapshot_revision = i64::MAX as u64 + 1;
        assert_eq!(reference.validate(), Err(SnapshotReadError::Stale));
        reference.snapshot_revision = 1;
        reference.runtime_epoch = i64::MAX as u64 + 1;
        assert_eq!(reference.validate(), Err(SnapshotReadError::Stale));
    }
}
