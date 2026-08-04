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
fn stderr_ring_allocates_lazily_and_never_retains_over_its_limit() {
    let mut ring = StderrRing::new(8, 8);
    assert_eq!(ring.bytes.capacity(), 0);
    ring.ingest(b"123456789");
    assert!(ring.bytes.capacity() <= 8);
    assert_eq!(ring.bytes.len(), 8);
    assert!(ring.safe_summary().truncated);
}
