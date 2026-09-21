use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use super::{
    DiscoveredHarness, DiscoverySource, HarnessDiscovery, HarnessExecutable, HarnessVersion,
    PATH_BYTES_MAX, PATH_DIR_CAP, ProbeLimits, authentic_harness_file, discover_from_candidates,
};
use super::{
    candidates::{bounded_path_roots, candidates_for_test, known_install_roots_from_app_data},
    validation::expected_basenames,
};

fn scratch_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("sage-host-discovery-{nonce}"));
    fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn test_basename(harness: HarnessExecutable) -> &'static str {
    expected_basenames(harness)
        .last()
        .copied()
        .expect("each harness has one expected basename")
}

fn write_script(directory: &Path, basename: &str, body: &str) -> PathBuf {
    let executable = directory.join(basename);
    #[cfg(windows)]
    fs::write(&executable, format!("@echo off\r\n{body}\r\n")).expect("write command script");
    #[cfg(not(windows))]
    {
        fs::write(&executable, format!("#!/bin/sh\n{body}\n")).expect("write shell script");
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).expect("mark script executable");
    }
    executable
}

fn candidates(
    harness: HarnessExecutable,
    source: DiscoverySource,
    root: &Path,
) -> Vec<super::candidates::Candidate> {
    candidates_for_test(harness, [(source, root.to_owned())])
}

fn claude_version_command() -> &'static str {
    #[cfg(windows)]
    {
        "echo 2.1.270 (Claude Code)"
    }
    #[cfg(not(windows))]
    {
        "echo '2.1.270 (Claude Code)'"
    }
}

fn short_limits() -> ProbeLimits {
    ProbeLimits {
        command_deadline: Duration::from_millis(50),
        reap_deadline: Duration::from_millis(100),
        output_bytes_max: 1_024,
    }
}

#[test]
fn absent_when_no_valid_candidate_exists() {
    let directory = scratch_dir();
    let result = discover_from_candidates(
        HarnessExecutable::Codex,
        candidates(HarnessExecutable::Codex, DiscoverySource::Path, &directory),
        short_limits(),
    );
    assert_eq!(result, HarnessDiscovery::Absent);
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn parses_only_the_documented_codex_and_claude_output_shapes() {
    let directory = scratch_dir();
    let codex = write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        "echo codex-cli 0.146.0",
    );
    let codex_result = discover_from_candidates(
        HarnessExecutable::Codex,
        vec![super::candidates::Candidate {
            executable: codex.clone(),
            source: DiscoverySource::Path,
        }],
        short_limits(),
    );
    assert_eq!(
        codex_result,
        HarnessDiscovery::Present(DiscoveredHarness {
            executable: codex,
            version: HarnessVersion {
                major: 0,
                minor: 146,
                patch: 0
            },
            source: DiscoverySource::Path,
        })
    );

    let claude = write_script(
        &directory,
        test_basename(HarnessExecutable::Claude),
        claude_version_command(),
    );
    let claude_result = discover_from_candidates(
        HarnessExecutable::Claude,
        vec![super::candidates::Candidate {
            executable: claude.clone(),
            source: DiscoverySource::Path,
        }],
        short_limits(),
    );
    assert_eq!(
        claude_result,
        HarnessDiscovery::Present(DiscoveredHarness {
            executable: claude,
            version: HarnessVersion {
                major: 2,
                minor: 1,
                patch: 270
            },
            source: DiscoverySource::Path,
        })
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn nonzero_version_command_stays_present_but_unreadable() {
    let directory = scratch_dir();
    #[cfg(windows)]
    let command = "exit /b 7";
    #[cfg(not(windows))]
    let command = "exit 7";
    let executable = write_script(&directory, test_basename(HarnessExecutable::Codex), command);
    let result = discover_from_candidates(
        HarnessExecutable::Codex,
        vec![super::candidates::Candidate {
            executable: executable.clone(),
            source: DiscoverySource::Path,
        }],
        short_limits(),
    );
    assert_eq!(
        result,
        HarnessDiscovery::PresentVersionUnreadable {
            executable,
            source: DiscoverySource::Path,
        }
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn hung_version_command_stays_present_but_unreadable_after_deadline() {
    let directory = scratch_dir();
    #[cfg(windows)]
    let command = "ping 127.0.0.1 -n 6 > nul";
    #[cfg(not(windows))]
    let command = "sleep 1";
    let executable = write_script(&directory, test_basename(HarnessExecutable::Codex), command);
    let started = Instant::now();
    let result = discover_from_candidates(
        HarnessExecutable::Codex,
        vec![super::candidates::Candidate {
            executable: executable.clone(),
            source: DiscoverySource::Path,
        }],
        short_limits(),
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        result,
        HarnessDiscovery::PresentVersionUnreadable {
            executable,
            source: DiscoverySource::Path,
        }
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn unrecognised_output_is_not_mistaken_for_a_version() {
    let directory = scratch_dir();
    let executable = write_script(
        &directory,
        test_basename(HarnessExecutable::Claude),
        "echo warning: 2.1.270 looks plausible",
    );
    let result = discover_from_candidates(
        HarnessExecutable::Claude,
        vec![super::candidates::Candidate {
            executable: executable.clone(),
            source: DiscoverySource::Path,
        }],
        short_limits(),
    );
    assert_eq!(
        result,
        HarnessDiscovery::PresentVersionUnreadable {
            executable,
            source: DiscoverySource::Path,
        }
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn symlink_candidate_is_rejected_when_links_are_available() {
    let directory = scratch_dir();
    let target = write_script(&directory, "target", "echo codex-cli 0.146.0");
    let link = directory.join(test_basename(HarnessExecutable::Codex));
    let created = {
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(&target, &link).is_ok()
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (&target, &link);
            false
        }
    };
    if created {
        assert_eq!(
            discover_from_candidates(
                HarnessExecutable::Codex,
                vec![super::candidates::Candidate {
                    executable: link,
                    source: DiscoverySource::Path,
                }],
                short_limits(),
            ),
            HarnessDiscovery::Absent
        );
    }
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn oversized_candidate_is_rejected_without_execution() {
    let oversized = PathBuf::from(format!(
        "{}{}",
        std::env::temp_dir().display(),
        "a".repeat(PATH_BYTES_MAX)
    ));
    assert!(!authentic_harness_file(
        &oversized,
        test_basename(HarnessExecutable::Codex)
    ));
}

#[test]
fn directory_named_like_harness_is_rejected() {
    let directory = scratch_dir();
    let executable = directory.join(test_basename(HarnessExecutable::Codex));
    fs::create_dir_all(&executable).expect("create named directory");
    assert!(!authentic_harness_file(
        &executable,
        test_basename(HarnessExecutable::Codex)
    ));
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn path_directory_cap_ignores_a_valid_seventeenth_root() {
    let directory = scratch_dir();
    let roots: Vec<PathBuf> = (0..PATH_DIR_CAP)
        .map(|index| directory.join(format!("missing-{index}")))
        .chain(std::iter::once(directory.join("seventeenth")))
        .collect();
    let seventeenth = roots.last().expect("seventeenth root");
    fs::create_dir_all(seventeenth).expect("create seventeenth root");
    let executable = write_script(
        seventeenth,
        test_basename(HarnessExecutable::Codex),
        "echo codex-cli 0.146.0",
    );
    let bounded = bounded_path_roots(roots);
    let candidates = candidates_for_test(
        HarnessExecutable::Codex,
        bounded
            .into_iter()
            .map(|root| (DiscoverySource::Path, root)),
    );
    assert_eq!(
        discover_from_candidates(HarnessExecutable::Codex, candidates, short_limits()),
        HarnessDiscovery::Absent
    );
    assert!(
        executable.exists(),
        "the ignored seventeenth root was genuinely valid"
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[cfg(windows)]
#[test]
fn appdata_npm_root_is_found_without_path_candidates() {
    let app_data = scratch_dir();
    let npm_root = app_data.join("npm");
    fs::create_dir_all(&npm_root).expect("create npm root");
    let executable = write_script(
        &npm_root,
        test_basename(HarnessExecutable::Claude),
        claude_version_command(),
    );
    let candidates = candidates_for_test(
        HarnessExecutable::Claude,
        known_install_roots_from_app_data(Some(app_data.as_os_str()))
            .into_iter()
            .map(|root| (DiscoverySource::KnownInstallRoot, root)),
    );
    let result = discover_from_candidates(HarnessExecutable::Claude, candidates, short_limits());
    assert_eq!(
        result,
        HarnessDiscovery::Present(DiscoveredHarness {
            executable,
            version: HarnessVersion {
                major: 2,
                minor: 1,
                patch: 270
            },
            source: DiscoverySource::KnownInstallRoot,
        })
    );
    fs::remove_dir_all(app_data).expect("remove scratch directory");
}

#[test]
fn parser_rejects_semver_lookalikes_and_unexpected_prose() {
    for output in [
        b"codex-cli 01.2.3".as_slice(),
        b"codex-cli 1.2".as_slice(),
        b"prefix codex-cli 1.2.3".as_slice(),
        b"codex-cli 1.2.3\nextra".as_slice(),
    ] {
        assert!(
            super::parsing::parse_harness_version(
                HarnessExecutable::Codex,
                &super::version_probe::StreamOutput {
                    bytes: output.to_vec(),
                    truncated: false,
                }
            )
            .is_none()
        );
    }
}
