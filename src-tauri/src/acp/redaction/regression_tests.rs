use super::{Mode, redact};

#[test]
fn repeated_invalid_aws_prefixes_have_linear_bounded_work() {
    let input = "0AKIA".repeat(65_536);
    let started = std::time::Instant::now();
    for mode in [Mode::Prose, Mode::CommandOutput, Mode::Diagnostic] {
        assert_eq!(
            redact("test", &input, input.len() * 10, mode).unwrap(),
            input
        );
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
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
