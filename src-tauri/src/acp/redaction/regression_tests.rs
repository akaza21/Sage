use super::{Mode, redact};

#[test]
fn home_paths_hide_windows_names_and_preserve_posix_prose() {
    for mode in [Mode::CommandOutput, Mode::Diagnostic] {
        let marker = if mode == Mode::Diagnostic {
            "[path]"
        } else {
            "[redacted]"
        };
        for input in [
            r"C:\Users\Jane Doe Smith\x tail",
            r"C:\home\Jane Doe Smith\x tail",
            r"C:\Users\Jane  Doe Smith\x tail",
        ] {
            let expected = if mode == Mode::Diagnostic {
                "[path] tail".to_owned()
            } else {
                format!(
                    "{marker}{}{marker} {marker} tail",
                    if input.contains("  ") { "  " } else { " " }
                )
            };
            assert_eq!(
                redact("test", input, 4096, mode).unwrap(),
                expected,
                "{mode:?}"
            );
        }
        let expected = if mode == Mode::Diagnostic {
            marker.to_owned()
        } else {
            format!("{marker} {marker}")
        };
        assert_eq!(
            redact("test", r"C:\Users\John Smith", 4096, mode).unwrap(),
            expected,
            "{mode:?}"
        );
        for ending in [
            "and/or", "and/or,", "and/or.", "AND/OR!", "</div>", "//TODO",
        ] {
            let input = format!("your home /Users/jane is ready {ending}");
            assert_eq!(
                redact("test", &input, 4096, mode).unwrap(),
                format!("your home {marker} is ready {ending}"),
                "{mode:?}"
            );
        }
        for boundary in [";", "|", "&", "\n", "\r\n"] {
            let input = format!(r"C:\Users\Jane Doe still public{boundary} Smith\x tail");
            let actual = redact("test", &input, 4096, mode).unwrap();
            assert!(!actual.contains("Doe"), "{mode:?}: {actual}");
        }
    }
}

#[test]
fn windows_home_names_stay_private_before_continuation_boundaries() {
    for mode in [Mode::CommandOutput, Mode::Diagnostic] {
        for (input, name, public_tail) in [
            (r"C:\Users\Jane Doe copied to /tmp/x tail", "Doe", " tail"),
            (r#"C:\Users\John Smith "D:\out" tail"#, "Smith", " tail"),
            (r"C:\Users\John Smith 'D:\out' tail", "Smith", " tail"),
            (r"C:\Users\John Smith \\server\out tail", "Smith", " tail"),
            (r"C:\Users\John Smith D:\out tail", "Smith", " tail"),
            (
                r"C:\Users\John Smith and/or Bob tail",
                "Smith",
                "and/or Bob tail",
            ),
            (
                r"C:\Users\John Smith AND/OR! Bob tail",
                "Smith",
                "AND/OR! Bob tail",
            ),
            (r"C:\Users\John Smith <none> tail", "Smith", "<none> tail"),
        ] {
            let actual = redact("test", input, 4096, mode).unwrap();
            assert!(!actual.contains(name), "{mode:?}: {actual}");
            assert!(actual.ends_with(public_tail), "{mode:?}: {actual}");
        }
    }
}

#[test]
fn windows_home_first_name_word_accepts_forward_slash_suffixes() {
    for mode in [Mode::CommandOutput, Mode::Diagnostic] {
        for name in ["Smith", "smith"] {
            let input = format!(r"C:\Users\John {name}/Documents tail");
            let expected = if mode == Mode::Diagnostic {
                "[path] tail"
            } else {
                "[redacted] [redacted] tail"
            };
            assert_eq!(
                redact("test", &input, 4096, mode).unwrap(),
                expected,
                "{mode:?}"
            );
        }
        for prefix in ["Smith", "Smith is ready"] {
            for path in ["a/b", "foo/bar"] {
                let input = format!(r"C:\Users\John {prefix} {path} tail");
                let actual = redact("test", &input, 4096, mode).unwrap();
                assert!(!actual.contains("Smith"), "{mode:?}: {actual}");
                assert!(
                    actual.ends_with(&format!("{path} tail")),
                    "{mode:?}: {actual}"
                );
            }
        }
    }
}

#[test]
fn posix_home_paths_redact_only_the_path_token() {
    for mode in [Mode::CommandOutput, Mode::Diagnostic] {
        let marker = if mode == Mode::Diagnostic {
            "[path]"
        } else {
            "[redacted]"
        };
        for (input, tail) in [
            ("/Users/jane is ready", "is ready"),
            ("/home/alice for a/b", "for a/b"),
            ("/Users/Jane Doe Smith/x tail", "Doe Smith/x tail"),
            ("/home/alice Doe Smith/x tail", "Doe Smith/x tail"),
            ("/Users/jane Foo/bar tail", "Foo/bar tail"),
        ] {
            assert_eq!(
                redact("test", input, 4096, mode).unwrap(),
                format!("{marker} {tail}"),
                "{mode:?}"
            );
        }
    }
}

#[test]
fn windows_home_names_remain_private_without_a_path_suffix() {
    let mode = Mode::CommandOutput;
    for input in [
        r"cd C:\Users\John Paul Smith",
        r"C:\Users\Jane Doe Smith",
        r"C:\Users\John Smith is locked",
        r"C:\Users\John van smith",
        "C:/Users/John Smith",
        "C:/Users/John Paul Smith",
        "/c/Users/John Paul Smith",
    ] {
        let actual = redact("test", input, 4096, mode).unwrap();
        for name in ["John", "Paul", "Jane", "Doe", "Smith", "van", "smith"] {
            assert!(!actual.contains(name), "{actual}");
        }
    }
    for home in [r"C:\Users\John", "C:/Users/John", "/c/Users/John"] {
        let input = format!("{home} Smith {} tail", "Name ".repeat(300));
        let actual = redact("test", &input, 8192, mode).unwrap();
        assert!(!actual.contains("Smith"), "{actual}");
        assert!(actual.ends_with(" tail"), "{actual}");
        for boundary in [";", "|", "&", "\n", "\r\n"] {
            let input = format!("{home} Doe Smith{boundary} echo public");
            let actual = redact("test", &input, 4096, mode).unwrap();
            assert!(
                !actual.contains("Doe") && !actual.contains("Smith"),
                "{actual}"
            );
            assert!(actual.ends_with("echo public"), "{actual}");
        }
    }
}

#[test]
fn home_continuations_do_not_capture_unrelated_relative_paths() {
    for mode in [Mode::CommandOutput, Mode::Diagnostic] {
        let marker = if mode == Mode::Diagnostic {
            "[path]"
        } else {
            "[redacted]"
        };
        for path in ["a/b", "foo/bar"] {
            for home in ["/Users/jane", "/Users/Jane Doe"] {
                let input = format!("{home} {path} tail");
                assert_eq!(
                    redact("test", &input, 4096, mode).unwrap(),
                    format!(
                        "{marker}{} {path} tail",
                        if home.ends_with("Doe") { " Doe" } else { "" }
                    ),
                    "{mode:?}"
                );
            }
            let input = format!("your home /Users/jane is ready {path}");
            assert_eq!(
                redact("test", &input, 4096, mode).unwrap(),
                format!("your home {marker} is ready {path}"),
                "{mode:?}"
            );
            let input = format!(r"C:\Users\John Smith {path} tail");
            let actual = redact("test", &input, 4096, mode).unwrap();
            assert!(
                actual.ends_with(&format!("{path} tail")),
                "{mode:?}: {actual}"
            );
        }
    }
}

#[test]
fn token_key_exclusions_preserve_metrics_without_hiding_credentials() {
    for mode in [
        Mode::Prose,
        Mode::CommandOutput,
        Mode::Permission,
        Mode::Diagnostic,
    ] {
        for input in [
            "token_string=token-canary",
            "TOKEN_SIGNATURE=token-canary",
            "to_ken=token-canary",
            "t_o_k_e_n=token-canary",
        ] {
            let actual = redact("test", input, 4096, mode).unwrap();
            assert!(!actual.contains("token-canary"), "{mode:?}: {actual}");
        }
        for input in [
            "token_count=5",
            "token-count=5",
            "token_counts=5",
            "token_count_total=5",
            "tokenization:",
            "detokenize=",
        ] {
            assert_eq!(
                redact("test", input, 4096, mode).unwrap(),
                input,
                "{mode:?}"
            );
        }
    }
}

#[test]
fn diagnostic_home_paths_keep_adjacent_command_operators() {
    for operator in [";", "|", "&"] {
        let input = format!(r"C:\Users\Jane Doe Smith\x{operator}echo ok");
        assert_eq!(
            redact("test", &input, 4096, Mode::Diagnostic).unwrap(),
            format!("[path]{operator}echo ok")
        );
    }
}

#[test]
fn repeated_invalid_aws_prefixes_have_linear_bounded_work() {
    use super::tokens::{AWS_KEY_TAIL_SCAN, work};
    // Count the scanner's work instead of timing it: every byte position is
    // visited once and each AWS prefix inspects a bounded tail, so the
    // work is linear in the input whatever the runner's load.
    for repeats in [16_384, 65_536] {
        let input = "0AKIA".repeat(repeats);
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            work::take();
            assert_eq!(
                redact("test", &input, input.len() * 10, mode).unwrap(),
                input,
                "{mode:?}"
            );
            let observed = work::take();
            assert_eq!(observed.positions, input.len(), "{mode:?}");
            assert_eq!(observed.aws_prefixes, repeats, "{mode:?}");
            assert!(
                observed.aws_tail_bytes <= observed.aws_prefixes * AWS_KEY_TAIL_SCAN,
                "{mode:?}: {observed:?}"
            );
        }
    }
}

#[test]
fn all_quote_forms_share_the_newline_bound() {
    for marker in ["KEY='", "KEY=prefix'", "--token '"] {
        for (newlines, tail_kept) in [(63, true), (64, false)] {
            let input = format!("before {marker}{}body' tail", "secret\n".repeat(newlines));
            for mode in [Mode::Prose, Mode::CommandOutput] {
                let actual = redact("test", &input, 65_536, mode).unwrap();
                assert!(!actual.contains("body"), "{marker}: {mode:?}: {actual}");
                assert_eq!(
                    actual.ends_with(" tail"),
                    tail_kept,
                    "{marker}: {mode:?}: {actual}"
                );
                assert_eq!(
                    actual.contains("[secret limit]"),
                    !tail_kept,
                    "{marker}: {mode:?}: {actual}"
                );
            }
        }
    }
}

#[test]
fn structured_values_share_the_newline_bound() {
    for newlines in [63, 64] {
        let input = format!(
            "before password={{\"a\":\"{}body\",\"c\":\"hidden\"}} tail",
            "secret\n".repeat(newlines)
        );
        for mode in [Mode::Prose, Mode::CommandOutput] {
            let actual = redact("test", &input, 65_536, mode).unwrap();
            assert!(!actual.contains("body") && !actual.contains("hidden"));
            assert_eq!(
                actual.ends_with(" tail"),
                newlines == 63,
                "{mode:?}: {actual}"
            );
        }
    }
}

#[test]
fn repeated_unclosed_fragments_have_linear_bounded_work() {
    for repetitions in [4096, 65_536] {
        let input = format!("before {}canary", "password=unclosed'".repeat(repetitions));
        let started = std::time::Instant::now();
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let actual = redact("test", &input, input.len().saturating_mul(10), mode).unwrap();
            assert!(actual.starts_with("before "));
            assert!(!actual.contains("canary"));
            assert!(actual.contains("[redacted]"));
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}

#[test]
fn permission_paths_are_rejected_even_inside_a_credential_value() {
    for input in [
        "password='/home/alice/x'",
        r"password='C:\Users\Jane\inc'",
        r#"password="C:\Users\Jane\inc""#,
        r"cc -isystemC:\Users\Jane\inc",
        "C:/Users/Jane Doe/x",
        "/c/Users/Jane Doe/x",
        ">/home/alice/x",
    ] {
        assert!(
            redact("test", input, 512, Mode::Permission).is_err(),
            "{input}"
        );
        assert!(
            crate::acp::bindings::command_summary::sanitize_command_summary(input).is_err(),
            "{input}"
        );
    }
}

#[test]
fn escaped_web_urls_preserve_prose_without_weakening_native_path_detection() {
    let input = r"before https:\/\/example.test\/about tail";
    for mode in [
        Mode::Prose,
        Mode::CommandOutput,
        Mode::Permission,
        Mode::Diagnostic,
    ] {
        let actual = redact("test", input, 512, mode).unwrap();
        assert_eq!(actual, input, "{mode:?}");
    }
    for input in [
        r"before \/home\/alice\/x tail",
        r"before file:\/\/\/home\/alice\/x tail",
    ] {
        assert!(redact("test", input, 512, Mode::Permission).is_err());
        for mode in [Mode::CommandOutput, Mode::Diagnostic] {
            let actual = redact("test", input, 512, mode).unwrap();
            assert!(!actual.contains("alice"), "{mode:?}: {actual}");
            assert!(actual.starts_with("before ") && actual.ends_with(" tail"));
        }
    }
}

#[test]
fn prose_shows_absolute_paths_as_written() {
    for input in [
        r"cargo build --release is compiling now into D:\SageWork\main-build\target which is still empty",
        "logs are in /var/log/sage/run.log and /srv/data.",
        "open `/opt/sage/bin` or (E:/builds/out)",
        r"the share \\buildhost\drops\nightly is mounted",
        "file:///srv/data/report.html",
    ] {
        assert_eq!(redact("text", input, 4096, Mode::Prose).unwrap(), input);
    }
}

#[test]
fn prose_shortens_the_current_home_folder() {
    let home = std::env::home_dir().expect("test machines have a home folder");
    let input = format!(
        "edited {} just now",
        home.join("proj").join("x.rs").display()
    );
    let separator = std::path::MAIN_SEPARATOR;
    assert_eq!(
        redact("text", &input, 4096, Mode::Prose).unwrap(),
        format!("edited ~{separator}proj{separator}x.rs just now")
    );
    assert_eq!(
        redact("toolOutput", &input, 4096, Mode::CommandOutput).unwrap(),
        "edited [redacted] just now"
    );
}

#[test]
fn prose_paths_never_carry_secrets_into_view() {
    for (input, expected) in [
        (
            r"wrote D:\work\app\.env with API_KEY='sk-live-abcdef' for you",
            r"wrote D:\work\app\.env with [redacted] for you",
        ),
        (
            "use /srv/app/run.sh --token abc123 now",
            "use /srv/app/run.sh --token [redacted] now",
        ),
        (
            "cloned into /srv/git/repo from https://alice:hunter2@example.test/repo.git",
            "cloned into /srv/git/repo from [redacted]",
        ),
        (
            "cache at /tmp/ghp_abcdef123456/data is stale",
            "cache at [redacted] is stale",
        ),
        (
            r"key in C:\keys\AKIAIOSFODNN7EXAMPLE\creds was rotated",
            "key in [redacted] was rotated",
        ),
        (
            "file:///srv/sk-abcdef123456/x and file://alice:hunter2@host/x",
            "[redacted] and [redacted]",
        ),
    ] {
        assert_eq!(
            redact("text", input, 4096, Mode::Prose).unwrap(),
            expected,
            "{input}"
        );
    }
}

#[test]
fn secret_keys_after_a_backslash_are_redacted_in_every_mode() {
    for key in [
        r"C:\keys\token=",
        r"C:\keys\token:",
        r"/srv/x\token=",
        r"\\host\share\token=",
        r"foo\token=",
        r"C:\app\tmp\password=",
        r"C:\app\new\api_key:",
    ] {
        let input = format!("see {key}abc123secret ok");
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let shown = redact("text", &input, 4096, mode).unwrap();
            assert!(!shown.contains("abc123secret"), "{mode:?}: {shown}");
            assert!(
                shown.starts_with("see ") && shown.ends_with("] ok"),
                "{mode:?}: {shown}"
            );
        }
    }
}

#[test]
fn provider_tokens_signed_urls_and_path_credentials_are_redacted() {
    // Built at run time so the fakes do not trip repository secret scanning.
    let gitlab = format!("glpat-{}", "xR4tQ9vLm2".repeat(2));
    let github = format!(
        "github_pat_{}_{}",
        "11AAAAAAA0abcdefghijkl",
        "MNOPQRSTUVWXYZ0123456789".repeat(2)
    );
    let google = format!("AIza{}", "Sy0_-9xQ".repeat(5));
    let npm = format!("npm_{}", "a1B2c3D4e5F6".repeat(3));
    for secret in [
        gitlab.as_str(),
        github.as_str(),
        google.as_str(),
        npm.as_str(),
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSJ9.c2lnbmF0dXJlLWZha2UtMTIz",
        "https://store.example.test/x.zip?sv=2024-01-01&sig=Zm9vYmFyc2lnbmF0dXJl",
        "https://bucket.s3.example.test/x?X-Amz-Expires=60&X-Amz-Signature=0a1b2c3d4e5f",
        "https://storage.example.test/x?X-Goog-Signature=0a1b2c3d4e5f&X-Goog-Expires=60",
        "/srv/u:hunter2@db/x",
    ] {
        for wrapped in [
            format!("found {secret} here"),
            format!("cache at /srv/cache/{secret}/data here"),
        ] {
            for mode in [Mode::Prose, Mode::CommandOutput] {
                let shown = redact("text", &wrapped, 4096, mode).unwrap();
                assert!(shown.contains("[redacted]"), "{mode:?}: {shown}");
                assert!(shown.ends_with(" here"), "{mode:?}: {shown}");
                for part in [
                    "xR4tQ9", "MNOPQRS", "Sy0_-9xQ", "a1B2c3", "c2lnbmF0", "Zm9vYmFy", "0a1b2c3d",
                    "hunter2",
                ] {
                    assert!(!shown.contains(part), "{mode:?}: {shown}");
                }
            }
        }
    }
}

#[test]
fn token_shaped_words_without_secrets_stay_in_prose() {
    for input in [
        "run npm_install, glpat-short, github_pat_ docs and AIza keys",
        "the eyJ prefix and eyJhbGciOiJIUzI1NiJ9 alone are headers",
        "pin nginx:1.25@sha256:0a1b2c3d4e5f and mail mailto:alice@example.test",
        "push to git@github.com:org/repo.git as alice@example.test at 10:30",
        "the design=final signature=pending Sig: done",
        r"C:\keys\keyboard=US and /srv/notes:today",
        "meet at 12:00@noon, ratio 1:2@3x, HH:MM@UTC",
        "ping bob:x@y.com and pull img:tag@sha512:zz",
    ] {
        assert_eq!(redact("text", input, 4096, Mode::Prose).unwrap(), input);
    }
    for input in [
        "meet at 12:00@noon, ratio 1:2@3x, HH:MM@UTC",
        "ping bob:x@y.com and pull img:tag@sha512:zz",
    ] {
        assert_eq!(
            redact("text", input, 4096, Mode::CommandOutput).unwrap(),
            input
        );
    }
}

#[test]
fn source_locations_with_secret_like_file_names_stay_visible() {
    for input in [
        "see src/auth/token.rs:42 and src-tauri/src/acp/redaction/secrets.rs:66",
        "[x](~/proj/src/token.rs:12) and credentials.rs:151.",
        "error at token.rs:42:7, password.py:10-20 and secrets.rs:3:1-9",
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput] {
            assert_eq!(
                redact("text", input, 4096, mode).unwrap(),
                input,
                "{mode:?}"
            );
        }
    }
    for (input, secret) in [
        ("see token.rs:ghp_abc123def456 now", "ghp_abc123def456"),
        ("see password.txt:hunter2 now", "hunter2"),
        ("see secrets.rs:66x now", "66x"),
        ("see token.rs:42:abc now", "abc"),
        ("see src/token:42 now", "42"),
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput] {
            let shown = redact("text", input, 4096, mode).unwrap();
            assert_eq!(shown, "see [redacted] now", "{mode:?}: {input}");
            assert!(!shown.contains(secret));
        }
    }
}

#[test]
fn tool_prose_keeps_paths_private() {
    let input = r"read D:\SageWork\main-build\target and /var/log/sage/run.log";
    assert_eq!(
        super::redact_prose_hiding_paths("toolOutput", input, 4096).unwrap(),
        "read [redacted] and [redacted]"
    );
}

#[test]
fn numeric_values_of_dotted_secret_keys_are_redacted() {
    for key in [
        "db.password",
        "auth.token",
        "app.secret",
        "db.passwd",
        "aws.secretKey",
        "server.key",
        "user.session",
        "src/auth.token",
    ] {
        for value in ["123456", "42:7", "1-2", "9."] {
            let input = format!("set {key}:{value} now");
            for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
                let shown = redact("text", &input, 4096, mode).unwrap();
                assert!(!shown.contains(value), "{mode:?}: {shown}");
                assert!(shown.starts_with("set ") && shown.ends_with(" now"));
            }
        }
    }
}

#[test]
fn line_references_to_files_with_secret_like_names_stay_visible() {
    // A known file extension names a file type rather than a field, so a
    // sensitive word in the file name does not hide a line reference to it.
    // Anything other than a line, column or range that fits a line index is
    // still judged as a secret.
    for input in [
        "see password.txt:1234 now",
        "see secrets.yaml:4294967295 now",
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput] {
            assert_eq!(
                redact("text", input, 4096, mode).unwrap(),
                input,
                "{mode:?}"
            );
        }
    }
    for (input, secret) in [
        ("see password.txt:4294967296 now", "4294967296"),
        ("see password.txt:1234abcd now", "1234abcd"),
        ("see password.txt:12:34:56 now", "12:34:56"),
        ("see password.txt:1-2-3 now", "1-2-3"),
        ("see password.txt:12:hunter2 now", "hunter2"),
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput] {
            let shown = redact("text", input, 4096, mode).unwrap();
            assert_eq!(shown, "see [redacted] now", "{mode:?}: {input}");
            assert!(!shown.contains(secret));
        }
    }
}

#[test]
fn source_extensions_never_name_a_credential_field() {
    for extension in super::locations::SOURCE_EXTENSIONS {
        assert!(!super::credentials::sensitive_key(extension), "{extension}");
    }
}

#[test]
fn colon_chains_are_scanned_in_linear_work() {
    use super::tokens::work;
    for unit in ["a:", "x.rs:1:", "x.rs:1-", "x.rs:1.", "a.rs:", "1:"] {
        let input = unit.repeat(16_384);
        for mode in [Mode::Prose, Mode::CommandOutput] {
            work::take();
            redact("test", &input, input.len() * 10, mode).unwrap();
            let observed = work::take();
            assert!(
                observed.reference_bytes <= input.len(),
                "{unit} {mode:?}: {observed:?}"
            );
        }
    }
}

#[test]
fn jwt_header_runs_are_measured_once() {
    use super::tokens::work;
    let input = "eyJ0".repeat(65_536);
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
        work::take();
        assert_eq!(
            redact("test", &input, input.len() * 10, mode).unwrap(),
            input,
            "{mode:?}"
        );
        let observed = work::take();
        assert!(
            observed.jwt_run_bytes <= input.len(),
            "{mode:?}: {observed:?}"
        );
    }
    let token = format!("eyJ{}.eyJzdWIiOiJhbGljZSJ9.c2ln", "a".repeat(4096));
    let shown = redact("text", &format!("found {token} here"), 16_384, Mode::Prose).unwrap();
    assert_eq!(shown, "found [redacted] here");
}

#[test]
fn every_credential_in_a_path_stays_hidden_in_diagnostics_and_replies() {
    let passwords = ["hunter2", "secondpass", "thirdpass"];
    for input in [
        "/srv/alice:hunter2@db/bob:secondpass@host/x",
        "/srv/alice:hunter2@db/bob:secondpass@host/carol:thirdpass@h/x",
        "srv/alice:hunter2@db/bob:secondpass@host/x",
        r"C:\srv\alice:hunter2@db\bob:secondpass@host\x",
        "https://alice:hunter2@db.test/bob:secondpass@host/x",
        "https://db.test/alice:hunter2@db/bob:secondpass@host/x",
    ] {
        let wrapped = format!("see {input} here");
        let diagnostic = crate::acp::redaction::sanitize_diagnostic_text(&wrapped);
        for mode in [Mode::Diagnostic, Mode::Prose, Mode::CommandOutput] {
            let shown = redact("test", &wrapped, 4096, mode).unwrap();
            for shown in [&shown, &diagnostic] {
                assert!(shown.starts_with("see "), "{mode:?}: {shown}");
                assert!(shown.ends_with(" here"), "{mode:?}: {shown}");
                for password in passwords {
                    assert!(!shown.contains(password), "{mode:?}: {shown}");
                }
            }
        }
    }
    assert_eq!(
        crate::acp::redaction::sanitize_diagnostic_text(
            "at /srv/alice:hunter2@db/bob:secondpass@host/x"
        ),
        "at [path]"
    );
    assert_eq!(
        crate::acp::redaction::sanitize_diagnostic_text("at https://alice:hunter2@db.test/x"),
        "at https://[redacted]@db.test/x"
    );
}

#[test]
fn a_path_after_url_userinfo_stays_hidden_in_diagnostics() {
    for input in ["file://u@/home/alice/x", "file://u:pw@/home/alice/x"] {
        let shown = crate::acp::redaction::sanitize_diagnostic_text(&format!("open {input} now"));
        assert!(shown.starts_with("open "), "{shown}");
        assert!(shown.ends_with(" now"), "{shown}");
        assert!(!shown.contains("alice"), "{shown}");
        assert!(!shown.contains("pw"), "{shown}");
    }
}

#[test]
fn secrets_after_punctuation_outside_ascii_are_redacted_in_replies() {
    for (input, expected) in [
        ("回答，--password X", "回答，--password [redacted]"),
        (
            "回答，--password 'X Y' 完",
            "回答，--password [redacted] 完",
        ),
        ("回答，password=X 完", "回答，[redacted] 完"),
        ("回答（Bearer X）", "回答（Bearer [redacted]"),
        ("«--password X» done", "«--password [redacted] done"),
        ("it’s fine, don’t worry", "it’s fine, don’t worry"),
    ] {
        assert_eq!(super::reply_text(input), expected, "{input}");
    }
}

#[test]
fn a_credential_written_straight_after_cjk_or_full_width_punctuation_is_hidden() {
    let secret = "2ZvYrB9g2WxubqFL4aMA";
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
        for lead in [
            "回答，",
            "（",
            "「",
            "：",
            "。",
            "；",
            "\u{3000}",
            "\u{a0}",
            "—",
            "“",
        ] {
            for form in [
                format!("--password {secret} y"),
                format!("-p {secret} y"),
                format!("--password=\"{secret} z\" y"),
                format!("Authorization: Bearer {secret}\ny"),
                format!("Bearer {secret} y"),
                format!("token={secret} y"),
                format!("postgres://admin:{secret}@db/app y"),
            ] {
                let text = format!("x {lead}{form}");
                let shown = redact("test", &text, 4096, mode).unwrap();
                assert!(!shown.contains(secret), "{mode:?}: {text:?} => {shown:?}");
                assert!(
                    shown.contains(lead.trim()),
                    "{mode:?}: {text:?} => {shown:?}"
                );
            }
        }
    }
}

#[test]
fn punctuation_outside_ascii_inside_a_path_keeps_the_whole_path_hidden() {
    for path in [
        "/home/o’brien/notes.txt",
        r"C:\Users\O’Brien\notes.txt",
        "/srv/报告，最终版/x.txt",
    ] {
        let text = format!("see {path} here");
        for mode in [Mode::CommandOutput, Mode::Diagnostic] {
            let shown = redact("test", &text, 4096, mode).unwrap();
            assert!(!shown.contains("rien"), "{mode:?}: {shown}");
            assert!(!shown.contains("最终版"), "{mode:?}: {shown}");
        }
        let shown = super::redact_prose_hiding_paths("test", &text, 4096).unwrap();
        assert!(
            !shown.contains("rien") && !shown.contains("最终版"),
            "{shown}"
        );
    }
}

#[test]
fn a_secret_value_is_never_cut_at_punctuation_or_a_change_of_script() {
    let secret = "hunter2Zq";
    for value in [
        format!("password=abc—{secret} y"),
        format!("password=密码{secret} y"),
        format!("https://alice:pw—{secret}@db/app y"),
        format!("postgres://admin:密码—{secret}@db/app y"),
        format!("--password 密码{secret} y"),
        format!("--password abc，{secret} y"),
        format!("Bearer 密码—{secret} y"),
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let shown = redact("test", &format!("x {value}"), 4096, mode).unwrap();
            assert!(!shown.contains(secret), "{mode:?}: {value} => {shown}");
        }
        let shown = super::reply_text(&value);
        assert!(!shown.contains(secret), "{value} => {shown}");
    }
}

#[test]
fn a_key_or_flag_written_straight_after_an_ideograph_starts_a_token() {
    for (input, expected) in [
        (
            "回答Authorization: Bearer X\n完",
            "回答Authorization: [redacted] [redacted]\n完",
        ),
        ("请用--password X登录", "请用--password [redacted]"),
        ("パスワードはtoken=X です", "パスワードは[redacted] です"),
        ("答案是Python的写法", "答案是Python的写法"),
    ] {
        assert_eq!(super::reply_text(input), expected, "{input}");
    }
}

#[test]
fn a_sensitive_key_holding_punctuation_outside_ascii_stays_whole() {
    for input in [
        r#"{"password—backup":"hunter2Zq"}"#,
        "password—backup=hunter2Zq",
        "password—backup = hunter2Zq",
        "--password—backup hunter2Zq",
        "回答，password’s=hunter2Zq",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(
        // Keys running past the bytes read ahead for a key, crossing them at
        // every byte around the boundary.
        (240..270).flat_map(|filler| {
            let long = "a".repeat(filler);
            [
                format!("{{\"password{long}—backup\":\"hunter2Zq\"}}"),
                format!("password{long}—backup:hunter2Zq"),
                format!("password{long}—backup=hunter2Zq"),
                format!("--password{long}—backup hunter2Zq"),
                format!("{{\"{long}—password\":\"hunter2Zq\"}}"),
                format!("回答，api_key{}，backup=hunter2Zq", "密".repeat(filler / 3)),
            ]
        }),
    ) {
        let input = input.as_str();
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let shown = redact("test", input, 4096, mode).unwrap();
            assert!(!shown.contains("hunter2Zq"), "{mode:?}: {input} => {shown}");
        }
        assert!(!super::reply_text(input).contains("hunter2Zq"), "{input}");
    }
}

#[test]
fn a_long_run_without_spaces_still_starts_tokens_at_punctuation_and_scripts() {
    let lead = "回答".repeat(200);
    for input in [
        format!("{lead}，--password hunter2Zq"),
        format!("{lead}请用--password hunter2Zq"),
        format!("{lead}Authorization: Bearer hunter2Zq"),
    ] {
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let shown = redact("test", &input, 4096, mode).unwrap();
            assert!(!shown.contains("hunter2Zq"), "{mode:?}: {input} => {shown}");
        }
        assert!(!super::reply_text(&input).contains("hunter2Zq"), "{input}");
    }
}

/// Everything redacting with tokens split at whitespace alone hides stays
/// hidden now that punctuation and scripts may start a token too.
#[test]
fn starting_tokens_at_punctuation_or_a_script_hides_all_that_spaces_alone_hide() {
    const KEYS: [&str; 8] = [
        "password",
        "api_key",
        "token",
        "secret",
        "passwd",
        "auth_token",
        "clientSecret",
        "Authorization",
    ];
    const MARKS: [&str; 10] = ["", "—", "，", "’", "：", "「", "」", "·", "、", "-"];
    const LEADS: [&str; 5] = ["", "回答", "回答，", "x ", "パス"];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut below = |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    for case in 0..4000 {
        let mark = MARKS[below(MARKS.len())];
        let base = KEYS[below(KEYS.len())];
        // A long key runs past the bytes read ahead for a key.
        let filler = "a".repeat(230 + below(50));
        let key = match below(6) {
            0 => base.to_owned(),
            1 => format!("{base}{mark}backup"),
            2 => format!("old{mark}{base}"),
            3 => format!("{base}{filler}{mark}backup"),
            4 => format!("{filler}{mark}{base}"),
            _ => format!("{mark}{base}{mark}"),
        };
        let secret = format!("s3cr3t{case}Zq");
        let form = match below(7) {
            0 => format!("{key}={secret}"),
            1 => format!("{key}: {secret}"),
            2 => format!("\"{key}\":\"{secret}\""),
            3 => format!("{{\"{key}\":\"{secret}\"}}"),
            4 => format!("--{key} {secret}"),
            5 => format!("{key} = {secret}"),
            _ => format!("{key}{mark}{secret}"),
        };
        let input = format!("{}{form} done", LEADS[below(LEADS.len())]);
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let before = super::with_spaces_only(|| redact("test", &input, 4096, mode).unwrap());
            let now = redact("test", &input, 4096, mode).unwrap();
            assert!(
                before.contains(&secret) || !now.contains(&secret),
                "{mode:?}: {input} => {now} (spaces alone: {before})"
            );
        }
        let before = super::with_spaces_only(|| super::reply_text(&input));
        let now = super::reply_text(&input);
        assert!(
            before.contains(&secret) || !now.contains(&secret),
            "{input} => {now} (spaces alone: {before})"
        );
    }
}
