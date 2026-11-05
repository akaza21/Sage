use super::*;
#[test]
fn selectors_reject_partial_mixed_and_unavailable_builds() {
    let partial = [Some(OsStr::new("fixture.exe")), None, None, None];
    for index in 0..4 {
        let mut inputs = [[None; 4]; 4];
        inputs[index] = partial;
        assert!(fixture_selector::validate_inputs(inputs, true).is_err());
        assert!(fixture_selector::validate_inputs(inputs, false).is_err());
    }
    let complete = [Some(OsStr::new("synthetic")); 4];
    for first in 0..4 {
        for second in first + 1..4 {
            let mut inputs = [[None; 4]; 4];
            inputs[first] = complete;
            inputs[second] = complete;
            assert!(fixture_selector::validate_inputs(inputs, true).is_err());
        }
    }
    assert!(fixture_selector::validate_inputs([[None; 4]; 4], false).is_ok());
    assert!(
        fixture_selector::validate_inputs([complete, [None; 4], [None; 4], [None; 4]], false)
            .is_err()
    );
}
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[test]
fn claude_selector_rejects_wrong_protocol_hash_drift_and_reuse() {
    use sha2::{Digest, Sha256};
    let proof = crate::test_paths::temp_dir().join(format!("sage-nc1-{}", uuid::Uuid::new_v4()));
    let profile_root = proof.join("profile");
    let project = proof.join("project");
    fs::create_dir_all(&profile_root).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(profile_root.join(PROFILE_MARKER_NAME), PROFILE_MARKER_BYTES).unwrap();
    let artifact = proof.join("fixture.exe");
    fs::write(&artifact, b"synthetic fixture").unwrap();
    let profile = resolve_launch_profile(
        Some(profile_root.as_os_str()),
        Some(project.as_os_str()),
        proof.join("normal"),
        proof.join("normal-webview"),
    )
    .unwrap();
    let hash = format!("{:x}", Sha256::digest(b"synthetic fixture"));
    let select = |case: &str, digest: &str, protocol: &str| {
        fixture_selector::resolve_selection(
            &profile,
            [
                Some(artifact.as_os_str()),
                Some(OsStr::new(digest)),
                Some(OsStr::new("17")),
                Some(OsStr::new(case)),
            ],
            &proof,
            protocol,
        )
    };
    for protocol in ["claude", "pi"] {
        assert!(select("codex-turn-completed", &hash, protocol).is_err());
        assert!(select(&format!("{protocol}-unknown"), &hash, protocol).is_err());
        assert!(
            select(
                &format!("{protocol}-turn-completed"),
                &"00".repeat(32),
                protocol
            )
            .is_err()
        );
        let selection = select(&format!("{protocol}-turn-completed"), &hash, protocol)
            .unwrap()
            .unwrap();
        selection.revalidate().unwrap();
    }
    let selection = select("pi-turn-completed", &hash, "pi").unwrap().unwrap();
    selection.consume_one_shot().unwrap();
    assert!(selection.consume_one_shot().is_err());
    fs::write(artifact, b"changed artifact!").unwrap();
    assert!(selection.revalidate().is_err());
    fs::remove_dir_all(proof).unwrap();
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[test]
fn desktop_codex_model_listing_is_enabled_only_for_model_and_effort_cases() {
    use crate::acp::bindings::codex::{AdmittedCodexBinding, CodexAdapter};
    use crate::acp::contract::{RuntimeEpoch, SageAgentSessionId};
    use sha2::{Digest, Sha256};
    let proof =
        crate::test_paths::temp_dir().join(format!("sage-nc1-models-{}", uuid::Uuid::new_v4()));
    let profile_root = proof.join("profile");
    let project = proof.join("project");
    fs::create_dir_all(&profile_root).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(profile_root.join(PROFILE_MARKER_NAME), PROFILE_MARKER_BYTES).unwrap();
    let artifact = proof.join("fixture.exe");
    fs::write(&artifact, b"synthetic fixture").unwrap();
    let profile = resolve_launch_profile(
        Some(profile_root.as_os_str()),
        Some(project.as_os_str()),
        proof.join("normal"),
        proof.join("normal-webview"),
    )
    .unwrap();
    let hash = format!("{:x}", Sha256::digest(b"synthetic fixture"));
    for (case, models) in [
        ("codex-effort-menu", true),
        ("codex-model-menu", true),
        ("codex-turn-completed", false),
        ("codex-browse-preview", false),
        ("codex-steer-lost-result", false),
    ] {
        let selection = fixture_selector::resolve_selection(
            &profile,
            [
                Some(artifact.as_os_str()),
                Some(OsStr::new(&hash)),
                Some(OsStr::new("17")),
                Some(OsStr::new(case)),
            ],
            &proof,
            "codex",
        )
        .unwrap()
        .unwrap();
        let binding = AdmittedCodexBinding::admit_desktop_fixture(selection).unwrap();
        let mut adapter = CodexAdapter::from_admitted_binding(
            &binding,
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("fixture-session").unwrap(),
        );
        let initialize = serde_json::json!({
            "userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
            "codexHome":proof.to_str().unwrap(),"platformFamily":"windows","platformOs":"windows"
        });
        adapter
            .accept_initialize(initialize.as_object().unwrap())
            .unwrap();
        assert_eq!(adapter.models(2, None).is_ok(), models, "{case}");
    }
    fs::remove_dir_all(proof).unwrap();
}
