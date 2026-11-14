use super::*;
use crate::acp::projection::FilePairState;

pub(super) struct PairView {
    pub state: FilePairState,
    pub revision: u64,
    pub old_bytes: Option<u64>,
    pub new_bytes: Option<u64>,
    pub digest: Option<String>,
}

pub(super) fn admitted_pair_view(
    truncated: bool,
    snapshot_revision: u64,
    old_bytes: u64,
    new_bytes: u64,
    content_digest: String,
    pair_available: bool,
) -> Result<PairView, StoreError> {
    if !pair_available {
        if snapshot_revision != 0 || old_bytes != 0 || new_bytes != 0 || !content_digest.is_empty()
        {
            return Err(StoreError::Corrupt("unavailable file pair metadata"));
        }
        return Ok(PairView {
            state: if truncated {
                FilePairState::SummaryTruncated
            } else {
                FilePairState::Unavailable
            },
            revision: 0,
            old_bytes: None,
            new_bytes: None,
            digest: None,
        });
    }
    if truncated
        || snapshot_revision == 0
        || snapshot_revision > i64::MAX as u64
        || old_bytes > crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX as u64
        || new_bytes > crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX as u64
        || content_digest.len() != 64
        || !content_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(StoreError::Corrupt("file pair metadata"));
    }
    Ok(PairView {
        state: FilePairState::Available,
        revision: snapshot_revision,
        old_bytes: Some(old_bytes),
        new_bytes: Some(new_bytes),
        digest: Some(content_digest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_legacy_and_truncated_pairs_have_distinct_truthful_states() {
        for (truncated, expected) in [
            (false, FilePairState::Unavailable),
            (true, FilePairState::SummaryTruncated),
        ] {
            let view = admitted_pair_view(truncated, 0, 0, 0, String::new(), false).unwrap();
            assert_eq!(view.state, expected);
            assert!(view.old_bytes.is_none() && view.new_bytes.is_none() && view.digest.is_none());
        }
        assert!(admitted_pair_view(false, 1, 0, 0, String::new(), false).is_err());
        assert!(admitted_pair_view(false, 0, 1, 0, String::new(), false).is_err());
        assert!(admitted_pair_view(false, 0, 0, 1, String::new(), false).is_err());
        assert!(admitted_pair_view(false, 0, 0, 0, "a".repeat(64), false).is_err());
    }

    #[test]
    fn available_pair_rejects_wrapping_revision_and_raw_side_plus_one() {
        let cap = crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX as u64;
        assert!(admitted_pair_view(false, i64::MAX as u64, cap, cap, "a".repeat(64), true).is_ok());
        assert!(
            admitted_pair_view(false, i64::MAX as u64 + 1, cap, cap, "a".repeat(64), true).is_err()
        );
        assert!(admitted_pair_view(false, 1, cap + 1, cap, "a".repeat(64), true).is_err());
        assert!(admitted_pair_view(false, 1, cap, cap + 1, "a".repeat(64), true).is_err());
    }
}
