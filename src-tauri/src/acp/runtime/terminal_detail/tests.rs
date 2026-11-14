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

fn lost(reason: TurnTerminalReason, message: &str) -> TurnTerminalOutcome {
    TurnTerminalOutcome {
        reason,
        message: Some(message.to_owned()),
        remediation: None,
    }
}

#[test]
fn a_lost_runtime_is_classified_by_how_its_tree_ended() {
    for (exit, expected) in [
        (
            TreeExit::Exited { code: Some(1) },
            lost(
                TurnTerminalReason::HarnessExited,
                "harness process exited with code 1",
            ),
        ),
        (
            TreeExit::ProtocolFailure,
            lost(
                TurnTerminalReason::ProtocolError,
                "harness process stream failed before a clean exit",
            ),
        ),
        (
            TreeExit::Deadline,
            lost(
                TurnTerminalReason::Timeout,
                "harness process exceeded its shutdown deadline",
            ),
        ),
        (
            TreeExit::ParentShutdown,
            lost(
                TurnTerminalReason::Other,
                "harness process stopped while Sage was shutting down",
            ),
        ),
    ] {
        assert_eq!(runtime_lost(exit, None, None), expected, "{exit:?}");
    }
}

#[test]
fn a_driver_classified_teardown_keeps_its_cause_and_detail() {
    let teardown = lost(
        TurnTerminalReason::RefusedVersion,
        "harness reported protocol version acp-9, which Sage has not characterised",
    );
    let outcome = runtime_lost(TreeExit::ProtocolFailure, Some(teardown.clone()), None);
    assert_eq!(outcome, teardown);
}

#[test]
fn a_harness_exit_names_the_exit_code_observed_after_the_driver_classified_it() {
    let teardown = lost(
        TurnTerminalReason::HarnessExited,
        "harness process exited without reporting an exit code",
    );
    let outcome = runtime_lost(TreeExit::Exited { code: Some(3) }, Some(teardown), None);
    assert_eq!(
        outcome,
        lost(
            TurnTerminalReason::HarnessExited,
            "harness process exited with code 3"
        )
    );
}

#[test]
fn a_harness_exit_cause_never_describes_a_stream_failure() {
    let teardown = TurnTerminalOutcome::from(TurnTerminalReason::HarnessExited);
    let outcome = runtime_lost(TreeExit::ProtocolFailure, Some(teardown), None);
    assert_eq!(
        outcome,
        lost(
            TurnTerminalReason::HarnessExited,
            "harness process exited without reporting an exit code"
        )
    );
}

#[test]
fn a_sage_requested_teardown_of_a_failed_turn_is_not_a_user_cancellation() {
    let teardown = TurnTerminalOutcome::from(TurnTerminalReason::CancelledByUser);
    let outcome = runtime_lost(TreeExit::Cancelled, Some(teardown), Some("late error"));
    assert_eq!(
        outcome,
        lost(
            TurnTerminalReason::Other,
            "harness process was terminated by Sage"
        )
    );
}

#[test]
fn an_intentional_stop_does_not_append_the_harness_error() {
    let outcome = runtime_lost(TreeExit::ParentShutdown, None, Some("broken pipe"));
    assert_eq!(
        outcome.message.as_deref(),
        Some("harness process stopped while Sage was shutting down")
    );
}

#[test]
fn the_harness_error_follows_the_exit_and_stays_sanitized_and_bounded() {
    let error = format!(
        r"fatal at C:\Users\alice\key using sk-live-42 {}",
        "x".repeat(DETAIL_CHARS * 2)
    );
    let outcome = runtime_lost(TreeExit::Exited { code: Some(1) }, None, Some(&error));
    let message = outcome.message.expect("message");
    assert!(
        message.starts_with("harness process exited with code 1: fatal at "),
        "{message}"
    );
    assert!(!message.contains("alice"), "{message}");
    assert!(!message.contains("sk-live-42"), "{message}");
    assert_eq!(message.chars().count(), DETAIL_CHARS);
}

#[test]
fn the_root_exit_replaces_a_stream_failure_seen_first_but_not_a_sage_stop() {
    let code_one = TreeExit::Exited { code: Some(1) };
    let no_code = TreeExit::Exited { code: None };
    for (runner, root, expected) in [
        (TreeExit::ProtocolFailure, code_one, code_one),
        (no_code, code_one, code_one),
        (code_one, TreeExit::Deadline, code_one),
        (TreeExit::ProtocolFailure, no_code, no_code),
        (TreeExit::Cancelled, code_one, TreeExit::Cancelled),
        (TreeExit::ParentShutdown, code_one, TreeExit::ParentShutdown),
    ] {
        assert_eq!(observed_exit(runner, root), expected, "{runner:?} {root:?}");
    }
}
