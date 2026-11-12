use super::*;

fn temp_root() -> PathBuf {
    let root = crate::test_paths::temp_dir().join(format!("sage-updates-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn only_a_final_release_on_the_latest_tag_counts() {
    assert_eq!(
        parse_published_version(b"0.168.0\n"),
        Some("0.168.0".into())
    );
    for refused in [&b"1.0.0-beta.1\n"[..], b"", b"npm warn config\n0.1"] {
        assert_eq!(parse_published_version(refused), None);
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
        update_command("codex", "@openai/codex", &shim).as_deref(),
        Some("npm install -g @openai/codex@latest")
    );
    // Another package's shim in the same folder is not attributed to it.
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
        published_releases(&[InstalledHarness {
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
fn a_prefix_bin_link_into_its_package_updates_through_npm() {
    let root = temp_root();
    let package = root.join("lib/node_modules/@openai/codex");
    write(&package.join("package.json"), "{}");
    write(&package.join("bin/codex.js"), "");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let link = root.join("bin/codex");
    std::os::unix::fs::symlink(package.join("bin/codex.js"), &link).unwrap();
    assert!(npm_owned(&link, "@openai/codex"));

    // A link elsewhere (a Homebrew cask) is not the npm copy beside it.
    let cask = root.join("Caskroom/codex/codex");
    write(&cask, "");
    let other = root.join("bin/codex-cask");
    std::os::unix::fs::symlink(&cask, &other).unwrap();
    assert!(!npm_owned(&other, "@openai/codex"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn the_lookup_reads_what_npm_reports_and_nothing_else() {
    let root = temp_root();
    let npm_cli = root.join("npm-cli.js");
    for (printed, expected) in [
        ("0.168.0", Some("0.168.0")),
        ("0.169.0-alpha.2", None),
        ("", None),
    ] {
        write(&npm_cli, &format!("printf '%s\\n' '{printed}'\n"));
        assert_eq!(
            latest_version(Path::new("/bin/sh"), &npm_cli, "@openai/codex").as_deref(),
            expected,
            "{printed}"
        );
    }
    write(&npm_cli, "exit 1\n");
    assert_eq!(
        latest_version(Path::new("/bin/sh"), &npm_cli, "@openai/codex"),
        None
    );
    std::fs::remove_dir_all(root).unwrap();
}
