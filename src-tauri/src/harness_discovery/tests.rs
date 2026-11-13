use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    DiscoveredHarness, DiscoverySource, DisplayVersionCache, HarnessDiscovery, HarnessExecutable,
    HarnessVersion, InstalledHarness, PATH_BYTES_MAX, ProbeLimits, authentic_harness_file,
    discover_from_candidates, enrich_installed_harness_versions, present_candidate,
};
use super::{
    candidates::{candidates_for_test, path_roots_from, scan_candidates_from_roots},
    validation::expected_basenames,
};

#[cfg(windows)]
use super::{InstallLayout, candidates::known_install_roots_from_windows};

fn scratch_dir() -> PathBuf {
    try_scratch_dir().expect("exclusively create scratch directory")
}

fn try_scratch_dir() -> std::io::Result<PathBuf> {
    let nonce = uuid::Uuid::new_v4();
    let directory = std::env::temp_dir().join(format!("sage-host-discovery-{nonce}"));
    fs::create_dir(&directory)?;
    Ok(directory)
}

#[test]
fn parallel_scratch_directories_have_exclusive_ownership() {
    let barrier = std::sync::Barrier::new(2);
    let paths = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    (0..500)
                        .map(|_| {
                            barrier.wait();
                            let path = try_scratch_dir();
                            barrier.wait();
                            path
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    let paths: Vec<_> = paths.into_iter().map(Result::unwrap).collect();
    let unique: std::collections::HashSet<_> = paths.iter().collect();
    for path in &unique {
        fs::remove_dir_all(path).unwrap();
    }
    assert_eq!(
        unique.len(),
        paths.len(),
        "scratch directory ownership collided"
    );
}

fn test_basename(harness: HarnessExecutable) -> &'static str {
    expected_basenames(harness)
        .last()
        .copied()
        .expect("each harness has one expected basename")
}

pub(super) fn write_script(directory: &Path, basename: &str, body: &str) -> PathBuf {
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

fn timeout_limits() -> ProbeLimits {
    ProbeLimits {
        command_deadline: Duration::from_millis(50),
        reap_deadline: Duration::from_millis(100),
        output_bytes_max: 1_024,
    }
}

#[test]
fn presence_projection_does_not_execute_a_version_command() {
    let directory = scratch_dir();
    let marker = directory.join("version-command-executed");
    let executable = write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        &format!("echo executed > \"{}\"", marker.display()),
    );
    let candidate = present_candidate(
        HarnessExecutable::Codex,
        vec![super::candidates::Candidate {
            executable,
            source: DiscoverySource::Path,
        }],
    );

    assert!(candidate.is_some());
    assert!(
        !marker.exists(),
        "presence discovery executed a version probe"
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn a_failed_version_probe_is_tried_again_on_the_next_discovery() {
    let directory = scratch_dir();
    let ready = directory.join("ready");
    // Fails until the marker exists, like a just-updated CLI that misses the
    // deadline on its first, cold run. The script itself never changes.
    let command = if cfg!(windows) {
        format!(
            "@if not exist \"{}\" exit /b 1\r\n@echo codex-cli 0.168.0",
            ready.display()
        )
    } else {
        format!(
            "[ -e \"{}\" ] || exit 1\necho codex-cli 0.168.0",
            ready.display()
        )
    };
    let executable = write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        &command,
    );
    let present = vec![InstalledHarness {
        id: "codex",
        executable,
        version: None,
    }];
    let cache = DisplayVersionCache::default();

    assert_eq!(
        enrich_installed_harness_versions(&present, &cache)[0].version,
        None
    );
    fs::write(&ready, "").unwrap();
    assert_eq!(
        enrich_installed_harness_versions(&present, &cache)[0]
            .version
            .as_deref(),
        Some("0.168.0")
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn display_version_cache_skips_unchanged_probe_and_rechecks_changed_file_identity() {
    let directory = scratch_dir();
    let marker = directory.join("version-probes.txt");
    let separator = if cfg!(windows) { "&" } else { ";" };
    let command = format!(
        "echo codex-cli 0.146.0 {separator} echo probe>>\"{}\"",
        marker.display()
    );
    let executable = write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        &command,
    );
    let present = vec![InstalledHarness {
        id: "codex",
        executable: executable.clone(),
        version: None,
    }];
    let cache = DisplayVersionCache::default();

    let first = enrich_installed_harness_versions(&present, &cache);
    let second = enrich_installed_harness_versions(&present, &cache);
    let comment = if cfg!(windows) { "rem" } else { "#" };
    write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        &format!("{command}\n{comment} changed"),
    );
    let third = enrich_installed_harness_versions(&present, &cache);

    assert_eq!(first[0].version.as_deref(), Some("0.146.0"));
    assert_eq!(second[0].version.as_deref(), Some("0.146.0"));
    assert_eq!(third[0].version.as_deref(), Some("0.146.0"));
    assert_eq!(fs::read_to_string(marker).unwrap().lines().count(), 2);
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn absent_when_no_valid_candidate_exists() {
    let directory = scratch_dir();
    let result = discover_from_candidates(
        HarnessExecutable::Codex,
        candidates(HarnessExecutable::Codex, DiscoverySource::Path, &directory),
        ProbeLimits::PRODUCTION,
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
        ProbeLimits::PRODUCTION,
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
        ProbeLimits::PRODUCTION,
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
        ProbeLimits::PRODUCTION,
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
    let result = discover_from_candidates(
        HarnessExecutable::Codex,
        vec![super::candidates::Candidate {
            executable: executable.clone(),
            source: DiscoverySource::Path,
        }],
        timeout_limits(),
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
        ProbeLimits::PRODUCTION,
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
                ProbeLimits::PRODUCTION,
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
fn path_resolution_reaches_a_valid_seventeenth_root_in_shell_order() {
    let directory = scratch_dir();
    let roots: Vec<PathBuf> = (0..16)
        .map(|index| directory.join(format!("missing-{index}")))
        .chain(std::iter::once(directory.join("agent-bin")))
        .collect();
    let agent_bin = roots.last().expect("agent binary root");
    fs::create_dir_all(agent_bin).expect("create agent binary root");
    let executable = write_script(
        agent_bin,
        test_basename(HarnessExecutable::Codex),
        "echo codex-cli 0.146.0",
    );
    let canonical_executable = executable.canonicalize().expect("canonical executable");
    let path = std::env::join_paths(roots).expect("compose test PATH");
    let bounded = path_roots_from(Some(&path));
    let candidates = scan_candidates_from_roots(
        &[test_basename(HarnessExecutable::Codex)],
        bounded
            .into_iter()
            .map(|root| (DiscoverySource::Path, root)),
    );
    assert_eq!(
        discover_from_candidates(
            HarnessExecutable::Codex,
            candidates,
            ProbeLimits::PRODUCTION
        ),
        HarnessDiscovery::Present(DiscoveredHarness {
            executable: canonical_executable,
            version: HarnessVersion {
                major: 0,
                minor: 146,
                patch: 0,
            },
            source: DiscoverySource::Path,
        })
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[cfg(windows)]
#[test]
fn path_scan_preserves_pathext_order_within_a_root() {
    let directory = scratch_dir();
    let exe = directory.join("codex.exe");
    let command = directory.join("codex.cmd");
    fs::write(&exe, b"native binary placeholder").expect("write executable placeholder");
    fs::write(&command, b"@echo off\r\n").expect("write command placeholder");

    let candidates = scan_candidates_from_roots(
        expected_basenames(HarnessExecutable::Codex),
        [(DiscoverySource::Path, directory.clone())],
    );

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.executable.clone())
            .collect::<Vec<_>>(),
        vec![
            exe.canonicalize().expect("canonical executable"),
            command.canonicalize().expect("canonical command"),
        ]
    );
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn presence_scan_finds_a_sought_basename_in_a_large_directory() {
    let directory = scratch_dir();
    for index in 0..8_192 {
        fs::write(directory.join(format!("unrelated-{index}")), b"placeholder")
            .expect("write unrelated entry");
    }
    let executable = directory.join(test_basename(HarnessExecutable::Codex));
    fs::write(&executable, b"native binary placeholder").expect("write executable placeholder");

    let candidates = scan_candidates_from_roots(
        &[test_basename(HarnessExecutable::Codex)],
        [(DiscoverySource::Path, directory.clone())],
    );

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.executable.clone())
            .collect::<Vec<_>>(),
        vec![executable.canonicalize().expect("canonical executable")]
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
        super::candidates::known_install_roots_from_app_data(Some(app_data.as_os_str()))
            .into_iter()
            .map(|root| (DiscoverySource::KnownInstallRoot, root)),
    );
    let result = discover_from_candidates(
        HarnessExecutable::Claude,
        candidates,
        ProbeLimits::PRODUCTION,
    );
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

#[test]
fn parser_accepts_only_each_harness_documented_version_shape() {
    let stream = |output: &str| super::version_probe::StreamOutput {
        bytes: output.as_bytes().to_vec(),
        truncated: false,
    };
    for (harness, output, expected) in [
        (
            HarnessExecutable::Pi,
            "0.74.2",
            HarnessVersion {
                major: 0,
                minor: 74,
                patch: 2,
            },
        ),
        (
            HarnessExecutable::Gemini,
            "0.1.22",
            HarnessVersion {
                major: 0,
                minor: 1,
                patch: 22,
            },
        ),
        (
            HarnessExecutable::OpenCode,
            "1.18.21",
            HarnessVersion {
                major: 1,
                minor: 18,
                patch: 21,
            },
        ),
        (
            HarnessExecutable::OhMyPi,
            "omp 0.74.2",
            HarnessVersion {
                major: 0,
                minor: 74,
                patch: 2,
            },
        ),
    ] {
        assert_eq!(
            super::parsing::parse_harness_version(harness, &stream(output)),
            Some(expected)
        );
    }
    for (harness, output) in [
        (HarnessExecutable::Pi, "omp 0.74.2"),
        (HarnessExecutable::OhMyPi, "0.74.2"),
    ] {
        assert_eq!(
            super::parsing::parse_harness_version(harness, &stream(output)),
            None
        );
    }
}

#[test]
fn codex_prerelease_display_version_preserves_the_observed_identity() {
    let stream = super::version_probe::StreamOutput {
        bytes: b"codex-cli 0.155.0-alpha.16.3".to_vec(),
        truncated: false,
    };

    assert_eq!(
        super::parsing::parse_harness_display_version(HarnessExecutable::Codex, &stream),
        Some("0.155.0-alpha.16.3".into())
    );
}

#[test]
fn enrichment_preserves_a_codex_prerelease_display_version() {
    let directory = scratch_dir();
    let executable = write_script(
        &directory,
        test_basename(HarnessExecutable::Codex),
        "echo codex-cli 0.155.0-alpha.16.3",
    );
    let cache = DisplayVersionCache::default();
    let enriched = enrich_installed_harness_versions(
        &[InstalledHarness {
            id: "codex",
            executable,
            version: None,
        }],
        &cache,
    );

    assert_eq!(enriched[0].version.as_deref(), Some("0.155.0-alpha.16.3"));
    fs::remove_dir_all(directory).expect("remove scratch directory");
}

#[test]
fn the_installed_cursor_agent_date_build_version_is_read_not_rejected() {
    let stream = |output: &str| super::version_probe::StreamOutput {
        bytes: output.as_bytes().to_vec(),
        truncated: false,
    };
    assert_eq!(
        super::parsing::parse_harness_version(
            HarnessExecutable::CursorAgent,
            &stream("2026.08.11-e8db854")
        ),
        Some(HarnessVersion {
            major: 2026,
            minor: 8,
            patch: 11,
        })
    );
    for rejected in [
        "2026.08.11",
        "2026.08.11-",
        "2026.08.11-e8db85z",
        "2026.08-e8db854",
        "2026.08.11.4-e8db854",
    ] {
        assert_eq!(
            super::parsing::parse_harness_version(
                HarnessExecutable::CursorAgent,
                &stream(rejected)
            ),
            None
        );
    }
}

#[cfg(windows)]
#[test]
fn npm_global_layout_discovers_pi_without_path_injection() {
    let app_data = scratch_dir();
    let npm_root = app_data.join("npm");
    fs::create_dir_all(&npm_root).expect("create npm root");
    let executable = write_script(
        &npm_root,
        test_basename(HarnessExecutable::Pi),
        "echo 0.74.2",
    );
    let roots = known_install_roots_from_windows(
        InstallLayout::NpmGlobal,
        Some(app_data.as_os_str()),
        None,
    );
    let result = discover_from_candidates(
        HarnessExecutable::Pi,
        candidates_for_test(
            HarnessExecutable::Pi,
            roots
                .into_iter()
                .map(|root| (DiscoverySource::KnownInstallRoot, root)),
        ),
        ProbeLimits::PRODUCTION,
    );
    assert_eq!(
        result,
        HarnessDiscovery::Present(DiscoveredHarness {
            executable,
            version: HarnessVersion {
                major: 0,
                minor: 74,
                patch: 2,
            },
            source: DiscoverySource::KnownInstallRoot,
        })
    );
    fs::remove_dir_all(app_data).expect("remove scratch directory");
}
