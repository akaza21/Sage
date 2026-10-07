use super::*;

#[test]
fn an_observed_exit_code_is_reported_in_the_detail() {
    let detail = harness_exited(Some(TreeExit::Exited { code: Some(9) })).expect("detail");
    assert_eq!(detail, "harness process exited with code 9");
}

#[test]
fn an_exit_without_a_code_never_claims_one() {
    let detail = harness_exited(Some(TreeExit::Exited { code: None })).expect("detail");
    assert_eq!(
        detail,
        "harness process exited without reporting an exit code"
    );
    assert!(harness_exited(None).is_none());
}

#[test]
fn every_tree_exit_produces_a_nonempty_bounded_detail() {
    for exit in [
        TreeExit::Exited { code: Some(-1) },
        TreeExit::Exited { code: None },
        TreeExit::Cancelled,
        TreeExit::Deadline,
        TreeExit::IdentityMismatch,
        TreeExit::ContainmentFailure,
        TreeExit::ProtocolFailure,
        TreeExit::ParentShutdown,
    ] {
        let detail = harness_exited(Some(exit)).expect("detail");
        assert!(!detail.is_empty());
        assert!(detail.chars().count() <= DETAIL_CHARS);
    }
}

#[test]
fn a_protocol_summary_never_leaks_a_path_or_a_credential() {
    let detail = protocol_error("decode failed at C:\\Users\\alice\\creds using sk-live-42")
        .expect("detail");
    assert!(detail.contains("[path]"), "{detail}");
    assert!(detail.contains("[redacted]"), "{detail}");
    assert!(!detail.contains("alice"), "{detail}");
    assert!(!detail.contains("sk-live-42"), "{detail}");
}

#[test]
fn an_auth_message_url_never_persists_its_oauth_parameters() {
    let detail = auth_required(Some(
        "Visit https://auth.example.com/authorize?client_id=abc&state=xyz",
    ))
    .expect("detail");
    assert!(!detail.contains("client_id=abc"), "{detail}");
    assert!(!detail.contains("state=xyz"), "{detail}");
    assert!(detail.contains("[redacted]"), "{detail}");
}

#[test]
fn a_sage_authored_remediation_keeps_its_slash_command_verbatim() {
    assert_eq!(
        remediation(Some("Run /login to continue")).expect("remediation"),
        "Run /login to continue"
    );
}

#[test]
fn a_harness_message_is_sanitized_while_its_remediation_is_not() {
    let message =
        auth_required(Some("failed at C:/Users/person/.claude/creds.json")).expect("message");
    assert!(!message.contains("creds.json"), "{message}");
    assert_eq!(
        remediation(Some("Run /login to continue")).expect("remediation"),
        "Run /login to continue"
    );
}

#[test]
fn an_auth_detail_without_evidence_stays_absent_rather_than_inventing_text() {
    assert!(auth_required(None).is_none());
    assert!(auth_required(Some("   ")).is_none());
    assert!(remediation(None).is_none());
    assert!(remediation(Some("")).is_none());
}

#[test]
fn a_refused_version_names_the_observed_version_and_the_characterised_set() {
    let detail = refused_version("acp-9", &["acp-1", "acp-2"]).expect("detail");
    assert_eq!(
        detail,
        "harness reported protocol version acp-9, but Sage supports acp-1, acp-2"
    );
}

#[test]
fn a_refused_version_without_a_characterised_set_says_so() {
    let detail = refused_version("acp-9", &[]).expect("detail");
    assert_eq!(
        detail,
        "harness reported protocol version acp-9, which Sage has not characterised"
    );
}

#[test]
fn an_oversized_detail_is_bounded_and_elided() {
    let detail = protocol_error(&"o".repeat(DETAIL_CHARS * 4)).expect("detail");
    assert_eq!(detail.chars().count(), DETAIL_CHARS);
    assert!(detail.ends_with('…'));
}
