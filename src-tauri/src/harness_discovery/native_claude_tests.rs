use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

use super::{
    DiscoverySource, HarnessDiscovery, HarnessExecutable, InstalledHarness, ProbeLimits,
    candidates, validation,
};

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("sage-native-claude-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join(".local/bin")).unwrap();
        fs::create_dir_all(root.join(".local/share/claude/versions")).unwrap();
        Self(root)
    }

    fn launcher(&self) -> PathBuf {
        self.0.join(".local/bin/claude")
    }

    fn version(&self, version: &str) -> PathBuf {
        let path = self.0.join(".local/share/claude/versions").join(version);
        sage_test_support::test_executable::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' '{version} (Claude Code)'\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_claude_install_is_discovered_and_available_to_sign_in() {
    let profile = Profile::new();
    let binary = profile.version("2.1.319");
    symlink("../share/claude/versions/2.1.319", profile.launcher()).unwrap();
    let launcher = super::native_claude::launcher(&profile.0).unwrap();
    let present = candidates::candidates_from_roots(
        &["claude"],
        [(
            DiscoverySource::KnownInstallRoot,
            launcher.parent().unwrap().to_owned(),
        )],
    );
    assert!(matches!(
        super::discover_from_candidates(
            HarnessExecutable::Claude,
            present,
            ProbeLimits::PRODUCTION
        ),
        HarnessDiscovery::Present(_)
    ));
    let catalog = crate::harnesses::HarnessCatalog::default();
    catalog.apply_presence(vec![InstalledHarness {
        id: "claude-code",
        executable: profile.launcher(),
        version: Some("2.1.319".into()),
    }]);
    assert_eq!(catalog.claude_cli_path(), Some(binary));
}

#[test]
fn native_claude_rejects_link_chains_broken_links_and_escapes() {
    let profile = Profile::new();
    let binary = profile.version("2.1.319");
    let launcher = profile.launcher();
    let codex = launcher.with_file_name("codex");
    symlink(&binary, &codex).unwrap();
    assert!(!validation::authentic_harness_file(&codex, "codex"));

    let outside = profile.0.join("outside");
    fs::write(&outside, "outside").unwrap();
    let chain = binary.with_file_name("2.1.320");
    symlink(&binary, &chain).unwrap();
    for target in [outside, chain, binary.with_file_name("2.1.321")] {
        symlink(target, &launcher).unwrap();
        assert!(!validation::authentic_harness_file(&launcher, "claude"));
        fs::remove_file(&launcher).unwrap();
    }

    fs::write(&launcher, "regular executable").unwrap();
    assert!(validation::authentic_harness_file(&launcher, "claude"));
}

#[test]
fn native_claude_refuses_symlinked_install_store_parents() {
    let profile = Profile::new();
    let binary = profile.version("2.1.319");
    symlink(&binary, profile.launcher()).unwrap();
    let share = profile.0.join(".local/share");
    let moved = profile.0.join("moved-share");
    fs::rename(&share, &moved).unwrap();
    symlink(&moved, &share).unwrap();
    assert!(super::native_claude::launcher(&profile.0).is_none());
}

#[test]
fn native_claude_revalidates_the_link_and_artifact_when_the_cli_updates() {
    use crate::acp::bindings::claude::sdk::ClaudeSdkBindingRegistration;

    let profile = Profile::new();
    symlink(profile.version("2.1.319"), profile.launcher()).unwrap();
    let registration = ClaudeSdkBindingRegistration::startup();
    registration.apply_discovery(Some(&InstalledHarness {
        id: "claude-code",
        executable: profile.launcher(),
        version: Some("2.1.319".into()),
    }));
    let old = registration.admitted().unwrap();
    assert!(old.revalidate().is_ok());
    fs::remove_file(profile.launcher()).unwrap();
    let updated = profile.version("2.1.322");
    symlink(&updated, profile.launcher()).unwrap();
    assert!(old.revalidate().is_err());
    let stale = ClaudeSdkBindingRegistration::startup();
    stale.apply_discovery(Some(&InstalledHarness {
        id: "claude-code",
        executable: profile.launcher(),
        version: Some("2.1.319".into()),
    }));
    assert!(stale.admitted().is_none());

    let new = registration.refresh().unwrap();
    assert_eq!(new.harness_version(), "2.1.322");
    assert_ne!(old.artifact_digest(), new.artifact_digest());
    assert!(new.revalidate().is_ok());

    fs::write(updated, b"changed artifact").unwrap();
    assert!(new.revalidate().is_err());
    fs::remove_file(profile.launcher()).unwrap();
    symlink(profile.version("2.1.299"), profile.launcher()).unwrap();
    assert!(registration.refresh().is_err());
}
