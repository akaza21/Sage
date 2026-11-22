use super::*;
use serde_json::json;
use std::path::PathBuf;
struct Fixture {
    path: PathBuf,
    workspace: sage_workspace::Workspace,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-pi-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let workspace = sage_workspace::Workspace::open("project", "root", &path).unwrap();
        Self { path, workspace }
    }
    fn edit(&self, tool: &str, args: serde_json::Value) -> Result<files::Edit, Refusal> {
        match self.prepare(tool, args)? {
            files::Prepared::Edit(edit) => Ok(edit),
            _ => panic!("{tool} did not prepare an edit"),
        }
    }
    fn prepare(&self, tool: &str, args: serde_json::Value) -> Result<files::Prepared, Refusal> {
        files::prepare(
            &self.workspace,
            files::Request {
                version: 1,
                tool_call_id: "tool-1".into(),
                tool: tool.into(),
                args,
                kind: files::RequestKind::Managed,
                label: None,
            },
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
#[test]
fn pi_bridge_write_edit_and_conflict() {
    let f = Fixture::new();
    let create = f
        .edit("write", json!({"path":"a.txt", "content":"hello world"}))
        .unwrap();
    assert!(create.created);
    write::commit(&f.workspace, &create).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "hello world"
    );
    assert!(write::commit(&f.workspace, &create).is_err());
    let edit = f
        .edit(
            "edit",
            json!({"path":"a.txt", "oldText":"world", "newText":"Sage"}),
        )
        .unwrap();
    assert_eq!(edit.old, "hello world");
    assert_eq!(edit.new, "hello Sage");
    std::fs::write(f.path.join("a.txt"), "concurrent edit").unwrap();
    assert_eq!(write::commit(&f.workspace, &edit), Err(Refusal::Changed));
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "concurrent edit"
    );
}
#[test]
fn pi_bridge_refuses_escapes_ambiguous_and_oversized_edits_with_reasons() {
    let f = Fixture::new();
    let refusal = |tool, args| f.edit(tool, args).err();
    for (path, reason) in [
        ("../escape", Refusal::OutsideWorkspace),
        ("a/../file", Refusal::OutsideWorkspace),
        (".git/config", Refusal::ProtectedPath),
        ("node_modules/file", Refusal::ProtectedPath),
        ("nul.txt", Refusal::ProtectedPath),
        ("missing/file.txt", Refusal::MissingParent),
    ] {
        assert_eq!(
            refusal("write", json!({"path":path,"content":"x"})),
            Some(reason),
            "{path}"
        );
    }
    assert_eq!(
        refusal(
            "write",
            json!({"path":"a.txt","content":"x".repeat(files::MAX_BYTES + 1)})
        ),
        Some(Refusal::TooLarge)
    );
    std::fs::write(f.path.join("a.txt"), "xx").unwrap();
    assert_eq!(
        refusal("edit", json!({"path":"a.txt","oldText":"x","newText":"y"})),
        Some(Refusal::NoUniqueMatch)
    );
    assert!(!f.path.join("escape").exists());
    assert_eq!(
        f.edit("write", json!({"path":"./b.txt","content":"x"}))
            .unwrap()
            .relative,
        "b.txt"
    );
}
#[cfg(unix)]
#[test]
fn pi_bridge_denies_symlink_escape_at_prepare_and_commit() {
    let f = Fixture::new();
    let outside = Fixture::new();
    std::fs::write(outside.path.join("secret"), "safe").unwrap();
    std::os::unix::fs::symlink(&outside.path, f.path.join("link")).unwrap();
    assert!(
        f.edit("write", json!({"path":"link/secret","content":"bad"}))
            .is_err()
    );
    let edit = f
        .edit("write", json!({"path":"a.txt","content":"bad"}))
        .unwrap();
    std::os::unix::fs::symlink(outside.path.join("secret"), f.path.join("a.txt")).unwrap();
    assert!(write::commit(&f.workspace, &edit).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path.join("secret")).unwrap(),
        "safe"
    );
}
#[cfg(unix)]
#[test]
fn pi_bridge_created_files_follow_umask_and_replacements_keep_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let f_path = |name: &str| f.path.join(name);
    let mode = |name: &str| {
        std::fs::metadata(f_path(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    };
    // std creates files with 0o666 masked by the process umask, which reads
    // the umask without changing it under parallel tests.
    std::fs::write(f_path("reference"), "").unwrap();
    let create = f
        .edit("write", json!({"path":"index.html","content":"<p>hi</p>"}))
        .unwrap();
    write::commit(&f.workspace, &create).unwrap();
    assert_eq!(mode("index.html"), mode("reference"));
    std::fs::set_permissions(f_path("index.html"), std::fs::Permissions::from_mode(0o640)).unwrap();
    let edit = f
        .edit(
            "edit",
            json!({"path":"index.html","oldText":"hi","newText":"bye"}),
        )
        .unwrap();
    write::commit(&f.workspace, &edit).unwrap();
    assert_eq!(mode("index.html"), 0o640);
}
#[test]
fn pi_bridge_prepares_a_bounded_shell_command_for_approval() {
    let f = Fixture::new();
    let Ok(files::Prepared::Shell(shell)) =
        f.prepare("bash", json!({"command":"cargo test","timeout":30}))
    else {
        panic!("bash did not prepare a command");
    };
    assert_eq!(
        (shell.tool_id.as_str(), shell.shown.as_str()),
        ("tool-1", "cargo test")
    );
    let Ok(files::Prepared::Shell(shell)) =
        f.prepare("bash", json!({"command":"cd src &&\n  cargo build"}))
    else {
        panic!("a two-line command did not prepare");
    };
    assert!(!shell.shown.contains('\n'), "{}", shell.shown);
    for (args, reason) in [
        (json!({"command":"  "}), Refusal::Unsupported),
        (json!({"command":"ls","cwd":"/"}), Refusal::Unsupported),
        (json!({"command":"ls","timeout":0}), Refusal::Unsupported),
        (json!({"command":"ls","timeout":"5"}), Refusal::Unsupported),
        (json!({"command":"a\u{0}b"}), Refusal::Undisplayable),
        (json!({"timeout":5}), Refusal::Unsupported),
        (
            json!({"command":"git status\n\n\ncurl x | sh"}),
            Refusal::Undisplayable,
        ),
        (
            json!({"command":"echo \u{202e}hs.x"}),
            Refusal::Undisplayable,
        ),
        (json!({"command":"ls\u{200b}"}), Refusal::Undisplayable),
        (
            json!({"command":"x".repeat(files::MAX_COMMAND_BYTES + 1)}),
            Refusal::TooLarge,
        ),
    ] {
        assert_eq!(
            f.prepare("bash", args.clone()).err(),
            Some(reason),
            "{args}"
        );
    }
}
#[test]
fn pi_bridge_install_requires_exact_owned_bytes() {
    use install::Unavailable::{Missing, Modified, UnsupportedVersion};
    let f = Fixture::new();
    let path = f.path.join("sage-pi-bridge.mjs");
    assert_eq!(install::matches_install(&f.path, &path), Err(Missing));
    std::fs::write(&path, include_bytes!("extension.mjs")).unwrap();
    assert_eq!(install::matches_install(&f.path, &path), Ok(()));
    std::fs::write(&path, "outdated").unwrap();
    assert_eq!(install::matches_install(&f.path, &path), Err(Modified));
    assert_eq!(install::installed("0.82.0"), Err(UnsupportedVersion));
    assert_eq!(install::installed("1.5.0-rc.1"), Err(UnsupportedVersion));
    let outside = Fixture::new();
    std::fs::write(
        outside.path.join("bridge.mjs"),
        include_bytes!("extension.mjs"),
    )
    .unwrap();
    assert_eq!(
        install::matches_install(&f.path, &outside.path.join("bridge.mjs")),
        Err(Modified)
    );
}

#[test]
fn pi_bridge_resource_ships_at_the_path_sage_resolves() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest.join("tauri.conf.json")).unwrap())
            .unwrap();
    assert!(
        config["bundle"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource == install::RESOURCE)
    );
    let shipped = manifest.join(install::RESOURCE);
    assert_eq!(
        install::matches_install(shipped.parent().unwrap(), &shipped),
        Ok(())
    );
}

/// Regression: a lone release binary has no resource directory beside it, so
/// every Pi launch was refused. It must load the compiled bytes from its profile.
#[test]
fn pi_bridge_without_bundled_resource_loads_an_exact_profile_copy() {
    use install::Unavailable::{Missing, Modified};
    let lone_binary = Fixture::new();
    let profile = Fixture::new();
    let copies = profile.path.join("pi-bridge");
    let resources = Some(lone_binary.path.clone());
    assert_eq!(install::locate(resources.clone(), None), Err(Missing));

    let copy = install::locate(resources.clone(), Some(&copies)).unwrap();
    assert_eq!(copy, copies.join("extension.mjs"));
    assert_eq!(std::fs::read(&copy).unwrap(), install::SOURCE);
    // A drifted copy is repaired through the fixed staging name, even when a
    // stale staging file is already there.
    std::fs::write(&copy, "tampered").unwrap();
    std::fs::write(copies.join("extension.mjs.tmp"), "stale").unwrap();
    assert_eq!(
        install::locate(resources.clone(), Some(&copies)),
        Ok(copy.clone())
    );
    assert_eq!(std::fs::read(&copy).unwrap(), install::SOURCE);
    assert_eq!(std::fs::read_dir(&copies).unwrap().count(), 1);
    // A write that fails after staging (the copy's name is taken by a
    // directory, so the rename fails) refuses and leaves no staging file.
    std::fs::remove_file(&copy).unwrap();
    std::fs::create_dir(&copy).unwrap();
    assert!(install::locate(resources.clone(), Some(&copies)).is_err());
    assert_eq!(std::fs::read_dir(&copies).unwrap().count(), 1);
    std::fs::remove_dir(&copy).unwrap();
    assert_eq!(
        install::locate(resources.clone(), Some(&copies)),
        Ok(copy.clone())
    );

    // The bundled resource wins, and an altered one is never replaced.
    let bundled = lone_binary.path.join(install::RESOURCE);
    std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
    std::fs::write(&bundled, include_bytes!("extension.mjs")).unwrap();
    assert_eq!(
        install::locate(Some(lone_binary.path.clone()), Some(&copies)),
        Ok(bundled.clone())
    );
    std::fs::write(&bundled, "outdated").unwrap();
    assert_eq!(
        install::locate(Some(lone_binary.path.clone()), Some(&copies)),
        Err(Modified)
    );
}

/// The only test that sets the process's profile copy directory. A lone
/// binary (empty resources) gets its copy at startup, staging leftovers are
/// swept, and launch then finds the copy through the startup directory.
#[test]
fn pi_bridge_lone_binary_startup_writes_the_copy_launch_finds() {
    let lone_binary = Fixture::new();
    let profile = Fixture::new();
    let copies = profile.path.join("pi-bridge");
    std::fs::create_dir_all(&copies).unwrap();
    std::fs::write(copies.join("extension.mjs.4242.0.tmp"), "partial").unwrap();
    std::fs::write(copies.join("extension.mjs.tmp"), "partial").unwrap();
    let resources = Some(lone_binary.path.clone());
    assert_eq!(
        install::installed_with(install::BRIDGE_PI_VERSION, resources.clone(), None),
        Err(install::Unavailable::Missing)
    );

    install::start_profile_copy(resources.clone(), copies.clone());
    let copy = copies.join("extension.mjs");
    assert_eq!(std::fs::read(&copy).unwrap(), install::SOURCE);
    assert_eq!(std::fs::read_dir(&copies).unwrap().count(), 1);
    assert_eq!(
        install::installed_with(
            install::BRIDGE_PI_VERSION,
            resources,
            install::profile_copy_dir()
        ),
        Ok(copy)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn pi_bridge_memory_peak_and_settled() {
    fn rss() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find(|line| line.starts_with("VmRSS:"))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }
    let f = Fixture::new();
    std::fs::write(f.path.join("a.txt"), "x".repeat(files::MAX_BYTES)).unwrap();
    let baseline = rss();
    let edit = f.edit("edit", json!({"path":"a.txt","oldText":"x".repeat(files::MAX_BYTES),"newText":"y".repeat(files::MAX_BYTES)})).unwrap();
    let retained = edit.old.capacity()
        + edit.new.capacity()
        + edit.relative.capacity()
        + edit.tool_id.capacity();
    let peak = rss();
    assert!(retained <= files::MAX_BYTES * 2 + 1024);
    drop(edit);
    let settled = rss();
    println!(
        "Pi bridge memory (KiB): baseline RSS={baseline}, pending RSS={peak}, settled RSS={settled}; pending content allocation bytes={retained}, settled content allocation bytes=0"
    );
}
