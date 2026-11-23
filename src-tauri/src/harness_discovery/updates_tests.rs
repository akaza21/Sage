use super::*;

fn temp_root() -> PathBuf {
    let root = sage_test_support::test_paths::temp_dir()
        .join(format!("sage-updates-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn only_a_final_release_alone_on_the_last_line_counts() {
    for (stdout, expected) in [
        (&b"0.168.0\n"[..], Some("0.168.0")),
        (b"npm warn config something\n0.168.0\n", Some("0.168.0")),
        (b"1.0.0-beta.1\n", None),
        (b"", None),
        (b"npm warn config\n0.1", None),
        (b"latest 0.168.0\n", None),
    ] {
        assert_eq!(
            parse_published_version(stdout).as_deref(),
            expected,
            "{stdout:?}"
        );
    }
}

#[test]
fn a_shim_beside_its_npm_package_updates_through_npm() {
    let root = temp_root();
    let shim = root.join("npm/codex.cmd");
    write(&shim, "");
    write(
        &root.join("npm/node_modules/@openai/codex/package.json"),
        "{}",
    );
    assert_eq!(
        update_sources(&[InstalledHarness {
            id: "codex",
            executable: shim.clone(),
            version: None,
        }]),
        [UpdateSource {
            harness_id: "codex",
            package: "@openai/codex",
            command: "npm install -g @openai/codex@latest".into(),
        }]
    );
    assert_eq!(
        update_command("pi", crate::acp::bindings::pi::PI_PACKAGE, &shim),
        None
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_install_sage_cannot_attribute_gets_no_command() {
    let root = temp_root();
    let executable = root.join("bin/codex");
    write(&executable, "");
    assert_eq!(update_command("codex", "@openai/codex", &executable), None);
    assert_eq!(npm_package("cursor-agent"), None);
    assert!(
        update_sources(&[InstalledHarness {
            id: "codex",
            executable,
            version: Some("0.167.1".into()),
        }])
        .is_empty()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn the_lookup_reads_what_npm_reports_and_nothing_else() {
    let root = temp_root();
    let npm_cli = root.join("lib/node_modules/npm/bin/npm-cli.js");
    std::fs::create_dir_all(npm_cli.parent().unwrap()).unwrap();
    let npm_root = root.join("lib");
    for (printed, expected) in [
        ("0.168.0", Some("0.168.0")),
        ("0.169.0-alpha.2", None),
        ("", None),
    ] {
        write(
            &npm_cli,
            &format!(
                "[ \"$(pwd -P)\" = \"$(cd '{}' && pwd -P)\" ] && printf '%s\\n' '{printed}'\n",
                npm_root.display()
            ),
        );
        assert_eq!(
            latest_version(Path::new("/bin/sh"), &npm_cli, "@openai/codex", &run_lookup).as_deref(),
            expected,
            "{printed}"
        );
    }
    write(&npm_cli, "exit 1\n");
    assert_eq!(
        latest_version(Path::new("/bin/sh"), &npm_cli, "@openai/codex", &run_lookup),
        None
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_runner_sees_no_npm_call_once_checking_is_off() {
    let source = |harness_id, package| UpdateSource {
        harness_id,
        package,
        command: format!("npm install -g {package}@latest"),
    };
    let sources = [
        source("codex", "@openai/codex"),
        source("pi", crate::acp::bindings::pi::PI_PACKAGE),
    ];
    fn npm() -> Option<(PathBuf, PathBuf)> {
        Some((
            PathBuf::from("/runtime/node"),
            PathBuf::from("/runtime/lib/node_modules/npm/bin/npm-cli.js"),
        ))
    }
    let switch = std::cell::Cell::new(false);
    let calls = std::cell::RefCell::new(Vec::new());
    let run = |_: &Path, args: &[&OsStr], _: &Path| {
        calls.borrow_mut().push(
            args.iter()
                .skip(1)
                .take(3)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        );
        Some(b"0.168.0\n".to_vec())
    };
    let enabled = || switch.get();
    assert_eq!(lookup_releases(&sources, &enabled, npm, &run), None);
    assert!(calls.borrow().is_empty());

    switch.set(true);
    let found = lookup_releases(&sources, &enabled, npm, &run).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(calls.borrow()[0], ["view", "@openai/codex", "version"]);
    calls.borrow_mut().clear();

    // Turned off while the first lookup runs: no second npm process starts.
    let run_then_off = |node: &Path, args: &[&OsStr], dir: &Path| {
        switch.set(false);
        run(node, args, dir)
    };
    assert_eq!(
        lookup_releases(&sources, &enabled, npm, &run_then_off),
        None
    );
    assert_eq!(calls.borrow().len(), 1);

    // Turning the switch off and back on during the first lookup must not
    // let that check continue with the next package in the old epoch. The
    // check asks what the production loop asks.
    let catalog = crate::harnesses::HarnessCatalog::default();
    let epoch = catalog.begin_update_check(&["codex", "pi"]).unwrap();
    switch.set(true);
    calls.borrow_mut().clear();
    let run_then_toggle = |node: &Path, args: &[&OsStr], dir: &Path| {
        switch.set(false);
        catalog.forget_published_releases();
        switch.set(true);
        run(node, args, dir)
    };
    let live = catalog.update_check_live(epoch, || switch.get());
    assert_eq!(lookup_releases(&sources, &live, npm, &run_then_toggle), None);
    assert_eq!(calls.borrow().as_slice(), [["view", "@openai/codex", "version"]]);
    let epoch = catalog.begin_update_check(&["codex", "pi"]).unwrap();
    let live = catalog.update_check_live(epoch, || switch.get());
    let found = lookup_releases(&sources, &live, npm, &run).unwrap();
    assert_eq!(
        found,
        [
            PublishedRelease {
                harness_id: "codex",
                version: "0.168.0".into(),
                command: "npm install -g @openai/codex@latest".into()
            },
            PublishedRelease {
                harness_id: "pi",
                version: "0.168.0".into(),
                command: format!(
                    "npm install -g {}@latest",
                    crate::acp::bindings::pi::PI_PACKAGE
                )
            },
        ]
    );
}
