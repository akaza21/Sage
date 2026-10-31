use super::{Mode, credentials::token_scan_bytes, redact};

#[test]
fn home_paths_hide_windows_names_and_preserve_posix_prose() {
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
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
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
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
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
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
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
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
    for mode in [Mode::Prose, Mode::CommandOutput] {
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
                assert!(!actual.contains(name), "{mode:?}: {actual}");
            }
        }
        for home in [r"C:\Users\John", "C:/Users/John", "/c/Users/John"] {
            let input = format!("{home} Smith {} tail", "Name ".repeat(300));
            let actual = redact("test", &input, 8192, mode).unwrap();
            assert!(!actual.contains("Smith"), "{mode:?}: {actual}");
            assert!(actual.ends_with(" tail"), "{mode:?}: {actual}");
            for boundary in [";", "|", "&", "\n", "\r\n"] {
                let input = format!("{home} Doe Smith{boundary} echo public");
                let actual = redact("test", &input, 4096, mode).unwrap();
                assert!(
                    !actual.contains("Doe") && !actual.contains("Smith"),
                    "{mode:?}: {actual}"
                );
                assert!(actual.ends_with("echo public"), "{mode:?}: {actual}");
            }
        }
    }
}

#[test]
fn home_continuations_do_not_capture_unrelated_relative_paths() {
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
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
    const REPEATS: usize = 4_096;
    let scanned = |repeats: usize| {
        let input = "0AKIA".repeat(repeats);
        let before = token_scan_bytes();
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            assert_eq!(
                redact("test", &input, input.len() * 10, mode).unwrap(),
                input
            );
        }
        token_scan_bytes() - before
    };
    let [one, two, three] = [1, 2, 3].map(|factor| scanned(factor * REPEATS));
    // Each further block of rejected AKIA candidates costs the same scan
    // work, so the work grows linearly with the input, not quadratically.
    assert!(one > 0);
    assert_eq!(three - two, two - one);
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
        for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
            let actual = redact("test", input, 512, mode).unwrap();
            assert!(!actual.contains("alice"), "{mode:?}: {actual}");
            assert!(actual.starts_with("before ") && actual.ends_with(" tail"));
        }
    }
}
