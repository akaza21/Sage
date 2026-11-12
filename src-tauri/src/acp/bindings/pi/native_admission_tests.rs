use super::*;

#[test]
fn every_pi_release_of_the_launched_package_is_admitted() {
    for admitted in [
        "0.74.0",
        "0.74.2",
        "1.4.9",
        "1.5.0",
        "1.5.2",
        "1.6.0",
        "2.0.0-beta.1",
    ] {
        assert!(version_admitted(admitted), "{admitted}");
    }
    for refused in ["0.73.9", "0.74.0-rc.1", "1.5", "garbage", ""] {
        assert!(!version_admitted(refused), "{refused}");
    }
}

#[test]
fn node_engine_is_checked_against_the_discovered_runtime() {
    assert_eq!(engine_satisfied(">=20.6.0", "22.18.0"), Some(true));
    assert_eq!(engine_satisfied(">=20.6.0", "20.5.9"), Some(false));
    assert_eq!(engine_satisfied("^20", "22.18.0"), None);
}

#[test]
fn launch_is_read_only_by_construction() {
    assert_eq!(
        &PI_READ_ONLY_ARGS[..4],
        ["--mode", "rpc", "--tools", "read"]
    );
    assert!(PI_READ_ONLY_ARGS.contains(&"--no-extensions"));
    // Templates, skills and saved sessions load as in Pi's own CLI.
    for loaded in ["--no-skills", "--no-prompt-templates", "--no-session"] {
        assert!(!PI_READ_ONLY_ARGS.contains(&loaded), "{loaded}");
    }
    assert!(
        !PI_READ_ONLY_ARGS
            .iter()
            .any(|arg| ["bash", "edit", "write"].contains(arg))
    );
}

#[test]
fn bridge_launch_registers_its_tools_without_enabling_builtin_writers() {
    assert_eq!(
        pi_launch_args(None),
        PI_READ_ONLY_ARGS.map(OsString::from).to_vec()
    );
    let bridge = PathBuf::from("bridge").join("extension.mjs");
    let args = pi_launch_args(Some(bridge.clone()));
    // Pi's --tools allowlist also filters extension tools, which would leave
    // the bridge's write, edit and bash unregistered.
    assert!(!args.iter().any(|arg| arg == "--tools"));
    assert!(args.iter().any(|arg| arg == "--no-builtin-tools"));
    for disabled in ["--no-extensions", "--no-themes"] {
        assert!(args.iter().any(|arg| arg == disabled), "{disabled}");
    }
    for loaded in ["--no-skills", "--no-prompt-templates", "--no-session"] {
        assert!(!args.iter().any(|arg| arg == loaded), "{loaded}");
    }
    assert_eq!(
        &args[args.len() - 2..],
        [OsString::from("--extension"), bridge.into_os_string()]
    );
}

fn discovered(version: &str) -> (PiLaunchDiscovery, PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-pi-admission-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("cli.js");
    let node = root.join("node.exe");
    std::fs::write(&entry, b"console.log('pi')\n").unwrap();
    std::fs::write(&node, b"synthetic node").unwrap();
    let digest = |path: &PathBuf| crate::harness_discovery::sha256_file(path).unwrap();
    (
        PiLaunchDiscovery::Present(Box::new(DiscoveredPiLaunch {
            entrypoint_sha256: digest(&entry),
            node_sha256: digest(&node),
            entrypoint_path: entry,
            package_version: version.into(),
            node_engine: ">=20.6.0".into(),
            node_path: node,
            node_version: "22.18.0".into(),
        })),
        root,
    )
}

#[test]
fn an_installed_pi_in_range_is_admitted_and_drift_revokes_it() {
    let (discovery, root) = discovered("1.5.2");
    let registration = PiBindingRegistration::from_discovery_for_test(discovery);
    let binding = registration.refresh().expect("in-range Pi is admitted");
    assert_eq!(binding.harness_version(), "1.5.2");
    assert_eq!(
        binding.characterisation(),
        HarnessCharacterisation::NewerThanTested
    );
    assert_eq!(binding.binding_digest_hex().len(), 64);
    std::fs::write(root.join("cli.js"), b"tampered").unwrap();
    assert_eq!(
        registration.refresh().err(),
        Some(PiAdmissionUnavailable::Drift)
    );
    assert!(!registration.is_available());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn older_and_newer_pi_releases_are_admitted_and_an_unreadable_or_too_old_one_is_not() {
    for version in ["0.74.2", "1.6.0"] {
        let (discovery, root) = discovered(version);
        let registration = PiBindingRegistration::from_discovery_for_test(discovery);
        assert_eq!(registration.unavailable(), None, "{version}");
        std::fs::remove_dir_all(root).unwrap();
    }
    for version in ["1.6", "0.73.0"] {
        let (discovery, root) = discovered(version);
        let registration = PiBindingRegistration::from_discovery_for_test(discovery);
        assert_eq!(
            registration.unavailable(),
            Some(PiAdmissionUnavailable::VersionUnsupported),
            "{version}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn installation_admission_does_not_invent_session_model_or_config_counts() {
    let (discovery, root) = discovered("1.5.2");
    let registry = crate::acp::bindings::registry::BindingRegistry::default();
    registry.pi_admission().apply_discovery(discovery);
    let record = registry.resolve(PI_BINDING_ID).unwrap();
    let capabilities = record.supported_capabilities().unwrap();
    assert_eq!(
        (capabilities.model_count, capabilities.config_count),
        (0, 0)
    );
    assert!(record.supported_capabilities().is_ok());
    std::fs::write(root.join("node.exe"), b"changed synthetic node").unwrap();
    assert!(record.supported_capabilities().is_err());
    std::fs::remove_dir_all(root).unwrap();
}
