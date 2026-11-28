use super::*;

fn model() -> Vec<ModelOption> {
    vec![ModelOption {
        id: "gpt-x".into(),
        name: "GPT-X".into(),
        description: None,
        is_default: Some(true),
        context_window: None,
        effort_levels: Some(vec!["low".into()]),
    }]
}

fn ids(models: Option<Vec<ModelOption>>) -> Option<Vec<String>> {
    models.map(|models| models.into_iter().map(|model| model.id).collect())
}

#[test]
fn an_account_change_during_a_listing_keeps_that_listing_out() {
    let catalog = CodexModelCatalog::default();
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
        os::unix::{fs::PermissionsExt, process::CommandExt},
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
    fn a_probe_runs_in_the_profile_folder_with_only_the_launch_environment() {
        let server = FakeAppServer::new("");
        let recorded = server.directory.join("recorded");
        let body = format!(
            "{{ pwd -P; env | cut -d= -f1; }} > '{}'\n{}",
            recorded.display(),
            answering_app_server(&std::env::temp_dir())
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();
        let binding = server.binding();
        let launch: Vec<&str> = binding
            .launch_environment()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        let home = &binding
            .launch_environment()
            .iter()
            .find(|(key, _)| key == "HOME")
            .unwrap()
            .1;
        let inherited = std::env::vars()
            .map(|(key, _)| key)
            .find(|key| !launch.contains(&key.as_str()))
            .expect("the test process has a variable the launch environment leaves out");

        assert!(CodexModelCatalog::default().probed(&binding).is_some());

        let recorded = std::fs::read_to_string(recorded).unwrap();
        let mut lines = recorded.lines();
        assert_eq!(
            Path::new(lines.next().unwrap()),
            std::fs::canonicalize(home).unwrap()
        );
        let seen: Vec<&str> = lines.collect();
        assert!(!seen.contains(&inherited.as_str()), "{inherited} leaked");
        for key in seen {
            assert!(
                launch.contains(&key) || ["PWD", "SHLVL", "_"].contains(&key),
                "{key} was not in the launch environment"
            );
        }
    }

    #[test]
    fn a_descendant_that_escapes_the_tree_with_the_output_fails_cleanup_and_frees_the_reader() {
        let server = FakeAppServer::new("");
        let escaped = server.directory.join("escaped");
        let body = format!(
            "perl -e 'use POSIX; POSIX::setsid(); open(my $f, \">\", $ARGV[0]); print $f $$; close $f; sleep 30' '{}' &\n{}",
            escaped.display(),
            answering_app_server(&std::env::temp_dir())
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();
        let catalog = CodexModelCatalog::default();
        let started = Instant::now();

        let probed = catalog.probed_within(
            &server.binding(),
            Duration::from_secs(2),
            Duration::from_millis(500),
        );

        let elapsed = started.elapsed();
        let pid: libc::pid_t = std::fs::read_to_string(&escaped)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: kills the escaped descendant this test's probe started.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        assert!(
            probed.is_none(),
            "zero was reported while the output stayed open"
        );
        assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        while catalog.unresolved.load(Ordering::Acquire) > 0 {
            assert!(Instant::now() < deadline, "the stopped reader never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_child_still_holding_the_probes_output_is_killed_with_it() {
        let server = FakeAppServer::new("");
        let recorded = server.directory.join("child");
        let body = format!(
            "sleep 60 &\necho $! > '{}'\n{}",
            recorded.display(),
            answering_app_server(&std::env::temp_dir())
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();

        assert!(
            CodexModelCatalog::default()
                .probed(&server.binding())
                .is_some()
        );

        let child: libc::pid_t = std::fs::read_to_string(recorded)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: signal 0 only checks that the process exists.
        let alive = || unsafe { libc::kill(child, 0) } == 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive(), "the probe's child outlived it");
    }

    #[test]
    fn a_listing_read_before_an_account_change_is_neither_kept_nor_offered() {
        let body = format!("sleep 1\n{}", answering_app_server(&std::env::temp_dir()));
        let server = FakeAppServer::new(&body);
        let catalog = CodexModelCatalog::default();
        let binding = server.binding();

        let models = std::thread::scope(|scope| {
            let probing = scope.spawn(|| catalog.probed(&binding));
            std::thread::sleep(Duration::from_millis(300));
            catalog.forget();
            probing.join().unwrap()
        });

        assert!(models.is_none());
        assert!(catalog.known(binding.binding_digest()).is_none());
        assert!(catalog.probed(&binding).is_some());
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
                    .probed_within(
                        &binding,
                        Duration::from_millis(500),
                        crate::acp::process::graceful_exit::close_budget()
                    )
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

    /// Waits for the fake CLI to create `path`, which it does only once it runs.
    fn await_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_probe_that_fails_after_a_newer_listing_landed_keeps_that_listing() {
        let server = FakeAppServer::new("");
        let started = server.directory.join("started");
        let refreshed = server.directory.join("refreshed");
        let body = format!(
            ": > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\nexit 1",
            started.display(),
            refreshed.display()
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();
        let catalog = CodexModelCatalog::default();
        let binding = server.binding();
        let digest = binding.binding_digest();

        let probed = std::thread::scope(|scope| {
            let probing = scope.spawn(|| catalog.probed(&binding));
            await_file(&started);
            assert!(catalog.refresh(catalog.generation(), digest, model()));
            std::fs::write(&refreshed, "").unwrap();
            probing.join().unwrap()
        });

        let listed = Some(vec!["gpt-x".to_owned()]);
        assert_eq!(ids(probed), listed);
        assert_eq!(ids(catalog.known(digest)), listed);
        std::fs::remove_file(server.script()).unwrap();
        assert_eq!(ids(catalog.probed(&binding)), listed);
    }

    #[test]
    fn no_probe_starts_while_too_many_probe_resources_are_unreleased() {
        let server = FakeAppServer::new("");
        let ran = server.directory.join("ran");
        let body = format!(
            ": > '{}'\n{}",
            ran.display(),
            answering_app_server(&std::env::temp_dir())
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();
        let binding = server.binding();
        let catalog = CodexModelCatalog::default();
        catalog.unresolved.store(MAX_UNRESOLVED, Ordering::Release);

        assert!(catalog.probed(&binding).is_none());
        assert!(!ran.exists(), "a probe started past the unreleased cap");

        let catalog = CodexModelCatalog::default();
        catalog
            .unresolved
            .store(MAX_UNRESOLVED - 1, Ordering::Release);
        assert!(catalog.probed(&binding).is_some());
        assert!(ran.exists());
    }

    /// A probe of a CLI that runs until it is killed, whose tree never
    /// reports empty, with its teardown due `cleanup` from now.
    fn unemptied_probe<'a>(
        server: &FakeAppServer,
        binding: &'a AdmittedCodexBinding,
        cleanup: Duration,
    ) -> ProbeTransport<'a> {
        sage_test_support::test_executable::write(
            server.script(),
            "#!/bin/sh\nexec /bin/sleep 30\n",
        )
        .unwrap();
        let mut command = Command::new(server.script());
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        isolate(&mut command);
        let mut transport = ProbeTransport::spawn(
            command,
            binding,
            Instant::now() + cleanup,
            Arc::new(AtomicUsize::new(0)),
        )
        .unwrap();
        transport.tree_empty = |_| false;
        transport
    }

    #[test]
    fn dropping_a_probe_tears_it_down_by_its_own_cleanup_deadline() {
        let server = FakeAppServer::new("");
        let binding = server.binding();
        let transport = unemptied_probe(&server, &binding, Duration::from_millis(300));
        let cleanup_end = transport.cleanup_end;

        drop(transport);

        let late = Instant::now().saturating_duration_since(cleanup_end);
        assert!(late < Duration::from_secs(1), "teardown ran {late:?} late");
    }

    #[test]
    fn a_tree_left_occupied_past_the_cleanup_deadline_proves_no_zero_in_time() {
        let server = FakeAppServer::new("");
        let binding = server.binding();
        let mut transport = unemptied_probe(&server, &binding, Duration::from_secs(5));
        let deadline = CodexDiscoveryDeadline::starting_now(
            Duration::from_millis(1),
            Duration::from_millis(300),
        )
        .unwrap();

        let zero = transport.terminate_zero(false, deadline);

        assert!(matches!(zero, Err(CodexDiscoveryRunError::Cleanup)));
        let late = Instant::now().saturating_duration_since(deadline.cleanup_end());
        assert!(late < Duration::from_secs(1), "teardown ran {late:?} late");
    }

    /// Linux keeps a killed, unreaped member in its group, so a real member
    /// the test holds unreaped keeps the probe from proving zero processes.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tree_that_outlives_its_cleanup_deadline_fails_the_probe_in_time() {
        let server = FakeAppServer::new("");
        let leader = server.directory.join("leader");
        let joined = server.directory.join("joined");
        let body = format!(
            "echo $$ > '{}.tmp' && mv '{0}.tmp' '{0}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n{}",
            leader.display(),
            joined.display(),
            answering_app_server(&std::env::temp_dir())
        );
        sage_test_support::test_executable::write(server.script(), format!("#!/bin/sh\n{body}\n"))
            .unwrap();
        let catalog = CodexModelCatalog::default();
        let binding = server.binding();
        let cleanup = Duration::from_millis(500);

        let (probed, elapsed, mut member) = std::thread::scope(|scope| {
            let probing = scope.spawn(|| {
                let started = Instant::now();
                let probed = catalog.probed_within(&binding, Duration::from_secs(2), cleanup);
                (probed, started.elapsed())
            });
            await_file(&leader);
            let group: libc::pid_t = std::fs::read_to_string(&leader)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let member = std::process::Command::new("/bin/sleep")
                .arg("30")
                .process_group(group)
                .spawn()
                .unwrap();
            std::fs::write(&joined, "").unwrap();
            let (probed, elapsed) = probing.join().unwrap();
            (probed, elapsed, member)
        });

        assert!(
            probed.is_none(),
            "zero was reported while a member was left"
        );
        assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
        assert!(catalog.known(binding.binding_digest()).is_none());
        member.wait().unwrap();
    }
}
