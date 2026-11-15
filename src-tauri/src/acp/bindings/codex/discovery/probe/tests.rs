use super::*;

#[test]
fn an_account_change_during_a_listing_keeps_that_listing_out() {
    let catalog = CodexModelCatalog::default();
    let model = || vec![listed_model("gpt-x", "GPT-X", true, ["low"].into_iter())];
    let began = catalog.generation();

    catalog.forget();

    assert!(!catalog.refresh(began, [7; 32], model()));
    assert!(catalog.known([7; 32]).is_none());
    assert!(catalog.refresh(catalog.generation(), [7; 32], model()));
    assert!(catalog.known([7; 32]).is_some());
    assert!(catalog.known([8; 32]).is_none());
    catalog.forget();
    assert!(catalog.known([7; 32]).is_none());
}

#[cfg(unix)]
mod fake_app_server {
    use std::{
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::*;

    const CODEX_VERSION: &str = "codex-cli 0.167.1";

    /// A fake `codex app-server` running `body` as a shell script.
    struct FakeAppServer {
        directory: PathBuf,
    }

    impl FakeAppServer {
        fn new(body: &str) -> Self {
            let directory =
                std::env::temp_dir().join(format!("sage-codex-probe-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&directory).unwrap();
            let script = directory.join("codex");
            sage_test_support::test_executable::write(&script, format!("#!/bin/sh\n{body}\n"))
                .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { directory }
        }

        fn script(&self) -> PathBuf {
            self.directory.join("codex")
        }

        fn binding(&self) -> AdmittedCodexBinding {
            AdmittedCodexBinding::admit_observed_for_test(self.script(), CODEX_VERSION.into())
                .unwrap()
        }
    }

    impl Drop for FakeAppServer {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn line(value: serde_json::Value) -> String {
        format!("echo '{value}'")
    }

    /// Answers `initialize`, then `account/read` and the first `model/list`, as
    /// Codex 0.167.1 does for a signed-out home, and exits when stdin closes.
    fn answering_app_server(home: &Path) -> String {
        let models = json!({"id": 3, "result": {"nextCursor": null, "data": [
            {"id": "gpt-6.1-sol", "displayName": "GPT-6.1-Sol", "isDefault": true,
             "hidden": false, "upgradeInfo": null, "serviceTiers": [],
             "supportedReasoningEfforts": [
                {"reasoningEffort": "low", "description": "Fast"},
                {"reasoningEffort": "max", "description": "Maximum"},
                {"reasoningEffort": "ultra", "description": "Delegates"}]},
            {"id": "gpt-hidden", "displayName": "Hidden", "hidden": true},
            {"id": "gpt-6-luna", "displayName": "GPT-6-Luna", "isDefault": false,
             "supportedReasoningEfforts": []}
        ]}});
        [
            "read request".to_owned(),
            line(json!({"id": 1, "result": {
                "userAgent": "sage/0.167.1 (test; x86_64) unix (sage; 0.1.0)",
                "codexHome": home.join(".codex"),
                "platformFamily": "unix", "platformOs": "linux"}})),
            "read initialized".to_owned(),
            line(json!({"method": "remoteControl/status/changed",
                "params": {"status": "disabled"}})),
            "read account".to_owned(),
            "read models".to_owned(),
            line(json!({"id": 2, "result": {"account": null, "requiresOpenaiAuth": true}})),
            line(models),
            "while read rest; do :; done".to_owned(),
        ]
        .join("\n")
    }

    #[test]
    fn a_signed_out_build_lists_its_visible_models_with_their_thinking_levels() {
        let server = FakeAppServer::new(&answering_app_server(&std::env::temp_dir()));
        let catalog = CodexModelCatalog::default();
        let binding = server.binding();

        let models = catalog.probed(&binding).unwrap();

        assert_eq!(
            serde_json::to_value(&models).unwrap(),
            json!([
                {"id": "gpt-6.1-sol", "name": "GPT-6.1-Sol", "description": null, "isDefault": true,
                 "effortLevels": ["low", "max", "ultra"]},
                {"id": "gpt-6-luna", "name": "GPT-6-Luna", "description": null, "isDefault": false}
            ])
        );
        assert_eq!(
            serde_json::to_value(catalog.known(binding.binding_digest())).unwrap(),
            serde_json::to_value(&models).unwrap()
        );
    }

    #[test]
    fn a_build_that_answers_garbage_or_hangs_lists_nothing_and_is_not_probed_again_at_once() {
        for body in [
            "read request\necho 'not json'\nwhile read rest; do :; done".to_owned(),
            "trap '' HUP TERM\nexec /bin/sleep 30".to_owned(),
        ] {
            let server = FakeAppServer::new(&body);
            let catalog = CodexModelCatalog::default();
            let binding = server.binding();
            let started = Instant::now();

            assert!(
                catalog
                    .probed_within(&binding, Duration::from_millis(500))
                    .is_none()
            );
            assert!(started.elapsed() < Duration::from_secs(10), "{body}");
            assert!(catalog.known(binding.binding_digest()).is_none());

            std::fs::remove_file(server.script()).unwrap();
            let retried = Instant::now();
            assert!(catalog.probed(&binding).is_none());
            assert!(retried.elapsed() < Duration::from_millis(100));
        }
    }
}
