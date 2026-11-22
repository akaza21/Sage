use super::*;

pub(super) fn check(launch: &crate::harness_discovery::DiscoveredPiLaunch) {
    initial_load_failure(launch);
    startup_failure(launch);
    reload_failure(launch);
}

fn profile(j: &Journey) -> PathBuf {
    let profile = j.root.join("profile");
    std::fs::create_dir_all(profile.join("extensions")).unwrap();
    std::fs::write(
        profile.join("extensions/approval.js"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/pi/extensions/approval.mjs"
        )),
    )
    .unwrap();
    profile
}

fn initial_load_failure(launch: &crate::harness_discovery::DiscoveredPiLaunch) {
    let j = Journey::new(SessionAccessMode::ReadOnly);
    let profile = profile(&j);
    let directory = j.root.join("bridge\u{a0}directory");
    std::fs::create_dir(&directory).unwrap();
    let bridge = directory.join("extension.mjs");
    let install = &crate::acp::bindings::pi::bridge::install::SOURCE;
    std::fs::write(&bridge, install).unwrap();
    crate::acp::bindings::pi::bridge::install::matches_install(&directory, &bridge).unwrap();
    let mut runtime = fixture_runtime(99);
    runtime.enable_bridge(sage_workspace::Workspace::open("project", "root-pi", &j.root).unwrap());
    let mut pi = Pi::start(launch, &j.root, &profile, bridge, "echo");
    for _ in 0..128 {
        match pi.frames.recv_timeout(Duration::from_secs(30)) {
            Ok(value) => {
                runtime.ingest(&frame(value), &Mapper).unwrap();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(error) => panic!("Pi must exit on its import failure: {error}"),
        }
    }
    assert_eq!(pi.child.wait().unwrap().code(), Some(1));
    assert!(
        !runtime.bridge.as_ref().unwrap().ready,
        "the NBSP import must actually fail"
    );
    let error = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "hello").unwrap(),
            client_message_id: RequiredText::new("client", "blocked").unwrap(),
            access_mode: SessionAccessMode::ReadOnly,
        })
        .err()
        .expect("no prompt may be admitted without the bridge");
    assert_eq!(error, AdapterRuntimeError::ApprovalBridgeUnavailable);
}

fn startup_failure(launch: &crate::harness_discovery::DiscoveredPiLaunch) {
    let j = Journey::new(SessionAccessMode::ReadOnly);
    let profile = profile(&j);
    let bridge = j.root.join("extension.mjs");
    let source = std::str::from_utf8(crate::acp::bindings::pi::bridge::install::SOURCE).unwrap();
    // Fault-inject a real filesystem failure at the bridge's startup hash read.
    let broken = source.replace(
        "digest: bridgeDigest(source)",
        "digest: bridgeDigest(`${source}.missing`)",
    );
    assert_ne!(source, broken);
    std::fs::write(&bridge, broken).unwrap();
    let mut runtime = fixture_runtime(99);
    runtime.enable_bridge(sage_workspace::Workspace::open("project", "root-pi", &j.root).unwrap());
    let mut pi = Pi::start(launch, &j.root, &profile, bridge, "echo");
    pi.send(json!({"type":"get_commands","id":"commands"}));
    let commands = (0..128)
        .find_map(|_| {
            let value = pi.next();
            if value["id"] == "commands" {
                return Some(value);
            }
            runtime.ingest(&frame(value), &Mapper).unwrap();
            None
        })
        .unwrap();
    assert!(
        commands["data"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "fixture-command"),
        "Pi continues discovering other extensions after bridge initialization fails"
    );
    assert!(
        !runtime.bridge.as_ref().unwrap().ready,
        "the real startup handler must fail before announcing readiness"
    );
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    pi.send(deliver(&mut runtime, initialize));
    let error = runtime
        .ingest(&frame(pi.next()), &Mapper)
        .err()
        .expect("failed bridge must close admission");
    assert_eq!(error, AdapterRuntimeError::ApprovalBridgeUnavailable);
    assert!(error.to_string().contains("No prompt was sent"));
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "hello").unwrap(),
                client_message_id: RequiredText::new("client", "blocked").unwrap(),
                access_mode: SessionAccessMode::ReadOnly,
            })
            .is_err()
    );
}

fn reload_failure(launch: &crate::harness_discovery::DiscoveredPiLaunch) {
    let mut j = Journey::new(SessionAccessMode::ReadOnly);
    j.settle();
    let profile = profile(&j);
    let bridge = j.root.join("extension.mjs");
    std::fs::write(&bridge, crate::acp::bindings::pi::bridge::install::SOURCE).unwrap();
    j.runtime
        .enable_bridge(sage_workspace::Workspace::open("project", "root-pi", &j.root).unwrap());
    let mut pi = Pi::start(launch, &j.root, &profile, bridge.clone(), "reload");
    pi.send(json!({"type":"get_commands","id":"commands"}));
    for _ in 0..128 {
        let value = pi.next();
        if value["id"] == "commands" {
            break;
        }
        j.runtime.ingest(&frame(value), &Mapper).unwrap();
    }
    assert!(
        j.runtime.bridge.as_ref().unwrap().ready,
        "initial bridge loaded"
    );
    std::fs::write(
        &bridge,
        "throw new Error('fixture bridge import failure');\n",
    )
    .unwrap();
    pi.send(json!({"type":"prompt","id":"reload","message":"/fixture-reload"}));
    let mut reloaded = false;
    for _ in 0..128 {
        let value = pi.next();
        if value["id"] == "reload" {
            assert_eq!(value["success"], true);
            reloaded = true;
            break;
        }
        j.runtime.ingest(&frame(value), &Mapper).unwrap();
    }
    assert!(reloaded);
    assert!(
        !j.runtime.bridge.as_ref().unwrap().ready,
        "old runner's readiness must be cleared"
    );
    let error = j
        .runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "hello").unwrap(),
            client_message_id: RequiredText::new("client", "after-failed-reload").unwrap(),
            access_mode: SessionAccessMode::ReadOnly,
        })
        .err()
        .expect("failed replacement bridge must close admission");
    assert_eq!(error, AdapterRuntimeError::ApprovalBridgeUnavailable);
}
