use super::*;

#[test]
fn diagnostics_never_contain_sensitive_input() {
    let canary = b"token=never-log-this";
    let mut ring = StderrRing::new(8, 4);
    ring.ingest(canary);
    let serialized = serde_json::to_string(&ring.safe_summary()).unwrap();
    assert!(!serialized.contains("never-log-this"));
    assert_eq!(ring.bytes.len(), 4);
    assert!(ring.safe_summary().truncated);

    let fingerprint =
        serde_json::to_string(&ExternalIdFingerprint::from_external_id(canary)).unwrap();
    assert!(!fingerprint.contains("token"));
}

#[test]
fn sensitive_bytes_have_normalizer_only_access() {
    let bytes = SensitiveBytes::new(b"secret".to_vec());
    assert_eq!(bytes.expose_to_normalizer().len(), 6);
}

#[test]
fn sensitive_bytes_redact_through_the_central_typed_boundary() {
    let canary = "secret.central-boundary.7e31";
    let diagnostic =
        SensitiveBytes::new(canary.as_bytes().to_vec()).redact(DiagnosticCode::IdentityMismatch);
    assert_eq!(diagnostic.code, DiagnosticCode::IdentityMismatch);
    assert_eq!(diagnostic.observed_bytes, canary.len() as u64);
    assert!(!diagnostic.truncated);
    assert!(!serde_json::to_string(&diagnostic).unwrap().contains(canary));
}

#[test]
fn stderr_ring_allocates_lazily_and_never_retains_over_its_limit() {
    let mut ring = StderrRing::new(8, 8);
    assert_eq!(ring.bytes.capacity(), 0);
    ring.ingest(b"123456789");
    assert!(ring.bytes.capacity() <= 8);
    assert_eq!(ring.bytes.len(), 8);
    assert!(ring.safe_summary().truncated);
}

#[test]
fn stderr_diagnostic_extracts_a_redacted_json_rpc_error() {
    let mut ring = StderrRing::new(512, 512);
    ring.ingest(
        br#"{"method":"thread/start","error":{"code":-32601,"message":"Bearer secret-token at C:\\private\\file"}}"#,
    );

    let diagnostic = ring.safe_diagnostic();

    assert_eq!(diagnostic.method.as_deref(), Some("thread/start"));
    assert_eq!(diagnostic.error_code.as_deref(), Some("-32601"));
    assert_eq!(
        diagnostic.error_message.as_deref(),
        Some("Bearer [redacted] at [path]")
    );
}

#[test]
fn stderr_diagnostic_redacts_unstructured_secret_text() {
    let mut ring = StderrRing::new(512, 512);
    ring.ingest(b"token=never-log-this");

    let diagnostic = ring.safe_diagnostic();

    assert_eq!(diagnostic.method, None);
    assert_eq!(diagnostic.error_code.as_deref(), Some("child-stderr"));
    assert_eq!(diagnostic.error_message.as_deref(), Some("[redacted]"));
}
