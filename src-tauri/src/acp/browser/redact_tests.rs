use super::*;

const JWT: &str =
    "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

#[test]
fn the_contract_query_keys_are_redacted_whatever_their_value() {
    for key in [
        "refresh_token",
        "client_secret",
        "client_assertion",
        "apikey",
        "api-key",
        "X-Amz-Signature",
        "x-amz-credential",
        "X-Amz-Security-Token",
        "X-Goog-Signature",
        "state",
    ] {
        let redacted = redact_url(&format!("https://example.com/cb?{key}=short&page=2"));
        assert_eq!(
            redacted,
            format!("https://example.com/cb?{key}=[redacted]&page=2"),
            "{key}"
        );
    }
}

#[test]
fn secret_like_values_of_other_keys_and_bare_segments_are_redacted() {
    let hex = "a".repeat(16) + &"0123456789abcdef".repeat(2);
    let base64 = "QWxhZGRpbjpvcGVuIHNlc2FtZQQWxhZGRpbjpvcGVu";
    let mixed = "Ab3dEf6hIj9kLm2nOp5qRs8t";
    for value in [
        JWT,
        hex.as_str(),
        base64,
        mixed,
        "ghp_abcdefghijklmnopqrstu",
    ] {
        assert_eq!(
            redact_url(&format!("https://example.com/?next={value}&q=boats")),
            "https://example.com/?next=[redacted]&q=boats",
            "{value}"
        );
        assert_eq!(
            redact_url(&format!("https://example.com/#{value}")),
            "https://example.com/#[redacted]",
            "{value}"
        );
        assert_eq!(
            redact_url(&format!("https://example.com/?{value}")),
            "https://example.com/?[redacted]",
            "{value}"
        );
    }
}

#[test]
fn ordinary_values_and_slugs_survive() {
    for url in [
        "https://example.com/wiki/Lighthouse?q=red+lighthouse&page=2#History",
        "https://example.com/blog/how-to-build-a-lighthouse-in-ten-days-2024",
        "https://example.com/items/123e4567-e89b-12d3-a456-426614174000",
        "https://example.com/#/settings/profile",
        "https://example.com/search?q=CamelCaseWordsWithout",
    ] {
        assert_eq!(redact_url(url), url);
    }
}

#[test]
fn word_slugs_survive_and_random_tokens_do_not() {
    // Words switch case or turn into digits rarely; separators do not count.
    for slug in [
        "Rust2024-Edition-Release-Notes",
        "covid19-vaccine-Update-2021-Moderna",
        "how_to_configure_the_reverse_proxy_server",
        "My-Long-Article-Title-About-Something",
        "iPhone15ProMaxReview2024Edition",
        "507f1f77bcf86cd799439011",
        "123E4567-E89B-12D3-A456-426614174000",
    ] {
        for url in [
            format!("https://example.com/posts/{slug}"),
            format!("https://example.com/?next={slug}"),
            format!("https://example.com/#{slug}"),
        ] {
            assert_eq!(redact_url(&url), url, "{slug}");
        }
    }
    let hex = "0123456789abcdef".repeat(2);
    for token in [
        JWT,
        hex.as_str(),
        "Xk9_fQ2-mZ7pL4wR8tY1uV6s",
        "aB3dE5gH7jK9mN1pQ3sT5vX7",
        "dGhpcyBpcyBhIHNlY3JldCB0b2tlbiB2YWx1ZQ==",
        "Ab3dEf6hIj9kLm2nOp5qRs8t",
        // Base64 of text switches rarely, but runs long without a separator.
        "QWxhZGRpbjpvcGVuIHNlc2FtZQQWxhZGRpbjpvcGVu",
    ] {
        assert_eq!(
            redact_url(&format!("https://example.com/posts/{token}")),
            "https://example.com/posts/%5Bredacted%5D",
            "{token}"
        );
        assert_eq!(
            redact_url(&format!("https://example.com/?next={token}")),
            "https://example.com/?next=[redacted]",
            "{token}"
        );
    }
}

#[test]
fn credential_path_segments_are_redacted_but_git_object_ids_are_kept() {
    assert_eq!(
        redact_url(&format!("https://example.com/reset/{JWT}/done")),
        "https://example.com/reset/%5Bredacted%5D/done"
    );
    assert_eq!(
        redact_url("https://example.com/invite/Ab3dEf6hIj9kLm2nOp5qRs8t"),
        "https://example.com/invite/%5Bredacted%5D"
    );
    let sha1 = "0123456789abcdef0123456789abcdef01234567";
    let sha256 = "0123456789abcdef".repeat(4);
    for hash in [sha1, sha256.as_str()] {
        let url = format!("https://forge.example/acme/sage/commit/{hash}");
        assert_eq!(redact_url(&url), url);
    }
    // Hex runs of other lengths are not exempt.
    let other = "0123456789abcdef".repeat(3);
    assert_eq!(
        redact_url(&format!("https://example.com/files/{other}")),
        "https://example.com/files/%5Bredacted%5D"
    );
    // An encoded secret is decoded before it is judged.
    assert_eq!(
        redact_url("https://example.com/k/%41b3dEf6hIj9kLm2nOp5qRs8t"),
        "https://example.com/k/%5Bredacted%5D"
    );
}

#[test]
fn redact_url_is_idempotent() {
    for input in [
        format!("https://user:pass@example.com/reset/{JWT}?token=a&next={JWT}#state=x&{JWT}"),
        "https://example.com/a/Ab3dEf6hIj9kLm2nOp5qRs8t/b?client_secret=s;page=1".into(),
        "https://example.com/wiki/Lighthouse?q=red#History".into(),
        "https://example.com/%5Bredacted%5D?[redacted]&x=[redacted]".into(),
        "not a url".into(),
        "ftp://example.com/file".into(),
    ] {
        let once = redact_url(&input);
        assert_eq!(redact_url(&once), once, "{input}");
        assert!(!once.contains("eyJ"), "{once}");
    }
}

#[test]
fn page_text_filter_removes_controls_and_bounds_multibyte_text() {
    assert_eq!(safe_page_text("A\u{202e}B\nC", 16, true), "AB\nC");
    assert_eq!(safe_page_text("A\u{2066}B\nC\t", 16, false), "ABC");
    assert_eq!(safe_page_text("ééé", 5, true), "éé");
}
