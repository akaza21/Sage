use super::sanitize_diagnostic_text;

#[test]
fn oversized_private_key_pem_headers_fail_closed() {
    for prefix in ["A".repeat(1024), "\u{754c}".repeat(400)] {
        let label = format!("{prefix} PRIVATE KEY");
        let input =
            format!("before -----BEGIN {label}-----\npem-body-canary\n-----END {label}----- after");
        let actual = sanitize_diagnostic_text(&input);
        assert_eq!(actual, "before [redacted]");
        assert_eq!(sanitize_diagnostic_text(&actual), actual);
    }
}

#[test]
fn private_key_pem_variants_require_matching_bounded_end_markers() {
    for label in [
        "PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "EC PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
    ] {
        let begin = format!("-----BEGIN {label}-----\npem-body-canary\n");
        for (body, keeps_tail) in [
            (format!("{begin}-----END {label}-----"), true),
            (format!("{begin}-----END RSA PRIVATE KEY-----"), false),
            (
                format!("{begin}{}-----END {label}-----", "x".repeat(2048)),
                false,
            ),
            (begin, false),
        ] {
            let actual = sanitize_diagnostic_text(&format!("before {body} after"));
            assert_eq!(
                actual,
                if keeps_tail {
                    "before [redacted] after"
                } else {
                    "before [redacted]"
                },
                "{label}"
            );
            assert_eq!(sanitize_diagnostic_text(&actual), actual);
        }
    }
}

#[test]
fn rsa_private_key_pem_is_redacted_with_bounded_fail_closed_scanning() {
    let complete =
        "-----BEGIN RSA PRIVATE KEY-----\npem-body-canary\n-----END RSA PRIVATE KEY-----";
    let unterminated = "-----BEGIN RSA PRIVATE KEY-----\npem-body-canary";
    let over_budget = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}pem-body-canary\n-----END RSA PRIVATE KEY-----",
        "x".repeat(2048)
    );
    let unicode = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}pem-body-canary\n-----END RSA PRIVATE KEY-----",
        "\u{754c}".repeat(400)
    );
    let repeated = format!("{complete}\n{complete}");
    for (input, keeps_tail) in [
        (complete, true),
        (unterminated, false),
        (over_budget.as_str(), false),
        (unicode.as_str(), false),
        (repeated.as_str(), true),
    ] {
        let actual = sanitize_diagnostic_text(&format!("before {input} after"));
        assert!(
            !actual.contains("pem-body-canary"),
            "leaked PEM: {actual:?}"
        );
        assert!(!actual.contains('x') && !actual.contains('\u{754c}'));
        assert!(
            actual.starts_with("before [redacted]"),
            "missing marker: {actual:?}"
        );
        assert_eq!(actual.ends_with(" after"), keeps_tail);
        assert_eq!(sanitize_diagnostic_text(&actual), actual);
    }
}

#[test]
fn attached_rsa_private_key_pem_headers_hide_the_body() {
    for prefix in ["\"", "pem:", "key=", "(", "prefix"] {
        for footer in ["-----END RSA PRIVATE KEY----- after", ""] {
            let input = format!(
                "before {prefix}-----BEGIN RSA PRIVATE KEY-----\npem-body-canary\n{footer}"
            );
            let actual = sanitize_diagnostic_text(&input);
            assert!(
                !actual.contains("pem-body-canary"),
                "leaked PEM: {actual:?}"
            );
            assert!(actual.contains("[redacted]"), "missing marker: {actual:?}");
            assert_eq!(sanitize_diagnostic_text(&actual), actual);
        }
    }
}
