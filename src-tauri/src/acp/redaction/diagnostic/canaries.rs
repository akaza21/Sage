use super::sanitize_diagnostic_text;

#[path = "pem_canaries.rs"]
mod pem_canaries;

#[test]
fn fallback_assignments_and_embedded_sk_tokens_hide_secrets() {
    for (input, secret) in [
        ("@{token=abc; user=bob}", "abc"),
        ("$env:API_KEY=hunter2", "hunter2"),
        ("${TOKEN}=abc", "abc"),
        ("provided:sk-abc", "sk-abc"),
        (r#"{"msg":"provided:sk-abc"}"#, "sk-abc"),
        ("Invalid key sk-or-v1-abc123", "sk-or-v1-abc123"),
        ("sk-lf-1234abcd", "sk-lf-1234abcd"),
        ("sk-proj-AbC123", "sk-proj-AbC123"),
        ("\u{754c}sk-abc", "sk-abc"),
        ("\u{5bc6}\u{94a5}sk-proj-abc123", "sk-proj-abc123"),
        ("sk-__a", "sk-__a"),
    ] {
        let actual = sanitize_diagnostic_text(input);
        assert!(!actual.contains(secret), "leaked {secret:?}: {actual:?}");
        assert!(actual.contains("[redacted]"), "missing marker: {actual:?}");
        assert_eq!(sanitize_diagnostic_text(&actual), actual);
    }
    for input in [
        "risk-free",
        "task-runner",
        "disk-full",
        "sk-ab",
        "provided:sk-!",
        "sk-___",
    ] {
        assert_eq!(sanitize_diagnostic_text(input), input);
    }
}
