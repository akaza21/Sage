use super::{earlier_codex::continues_on_pinned_codex, *};
use crate::acp::bindings::codex::{CODEX_BINDING_ID, CODEX_PINNED_VERSION};

fn session_at(version: &str) -> DurableSession {
    DurableSession {
        binding_id: CODEX_BINDING_ID.into(),
        binding_digest: "e".repeat(64),
        harness_version: Some(version.into()),
        model_id: None,
        external_session_id: Some("thread-before-upgrade".into()),
        resumed_thread: None,
        state: "ready".into(),
        residency: "cold".into(),
    }
}

fn next_minor() -> String {
    let mut parts = CODEX_PINNED_VERSION.split('.');
    let major = parts.next().unwrap();
    let minor: u64 = parts.next().unwrap().parse().unwrap();
    format!("{major}.{}.0", minor + 1)
}

#[test]
fn only_an_earlier_parked_codex_session_continues_on_the_pinned_release() {
    for earlier in ["0.145.0", "0.146.0"] {
        assert!(
            continues_on_pinned_codex(&session_at(earlier), CODEX_BINDING_ID, CODEX_PINNED_VERSION),
            "{earlier}"
        );
    }
    for refused in [
        next_minor(),
        CODEX_PINNED_VERSION.to_owned(),
        "0.146".into(),
        "earlier".into(),
    ] {
        assert!(
            !continues_on_pinned_codex(
                &session_at(&refused),
                CODEX_BINDING_ID,
                CODEX_PINNED_VERSION
            ),
            "{refused}"
        );
    }
    let mut live = session_at("0.146.0");
    live.residency = "hot".into();
    let mut unbound = session_at("0.146.0");
    unbound.external_session_id = None;
    let mut unversioned = session_at("0.146.0");
    unversioned.harness_version = None;
    for session in [live, unbound, unversioned] {
        assert!(!continues_on_pinned_codex(
            &session,
            CODEX_BINDING_ID,
            CODEX_PINNED_VERSION
        ));
    }
}

#[test]
fn a_later_codex_session_is_refused() {
    let mut later = session_at(&next_minor());
    later.binding_digest = crate::acp::bindings::codex::CODEX_BINDING_DIGEST_HEX.into();
    assert!(matches!(
        validate_session_identity(
            &later,
            CODEX_BINDING_ID,
            crate::acp::bindings::codex::CODEX_BINDING_DIGEST_HEX,
            CODEX_PINNED_VERSION,
        ),
        Err(CodexSessionError::Intent)
    ));
}

#[test]
fn a_linked_session_resumes_the_earlier_thread() {
    let mut linked = session_at(CODEX_PINNED_VERSION);
    linked.state = "created".into();
    linked.external_session_id = None;
    linked.resumed_thread = Some("thread-before-upgrade".into());
    assert!(matches!(
        activation_mode(linked, "root".into(), None),
        Ok(ActivationMode::Resume { external_session_id, .. })
            if external_session_id.as_str() == "thread-before-upgrade"
    ));
    let mut fresh = session_at(CODEX_PINNED_VERSION);
    fresh.state = "created".into();
    fresh.external_session_id = None;
    assert!(matches!(
        activation_mode(fresh, "root".into(), None),
        Ok(ActivationMode::New { .. })
    ));
}

#[test]
fn claude_sdk_chats_resume_after_a_claude_code_update() {
    let session = DurableSession {
        binding_id: "claude-code-sdk".into(),
        binding_digest: "digest-of-2.1.319".into(),
        harness_version: Some("2.1.319".into()),
        model_id: None,
        external_session_id: Some("0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90".into()),
        resumed_thread: None,
        state: "ready".into(),
        residency: "cold".into(),
    };
    assert!(
        validate_session_identity(&session, "claude-code-sdk", "digest-of-2.1.320", "2.1.320")
            .is_ok()
    );
    assert!(
        validate_session_identity(&session, "claude-agent-acp", "digest-of-2.1.320", "2.1.320")
            .is_err()
    );
}

#[test]
fn a_session_without_a_version_matches_only_a_binding_that_reports_none() {
    let mut unversioned = session_at("");
    unversioned.binding_id = "custom-acp-1".into();
    unversioned.harness_version = None;
    let digest = "e".repeat(64);
    assert!(validate_session_identity(&unversioned, "custom-acp-1", &digest, "").is_ok());
    assert!(validate_session_identity(&unversioned, "custom-acp-1", &digest, "1.0.0").is_err());
    let mut versioned = session_at("1.0.0");
    versioned.binding_id = "custom-acp-1".into();
    assert!(validate_session_identity(&versioned, "custom-acp-1", &digest, "").is_err());
    assert!(validate_session_identity(&versioned, "custom-acp-1", &digest, "1.0.0").is_ok());
}
