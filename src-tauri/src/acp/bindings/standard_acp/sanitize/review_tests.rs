use super::*;

use crate::acp::bindings::command_summary::sanitize_command_summary;

fn assert_secret(input: &str, private: &str) {
    for shown in [
        normalized_text::<1024>("text", input)
            .unwrap()
            .as_str()
            .to_owned(),
        normalized_label_text::<1024>("toolLabel", input)
            .unwrap()
            .as_str()
            .to_owned(),
        sanitized_content_output(input).unwrap(),
        sanitized_command_output(input).unwrap(),
        sanitize_command_summary(input).unwrap().as_str().to_owned(),
    ] {
        assert!(!shown.contains(private), "{input}: {shown}");
        assert!(shown.contains("[redacted]"), "{input}: {shown}");
    }
}

fn assert_preserved(input: &str) {
    assert_eq!(
        normalized_text::<1024>("text", input).unwrap().as_str(),
        input
    );
    assert_eq!(
        normalized_label_text::<1024>("toolLabel", input)
            .unwrap()
            .as_str(),
        input
    );
    assert_eq!(sanitized_content_output(input).unwrap(), input);
    assert_eq!(sanitized_command_output(input).unwrap(), input);
    assert_eq!(sanitize_command_summary(input).unwrap().as_str(), input);
}

macro_rules! secret_case {
    ($name:ident, $input:expr, $private:expr) => {
        #[test]
        fn $name() {
            assert_secret($input, $private);
        }
    };
}

macro_rules! preserved_case {
    ($name:ident, $input:expr) => {
        #[test]
        fn $name() {
            assert_preserved($input);
        }
    };
}

secret_case!(colon_auth, "auth:abc", "abc");
secret_case!(colon_key, "key:abc", "abc");
secret_case!(colon_p, "p:abc", "abc");
secret_case!(colon_pw, "pw:abc", "abc");
secret_case!(colon_pass, "pass:abc", "abc");
secret_case!(colon_session, "session:abc", "abc");
secret_case!(pgpassword, "PGPASSWORD=hunter2", "hunter2");
secret_case!(access_token, "accesstoken=opaque", "opaque");
secret_case!(auth_token, "authtoken=opaque", "opaque");
secret_case!(api_token, "apitoken=opaque", "opaque");
secret_case!(secret_key, "secretkey=opaque", "opaque");
secret_case!(private_key, "privatekey=opaque", "opaque");
secret_case!(redis_password, "REDISPASSWORD=opaque", "opaque");
secret_case!(plural_secrets, "SECRETS=opaque", "opaque");
secret_case!(passphrase_flag, "--passphrase opaque", "opaque");
secret_case!(session_id, "sessionid=opaque", "opaque");
secret_case!(java_session_id, "JSESSIONID=opaque", "opaque");
preserved_case!(author, "Author: Jane");
preserved_case!(tokens_count, "Tokens: 1,234");
preserved_case!(session_count, "Session: 3");
preserved_case!(statistical_p, "p=0.05");
preserved_case!(key_prose, "\"Key: the point is\"");

secret_case!(escaped_json_token, r#"{\"token\":\"abc\"}"#, "abc");
secret_case!(escaped_json_password, r#"{\"password\": \"a b\"}"#, "a b");
secret_case!(curl_json, r#"curl -d "{\"token\":\"abc\"}""#, "abc");
secret_case!(glued_quote_tail, r#"password="a"bc"#, "bc");
secret_case!(glued_quote_chain, r#"DB_PASSWORD="x"'y'"#, "y");
secret_case!(
    pem_body,
    "KEY='-----BEGIN PRIVATE KEY-----\nMIIEprivate\n-----END PRIVATE KEY-----' tail",
    "MIIEprivate"
);
#[test]
fn ordinary_apostrophe() {
    let input = "O'Brien\nordinary output";
    assert_eq!(sanitized_content_output(input).unwrap(), input);
    assert_eq!(sanitized_command_output(input).unwrap(), input);
    assert_eq!(
        normalized_text::<1024>("text", input).unwrap().as_str(),
        input
    );
    assert_eq!(
        normalized_label_text::<1024>("toolLabel", input)
            .unwrap()
            .as_str(),
        input
    );
    assert!(sanitize_command_summary(input).is_err());
}
preserved_case!(ordinary_json, r#"{"user":"bob","message":"O'Brien"}"#);

fn assert_nested(input: &str, private: &str, public: &str) {
    assert_secret(input, private);
    assert!(sanitized_content_output(input).unwrap().contains(public));
    assert!(sanitized_command_output(input).unwrap().contains(public));
    assert!(
        sanitize_command_summary(input)
            .unwrap()
            .as_str()
            .contains(public)
    );
    assert!(
        normalized_text::<1024>("text", input)
            .unwrap()
            .as_str()
            .contains(public)
    );
    assert!(
        normalized_label_text::<1024>("toolLabel", input)
            .unwrap()
            .as_str()
            .contains(public)
    );
}

#[test]
fn second_json_key() {
    assert_nested(r#"{"user":"bob","token":"abc"}"#, "abc", "bob");
}

#[test]
fn nested_header() {
    assert_nested(
        r#"{"headers":{"Authorization":"Bearer x"}}"#,
        "Bearer x",
        "headers",
    );
}

#[test]
fn quote_line_limit_fails_closed() {
    let input = format!("KEY='{}body' tail", "secret\n".repeat(65));
    assert_secret(&input, "body");
    assert!(
        sanitized_command_output(&input)
            .unwrap()
            .contains("[secret limit]")
    );
    assert!(
        sanitized_content_output(&input)
            .unwrap()
            .contains("[secret limit]")
    );
}

#[test]
fn quote_byte_limit_fails_closed() {
    let input = format!("KEY='{}body' tail", "x".repeat(8192));
    for shown in [
        sanitized_command_output(&input).unwrap(),
        sanitized_content_output(&input).unwrap(),
        sanitize_command_summary(&input)
            .unwrap()
            .as_str()
            .to_owned(),
    ] {
        assert!(!shown.contains("body"));
        assert!(shown.contains("[secret limit]"));
    }
}

macro_rules! path_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(sanitized_command_output($input).unwrap(), $expected);
            assert_eq!(sanitized_content_output($input).unwrap(), $expected);
            assert_eq!(
                normalized_text::<1024>("text", $input).unwrap().as_str(),
                $expected
            );
            assert_eq!(
                normalized_label_text::<1024>("toolLabel", $input)
                    .unwrap()
                    .as_str(),
                $expected
            );
            assert!(sanitize_command_summary($input).is_err());
        }
    };
}

path_case!(idirafter_path, "cc -idirafter/abs/path", "cc [redacted]");
path_case!(imacros_path, "cc -imacros/abs/path", "cc [redacted]");
path_case!(include_path, "cc -include/abs/path", "cc [redacted]");
path_case!(isysroot_path, "cc -isysroot/abs/path", "cc [redacted]");
path_case!(
    arbitrary_lowercase_flag_path,
    "cc -unknown/abs/path",
    "cc [redacted]"
);
path_case!(
    three_word_windows_name,
    r"C:\Users\Jane Doe Smith\x tail",
    "[redacted] Doe [redacted] tail"
);
path_case!(
    trailing_windows_name,
    r"C:\Users\John Smith",
    "[redacted] Smith"
);
path_case!(
    windows_home_name,
    r"C:\home\Jane Doe Smith\x tail",
    "[redacted] Doe [redacted] tail"
);
preserved_case!(relative_glued_flag, "cc -includeinclude.h");

#[test]
fn non_home_path_does_not_consume_following_words() {
    assert_eq!(
        sanitized_command_output(r"C:\src\x ordinary words").unwrap(),
        "[redacted] ordinary words"
    );
    assert_eq!(
        sanitized_content_output(r"C:\src\x ordinary words").unwrap(),
        "[redacted] ordinary words"
    );
}

macro_rules! output_case {
    ($name:ident, $input:expr) => {
        #[test]
        fn $name() {
            assert_eq!(sanitized_command_output($input).unwrap(), $input);
            assert_eq!(sanitized_content_output($input).unwrap(), $input);
        }
    };
}
output_case!(division, "3 / 4");
output_case!(closing_tag, "</div>");
output_case!(source_comment, "//TODO");

#[test]
fn second_unquoted_assignment() {
    assert_nested("user=bob,token=abc", "abc", "bob");
}

#[test]
fn quoted_flag_keeps_following_prose() {
    let input = "before --token 'secret words' tail";
    assert_eq!(
        sanitized_content_output(input).unwrap(),
        "before --token [redacted] tail"
    );
    assert_eq!(
        sanitized_command_output(input).unwrap(),
        "before --token [redacted] tail"
    );
}

#[test]
fn glued_open_quote_continues_across_lines() {
    let input = "TOKEN=prefix'first\nprivate body' tail";
    assert!(
        !sanitized_command_output(input)
            .unwrap()
            .contains("private body")
    );
    assert!(
        !sanitize_command_summary(input)
            .unwrap()
            .as_str()
            .contains("private body")
    );
    assert!(sanitized_command_output(input).unwrap().ends_with(" tail"));
    assert!(
        !sanitized_content_output(input)
            .unwrap()
            .contains("private body")
    );
    assert!(sanitized_content_output(input).unwrap().ends_with(" tail"));
}

#[test]
fn quote_limit_on_flag_preserves_the_limit_marker() {
    let input = format!("--token '{}body' tail", "secret\n".repeat(65));
    assert_secret(&input, "body");
    assert!(
        sanitized_content_output(&input)
            .unwrap()
            .contains("[secret limit]")
    );
}

secret_case!(
    escaped_quote_inside_json,
    r#"{\"token\":\"a\\\" private body\"}"#,
    "private body"
);
secret_case!(
    escaped_quote_inside_curl_json,
    r#"curl -d "{\"token\":\"a\\\" private body\"}""#,
    "private body"
);
secret_case!(json_auth, r#"{"user":"bob","auth":"abc"}"#, "abc");
secret_case!(json_key, r#"{"user":"bob","key":"abc"}"#, "abc");
secret_case!(json_p, r#"{"user":"bob","p":"abc"}"#, "abc");
secret_case!(json_pw, r#"{"user":"bob","pw":"abc"}"#, "abc");
secret_case!(json_pass, r#"{"user":"bob","pass":"abc"}"#, "abc");
secret_case!(json_session, r#"{"user":"bob","session":"abc"}"#, "abc");
secret_case!(
    numeric_second_json_password,
    r#"{"user":"bob","password":1234}"#,
    "1234"
);
secret_case!(
    numeric_second_json_pw,
    r#"{"user":"bob","pw":1234}"#,
    "1234"
);

#[test]
fn later_equals_key_keeps_public_prefix() {
    assert_nested("user=bob=token=abc", "abc", "bob");
}

#[test]
fn first_unquoted_assignment_keeps_public_sibling() {
    assert_nested("token=abc,user=bob", "abc", "bob");
}

#[test]
fn spaced_assignment_keeps_public_sibling() {
    assert_nested("token = abc,mode=public", "abc", "public");
}

secret_case!(
    literal_marker_cannot_expose_glued_flag_value,
    "--token [redacted]abc",
    "abc"
);
secret_case!(
    literal_marker_cannot_expose_comma_flag_value,
    "--token [redacted],abc",
    "abc"
);

secret_case!(
    literal_marker_cannot_end_a_header_value,
    "Authorization: foo [redacted]abc",
    "abc"
);

#[test]
fn repeated_p_assignments_have_linear_cost() {
    let input = "p=".repeat(32_768);
    let started = std::time::Instant::now();
    let shown = sanitized_command_output(&input).unwrap();
    assert!(shown.contains("[redacted]"));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn quoted_assignment_argument_limit_keeps_a_marker() {
    let input = format!("--token account='{}body' tail", "x".repeat(8192));
    for shown in [
        sanitized_command_output(&input).unwrap(),
        sanitized_content_output(&input).unwrap(),
    ] {
        assert!(!shown.contains("body"));
        assert!(shown.contains("[secret limit]"));
    }
}

#[test]
fn quote_at_the_line_bound_keeps_following_public_text() {
    let input = format!("KEY='{}body' tail", "secret\n".repeat(63));
    assert_secret(&input, "body");
    assert!(sanitized_command_output(&input).unwrap().ends_with(" tail"));
    assert!(sanitized_content_output(&input).unwrap().ends_with(" tail"));
}

#[test]
fn quote_below_the_byte_bound_keeps_following_public_text() {
    let input = format!("KEY='{}body' tail", "x".repeat(8184));
    for shown in [
        sanitized_command_output(&input).unwrap(),
        sanitized_content_output(&input).unwrap(),
        sanitize_command_summary(&input)
            .unwrap()
            .as_str()
            .to_owned(),
    ] {
        assert!(!shown.contains("body"));
        assert!(shown.ends_with(" tail"));
        assert!(!shown.contains("[secret limit]"));
    }
}
