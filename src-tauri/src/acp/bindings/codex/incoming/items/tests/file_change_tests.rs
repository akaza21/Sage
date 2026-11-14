use super::*;
use crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX;

fn project(
    diff: &str,
) -> (
    NormalizedPayload,
    Option<super::super::file_change::FilePair>,
) {
    project_with_mapper(diff, &MappedResource)
}

fn project_with_mapper(
    diff: &str,
    mapper: &impl ResourceMapper,
) -> (
    NormalizedPayload,
    Option<super::super::file_change::FilePair>,
) {
    let file = item(json!({
        "type":"fileChange", "id":"edit", "status":"completed",
        "changes":[{"path":"src/example.ts", "kind":{"type":"update"}, "diff":diff}]
    }));
    let ((payload, _, _), pair) = normalize_file_change(&file, mapper, true).unwrap();
    (payload, pair)
}

struct ContextResource(String);

impl ResourceMapper for ContextResource {
    fn map_existing(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        MappedResource.map_existing(path)
    }

    fn read_diff_context(&self, _: &SafeResourceRef) -> Option<String> {
        Some(self.0.clone())
    }
}

#[test]
fn large_blank_line_context_reconstructs_disjoint_hunks_in_both_directions() {
    let gap = "\n".repeat(260_000);
    let old_text = format!("\nold\n{gap}tail\nend");
    let new_text = format!("\nnew\nextra\n{gap}last\nend");
    let diff = concat!(
        "@@ -2 +2,2 @@\n-old\n+new\n+extra\n",
        "@@ -260003 +260004 @@\n-tail\n+last\n",
    );
    for context in [&old_text, &new_text] {
        let (payload, pair) = project_with_mapper(diff, &ContextResource(context.clone()));
        assert!(matches!(
            payload,
            NormalizedPayload::FileChangeDiff {
                additions: Some(3),
                deletions: Some(2),
                pair_available: true,
                truncated: false,
                ..
            }
        ));
        let pair = pair.unwrap();
        assert_eq!(pair.old_text, old_text);
        assert_eq!(pair.new_text, new_text);
    }
}

#[test]
fn out_of_range_hunk_is_invalid_even_when_its_partial_gap_would_exceed_the_cap() {
    let (payload, pair) = project_with_mapper(
        "@@ -0,0 +1 @@\n+x\n@@ -262146 +262147 @@\n-a\n+b\n",
        &ContextResource("\n".repeat(DIFF_SIDE_BYTES_MAX)),
    );
    assert!(pair.is_none());
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            pair_available: false,
            truncated: false,
            additions: Some(2),
            deletions: Some(1),
            ..
        }
    ));
}

#[test]
fn reconstructed_pairs_retain_eof_and_fail_closed_at_the_shared_side_cap() {
    let (_, pair) = project_with_mapper(
        concat!(
            "@@ -1 +1 @@\n-a\n+d\n",
            "@@ -3 +3 @@\n-c\n\\ No newline at end of file\n+z\n\\ No newline at end of file\n",
        ),
        &ContextResource("a\nb\nc".into()),
    );
    let pair = pair.unwrap();
    assert_eq!(pair.old_text, "a\nb\nc");
    assert_eq!(pair.new_text, "d\nb\nz");

    let context = format!("a\n{}\nz\n", "x".repeat(DIFF_SIDE_BYTES_MAX - 6));
    let (payload, pair) = project_with_mapper(
        "@@ -1 +1 @@\n-a\n+aa\n@@ -3 +3 @@\n-z\n+zz\n",
        &ContextResource(context),
    );
    assert!(pair.is_none());
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            pair_available: false,
            truncated: true,
            ..
        }
    ));
}

#[test]
fn saved_patch_counts_only_changed_lines_and_retains_context_and_eof() {
    let (payload, pair) = project(concat!(
        "--- a/src/example.ts\n+++ b/src/example.ts\n",
        "@@ -4,4 +4,5 @@ function example\n context\n-old\n+new\n+extra\n tail\n-last\n+next\n",
        "\\ No newline at end of file\n",
    ));
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            additions: Some(3),
            deletions: Some(2),
            pair_available: true,
            ..
        }
    ));
    let pair = pair.unwrap();
    assert_eq!(pair.old_text, "context\nold\ntail\nlast\n");
    assert_eq!(pair.new_text, "context\nnew\nextra\ntail\nnext");
}

#[test]
fn disjoint_hunks_without_verified_context_keep_counts_but_no_saved_pair() {
    let (payload, pair) = project("@@ -1 +0,0 @@\n-a\n@@ -3,0 +3 @@\n+a\n");
    assert!(pair.is_none());
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            additions: Some(1),
            deletions: Some(1),
            pair_available: false,
            ..
        }
    ));
}

#[test]
fn malformed_or_incomplete_patches_never_publish_partial_saved_text() {
    for diff in [
        "opaque",
        "@@ -1 +7 @@\n-old\n+new\n",
        "@@ -1,2 +1 @@\n-old\n+new\n",
        "@@ -1 +1 @@\n-old\n+new\n+excess\n",
        "@@ -1 +1 @@\n-old\n+new\n@@ -1 +1 @@\n-old\n+new\n",
        "@@ -1 +1 @@\n-old\n+new\n@@ -4 +5 @@\n-old\n+new\n",
        "@@ -999999999999999999999999999999 +1 @@\n-old\n+new\n",
        "@@ -1 +1 @@\n\\ No newline at end of file\n-old\n+new\n",
        "@@ -1,2 +1 @@\n-old\n\\ No newline at end of file\n-second\n+new\n",
        "@@ -1 +1 @@\n-old\n+new\ndiff --git a/other b/other\n",
    ] {
        let (payload, pair) = project(diff);
        assert!(pair.is_none(), "{diff}");
        assert!(
            matches!(
                payload,
                NormalizedPayload::FileChangeDiff {
                    pair_available: false,
                    additions: None,
                    deletions: None,
                    ..
                }
            ),
            "{diff}"
        );
    }
}

#[test]
fn patch_sides_admit_the_shared_cap_and_overflow_to_truncated_summary() {
    let exact = format!("{}\n", "x".repeat(DIFF_SIDE_BYTES_MAX - 1));
    let (payload, pair) = project(&format!("@@ -0,0 +1 @@\n+{exact}"));
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            pair_available: true,
            additions: Some(1),
            deletions: Some(0),
            ..
        }
    ));
    assert_eq!(pair.unwrap().new_text, exact);

    let oversized = format!("{}x\n", "x".repeat(DIFF_SIDE_BYTES_MAX - 1));
    for diff in [
        format!("@@ -0,0 +1 @@\n+{oversized}"),
        "x".repeat(DIFF_SIDE_BYTES_MAX * 2 + 1),
        "@@ -0,0 +0,0 @@\n".repeat(1025),
    ] {
        let (payload, pair) = project(&diff);
        assert!(pair.is_none());
        assert!(matches!(
            payload,
            NormalizedPayload::FileChangeDiff {
                pair_available: false,
                truncated: true,
                old_bytes: 0,
                new_bytes: 0,
                ..
            }
        ));
    }
}
