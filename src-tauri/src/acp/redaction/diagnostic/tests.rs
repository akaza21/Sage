use super::sanitize_diagnostic_text;

#[path = "canaries.rs"]
mod canaries;

fn assert_redacted(input: &str, expected: &str) {
    let actual = sanitize_diagnostic_text(input);
    assert_eq!(actual, expected, "{input:?}");
    assert_eq!(
        sanitize_diagnostic_text(&actual),
        actual,
        "idempotent: {input:?}"
    );
}

macro_rules! redaction_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_redacted($input, $expected);
        }
    };
}

redaction_case!(
    compact_json_after_user,
    r#"{"user":"bob","token":"abc"}"#,
    r#"{"user":"bob","token":[redacted]}"#
);
redaction_case!(
    compact_nested_json,
    r#"{"error":{"message":"x","authorization":"Bearer abc"}}"#,
    r#"{"error":{"message":"x","authorization":[redacted]}}"#
);
redaction_case!(
    compact_headers_json,
    r#""headers":{"Authorization":"Bearer abc"}"#,
    r#""headers":{"Authorization":[redacted]}"#
);
redaction_case!(
    compact_multiple_secrets,
    r#"{"token":"abc","user":"bob","password":"xyz","count":3}"#,
    r#"{"token":[redacted],"user":"bob","password":[redacted],"count":3}"#
);
redaction_case!(
    escaped_json_key,
    r#"{\"token\":\"abc\"}"#,
    r#"{\"token\":[redacted]}"#
);
redaction_case!(
    structured_credentials_are_private,
    r#"{"credentials":{"user":"canary-user","password":"canary-password"},"count":3}"#,
    r#"{"credentials":[redacted],"count":3}"#
);
redaction_case!(
    secret_array_is_private,
    r#"{"tokens":3,"credentials":["canary-one","canary-two"],"count":4}"#,
    r#"{"tokens":3,"credentials":[redacted],"count":4}"#
);
redaction_case!(
    json_path_before_secret_stays_private,
    r#"{"path":"C:\\private\\file","token":"canary-token"}"#,
    r#"{"path":"[path]","token":[redacted]}"#
);
redaction_case!(
    compact_json_after_multiword_public_value,
    r#"{"user":"Bob Smith","token":"canary"}"#,
    r#"{"user":"Bob Smith","token":[redacted]}"#
);
redaction_case!(
    json_userinfo_after_secret_stays_private,
    r#"{"token":"abc","url":"https://user:secret@host/"}"#,
    r#"{"token":[redacted],"url":"https://[redacted]@host/"}"#
);
redaction_case!(
    unicode_escaped_json_key,
    r#"{"to\u006ben":"canary"}"#,
    r#"{"to\u006ben":[redacted]}"#
);
redaction_case!(
    encoded_json_embedded_quote,
    r#"{\"token\":\"prefix\\\"tail-canary\"}"#,
    r#"{\"token\":[redacted]}"#
);
redaction_case!(
    windows_multiword_name_with_continuation,
    r"C:\Users\John Paul Smith\.ssh\id failed",
    "[path] failed"
);

redaction_case!(
    proxy_authorization,
    "Proxy-Authorization: Basic abc",
    "Proxy-Authorization: [redacted] [redacted]"
);
redaction_case!(
    x_auth_token,
    "X-Auth-Token: abc",
    "X-Auth-Token: [redacted]"
);
redaction_case!(
    refresh_token_flag,
    "--refresh-token abc",
    "--refresh-token [redacted]"
);
redaction_case!(
    refresh_token_json,
    r#""refresh_token":"abc""#,
    r#""refresh_token":[redacted]"#
);
redaction_case!(
    id_token_json,
    r#""id_token":"abc""#,
    r#""id_token":[redacted]"#
);
redaction_case!(
    session_token_json,
    r#""session_token":"abc""#,
    r#""session_token":[redacted]"#
);
redaction_case!(
    api_token_json,
    r#""api_token":"abc""#,
    r#""api_token":[redacted]"#
);
redaction_case!(
    secret_key_json,
    r#""secret_key":"abc""#,
    r#""secret_key":[redacted]"#
);
redaction_case!(
    aws_secret_access_key_json,
    r#""aws_secret_access_key":"abc""#,
    r#""aws_secret_access_key":[redacted]"#
);
redaction_case!(
    credentials_json,
    r#""credentials":"abc""#,
    r#""credentials":[redacted]"#
);
redaction_case!(
    passphrase_flag,
    "--passphrase abc",
    "--passphrase [redacted]"
);
redaction_case!(cookie_header, "Cookie: abc", "Cookie: [redacted]");
redaction_case!(tokens_count_is_public, "Tokens: 1,234", "Tokens: 1,234");
redaction_case!(author_is_public, "Author: Jane", "Author: Jane");
redaction_case!(session_count_is_public, "Session: 3", "Session: 3");

redaction_case!(
    apostrophe_inside_single_quoted_secret,
    "--password 'it's secret' denied",
    "--password [redacted] denied"
);
redaction_case!(
    unclosed_single_quoted_multiword_secret,
    "--password 'it's secret never closed",
    "--password [redacted]"
);
redaction_case!(
    windows_paths_do_not_consume_intervening_prose,
    r"Cannot open C:\a\b because C:\c is locked",
    "Cannot open [path] because [path] is locked"
);
redaction_case!(
    windows_name_without_continuation_retains_prose,
    r"C:\Users\John Smith failed",
    "[path] Smith failed"
);
redaction_case!(
    windows_name_with_continuation_hides_name,
    r"C:\Users\John Smith\file failed",
    "[path] failed"
);

redaction_case!(
    dotted_password_assignment,
    "spring.datasource.password=hunter2",
    "[redacted]"
);
redaction_case!(namespaced_token_assignment, "env:TOKEN=abc", "[redacted]");
redaction_case!(indexed_key_assignment, "key[0]=abc", "[redacted]");
redaction_case!(dotted_api_key_assignment, "api.key=abc", "[redacted]");
redaction_case!(
    spaced_assignment_value,
    "KEY=  \"private multi word\"",
    "[redacted]  [redacted]"
);
redaction_case!(
    separated_assignment_delimiter,
    "KEY = \"private multi word\"",
    "KEY = [redacted]"
);
redaction_case!(
    separated_authorization_delimiter,
    "Authorization :  private-token",
    "Authorization :  [redacted]"
);
redaction_case!(
    json_authorization_value,
    r#"{"Authorization": "Basic dXNlcjpwYXNz"}"#,
    r#"{"Authorization": [redacted]}"#
);
redaction_case!(
    quoted_api_key_value,
    r#""x-api-key": "abc""#,
    r#""x-api-key": [redacted]"#
);
redaction_case!(
    json_token_value,
    r#"{"token":"abc123"}"#,
    r#"{"token":[redacted]}"#
);
redaction_case!(
    password_colon_value,
    "password: hunter2",
    "password: [redacted]"
);
redaction_case!(inline_token_colon_value, "token:abc", "[redacted]");
redaction_case!(inline_flag_colon_value, "--token:abc", "[redacted]");
redaction_case!(
    client_secret_flag_value,
    "--client-secret abc",
    "--client-secret [redacted]"
);
redaction_case!(
    access_token_flag_value,
    "--access-token abc",
    "--access-token [redacted]"
);
redaction_case!(
    at_prefixed_absolute_path,
    "curl -d @/home/u/secret",
    "curl -d [path]"
);
redaction_case!(colon_prefixed_absolute_path, "config:/home/u/x", "[path]");
redaction_case!(
    url_userinfo,
    "https://user:secret@host/",
    "https://[redacted]@host/"
);
redaction_case!(
    windows_path_with_spaces,
    r"C:\Users\John Smith\.ssh\id",
    "[path]"
);
redaction_case!(
    unclosed_single_quoted_value_fails_closed,
    "--password 'hunter2 rejected; it isn't valid",
    "--password [redacted]"
);
redaction_case!(
    quoted_assignment_syntax_remains_prose,
    r#"error: expected "KEY=" but got "x""#,
    r#"error: expected "KEY=" but got "x""#
);
redaction_case!(
    empty_quotes_with_attached_bearer_secret,
    r#"Bearer ""secret"#,
    "Bearer [redacted]"
);
redaction_case!(
    quoted_assignment_with_attached_secret,
    r#"KEY="a"rest"#,
    "[redacted]"
);
redaction_case!(risk_free_is_not_an_sk_token, "risk-free", "risk-free");
redaction_case!(task_runner_is_not_an_sk_token, "task-runner", "task-runner");
redaction_case!(disk_full_is_not_an_sk_token, "disk-full", "disk-full");
redaction_case!(sk_token_is_private, "sk-private-token", "[redacted]");
redaction_case!(
    separated_flag_assignment,
    "--token = abc",
    "--token = [redacted]"
);
redaction_case!(
    separated_flag_colon,
    "--client-secret : abc",
    "--client-secret : [redacted]"
);
redaction_case!(
    single_quoted_value_with_trailing_space,
    "--password 'private multi word ' denied",
    "--password [redacted] denied"
);
redaction_case!(
    single_quoted_assignment_with_trailing_space,
    "KEY='private multi word ' denied",
    "[redacted] denied"
);
redaction_case!(quoted_assignment_key, r#""password"="abc""#, "[redacted]");

#[test]
fn over_budget_quoted_values_fail_closed_with_a_marker() {
    let input = format!("KEY=\"alpha beta {}\" denied", "x".repeat(1100));
    assert_eq!(sanitize_diagnostic_text(&input), "[redacted]");
    let unicode = format!("KEY=\"{}\" denied", "\u{754c}".repeat(400));
    assert_eq!(sanitize_diagnostic_text(&unicode), "[redacted]");
}

#[test]
fn over_budget_attached_values_fail_closed_with_a_marker() {
    let input = format!("KEY=\"a\"{} denied", "rest".repeat(300));
    assert_eq!(sanitize_diagnostic_text(&input), "[redacted]");
}

#[test]
fn over_budget_windows_path_continuations_fail_closed() {
    let input = format!(r"C:\Users\John{}Smith\.ssh\id", " ".repeat(2048));
    assert_eq!(sanitize_diagnostic_text(&input), "[path]");
}

#[test]
fn sensitive_key_names_cover_flags_keys_and_json_keys() {
    for key in [
        "password",
        "passwd",
        "secret",
        "token",
        "api-key",
        "api_key",
        "apikey",
        "access-token",
        "access_token",
        "accesstoken",
        "client-secret",
        "client_secret",
        "clientsecret",
        "authorization",
        "auth",
        "credential",
        "private-key",
        "private_key",
        "privatekey",
    ] {
        assert_redacted(&format!("--{key} abc"), &format!("--{key} [redacted]"));
        assert_redacted(&format!("{key}:abc"), "[redacted]");
        assert_redacted(
            &format!(r#"{{"{key}":"abc"}}"#),
            &format!(r#"{{"{key}":[redacted]}}"#),
        );
    }
}

#[test]
fn repeated_unclosed_value_quotes_have_bounded_work_and_visible_markers() {
    let input = "--password 'private ".repeat(4096);
    assert_eq!(sanitize_diagnostic_text(&input), "--password [redacted]");
}

#[test]
fn many_compact_credentials_keep_public_fields() {
    let input = format!(
        "[{}]",
        [r#"{"user":"bob","token":"canary"}"#; 4096].join(",")
    );
    let expected = format!(
        "[{}]",
        [r#"{"user":"bob","token":[redacted]}"#; 4096].join(",")
    );
    assert_redacted(&input, &expected);
}

redaction_case!(
    outer_quoted_escaped_array,
    r#""[{\"token\":\"canary\"}]""#,
    r#""[{\"token\":[redacted]}]""#
);
redaction_case!(
    encoded_json_scalar_auth_scheme,
    r#"{\"message\":\"Bearer canary\"}"#,
    r#"{\"message\":\"Bearer [redacted]\"}"#
);
redaction_case!(
    encoded_json_scalar_sensitive_flag,
    r#"{\"message\":\"--token canary\"}"#,
    r#"{\"message\":\"--token [redacted]\"}"#
);

redaction_case!(
    json_path_key_is_private,
    r#"{"files":{"/home/alice/project/config.toml":"permission denied"}}"#,
    r#"{"files":{"[path]":"permission denied"}}"#
);

redaction_case!(
    json_whitespace_before_sensitive_colon,
    r#"{"user":"bob","token" : "canary"}"#,
    r#"{"user":"bob","token" : [redacted]}"#
);
redaction_case!(
    json_escaped_url_separators,
    r#"{"token":"abc","url":"https:\/\/alice:canary@host\/"}"#,
    r#"{"token":[redacted],"url":"https://[redacted]@host/"}"#
);
redaction_case!(
    outer_quoted_escaped_json,
    r#""{\"token\":\"canary\"}""#,
    r#""{\"token\":[redacted]}""#
);

redaction_case!(
    multiword_json_userinfo_stays_private,
    r#"{"token":"abc","message":"https://user:secret@host/ failed"}"#,
    r#"{"token":[redacted],"message":"https://[redacted]@host/ failed"}"#
);
