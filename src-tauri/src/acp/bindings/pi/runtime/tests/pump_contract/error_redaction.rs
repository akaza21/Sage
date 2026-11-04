use super::turn_failures::Reopened;
use super::*;
use crate::acp::{
    intent::ErrorKind, lifecycle::TurnState, projection::ItemContent, store::load_chat_page,
};

#[tokio::test]
async fn pi_credentials_never_reopen_in_terminal_reasons_or_retry_items() {
    // Whole-page scans include random UUIDs; canaries must be distinctive non-hex values.
    for (detail, expected, canaries) in [
        (
            r#"{"user":"Bob Smith","token":"pi-canary-Q7z9-multiword-value"}"#,
            r#"{"user":"Bob Smith","token":[redacted]}"#,
            &["pi-canary-Q7z9-multiword-value"][..],
        ),
        (
            r#"{"credentials":["pi-canary-Q7z9-array-value-one","pi-canary-Q7z9-array-value-two"],"count":4}"#,
            r#"{"credentials":[redacted],"count":4}"#,
            &[
                "pi-canary-Q7z9-array-value-one",
                "pi-canary-Q7z9-array-value-two",
            ][..],
        ),
        (
            r#"{"to\u006ben":"pi-canary-Q7z9-unicode-key-value"}"#,
            r#"{"to\u006ben":[redacted]}"#,
            &["pi-canary-Q7z9-unicode-key-value"][..],
        ),
        (
            r#"{\"token\":\"pi-canary-Q7z9-escaped-quote-prefix\\\"pi-canary-Q7z9-attached-tail-value\"}"#,
            r#"{\"token\":[redacted]}"#,
            &[
                "pi-canary-Q7z9-escaped-quote-prefix",
                "pi-canary-Q7z9-attached-tail-value",
            ][..],
        ),
        (
            r#"{"token":"pi-canary-Q7z9-json-token-value","message":"https://pi-canary-Q7z9-url-user:pi-canary-Q7z9-url-password@host/ failed"}"#,
            r#"{"token":[redacted],"message":"https://[redacted]@host/ failed"}"#,
            &[
                "pi-canary-Q7z9-json-token-value",
                "pi-canary-Q7z9-url-user:pi-canary-Q7z9-url-password",
            ][..],
        ),
        (
            r"C:\Users\pi-canary-Q7z9-John pi-canary-Q7z9-Paul pi-canary-Q7z9-Smith\.ssh\id failed",
            "[path] failed",
            &[
                "pi-canary-Q7z9-John",
                "pi-canary-Q7z9-Paul",
                "pi-canary-Q7z9-Smith",
            ][..],
        ),
        (
            "Bearer  pi-canary-Q7z9-private-token-value",
            "Bearer  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Bearer pi-canary-Q7z9-private\u{0000}token-value",
            "Bearer [redacted]",
            &["pi-canary-Q7z9-privatetoken-value"][..],
        ),
        (
            "Bearer\r\rpi-canary-Q7z9-private-token-value",
            "Bearer  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "--token\r\rpi-canary-Q7z9-private-token-value",
            "--token  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Bearer\u{000b}\u{000c}pi-canary-Q7z9-private-token-value",
            "Bearer  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Authorization: Basic pi-canary-Q7z9-private-token-value",
            "Authorization: [redacted] [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Authorization:Basic pi-canary-Q7z9-private-token-value",
            "[redacted] [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            r#""failed Bearer pi-canary-Q7z9-private-token-value""#,
            r#""failed Bearer [redacted]""#,
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Bearer \t\n pi-canary-Q7z9-private-token-value",
            "Bearer \t\n [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Bearer \"pi-canary-Q7z9-private multi word value\" denied",
            "Bearer [redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "Authorization:  Bearer  pi-canary-Q7z9-private-token-value",
            "Authorization:  [redacted]  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Authorization:Bearer pi-canary-Q7z9-private-token-value",
            "[redacted] [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "Authorization: \"Bearer pi-canary-Q7z9-private token value\" denied",
            "Authorization: [redacted] denied",
            &["Bearer pi-canary-Q7z9-private token value"][..],
        ),
        (
            "--token  pi-canary-Q7z9-private-token-value denied",
            "--token  [redacted] denied",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "--password 'pi-canary-Q7z9-private multi word value' denied",
            "--password [redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "--secret \"pi-canary-Q7z9-private multi word value\" denied",
            "--secret [redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "--api-key  pi-canary-Q7z9-private-token-value denied",
            "--api-key  [redacted] denied",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            "KEY=\"pi-canary-Q7z9-private multi word value\" denied",
            "[redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "KEY='pi-canary-Q7z9-private multi word value' denied",
            "[redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "spring.datasource.password=pi-canary-Q7z9-password-value",
            "[redacted]",
            &["pi-canary-Q7z9-password-value"][..],
        ),
        (
            "env:TOKEN=pi-canary-Q7z9-api-key-value",
            "[redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "key[0]=pi-canary-Q7z9-api-key-value",
            "[redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "api.key=pi-canary-Q7z9-api-key-value",
            "[redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "Authorization :  pi-canary-Q7z9-private-token-value",
            "Authorization :  [redacted]",
            &["pi-canary-Q7z9-private-token-value"][..],
        ),
        (
            r#"{"Authorization": "Basic cGktY2FuYXJ5LVE3ejktYmFzaWMtdXNlcjpwYXNzd29yZA=="}"#,
            r#"{"Authorization": [redacted]}"#,
            &["cGktY2FuYXJ5LVE3ejktYmFzaWMtdXNlcjpwYXNzd29yZA=="][..],
        ),
        (
            r#""x-api-key": "pi-canary-Q7z9-api-key-value""#,
            r#""x-api-key": [redacted]"#,
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            r#"{"token":"pi-canary-Q7z9-compact-token-value"}"#,
            r#"{"token":[redacted]}"#,
            &["pi-canary-Q7z9-compact-token-value"][..],
        ),
        (
            "password: pi-canary-Q7z9-password-value",
            "password: [redacted]",
            &["pi-canary-Q7z9-password-value"][..],
        ),
        (
            "token:pi-canary-Q7z9-api-key-value",
            "[redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "--token:pi-canary-Q7z9-api-key-value",
            "[redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "--client-secret pi-canary-Q7z9-api-key-value",
            "--client-secret [redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "--access-token pi-canary-Q7z9-api-key-value",
            "--access-token [redacted]",
            &["pi-canary-Q7z9-api-key-value"][..],
        ),
        (
            "curl -d @/home/pi-canary-Q7z9-user/secret-value",
            "curl -d [path]",
            &["/home/pi-canary-Q7z9-user/secret-value"][..],
        ),
        (
            "config:/home/pi-canary-Q7z9-user/config-value",
            "[path]",
            &["/home/pi-canary-Q7z9-user/config-value"][..],
        ),
        (
            "https://pi-canary-Q7z9-url-user:pi-canary-Q7z9-url-password@host/",
            "https://[redacted]@host/",
            &["pi-canary-Q7z9-url-user:pi-canary-Q7z9-url-password"][..],
        ),
        (
            r"C:\Users\pi-canary-Q7z9-John pi-canary-Q7z9-Smith\.ssh\id",
            "[path]",
            &["pi-canary-Q7z9-John", "pi-canary-Q7z9-Smith"][..],
        ),
        (
            "--password 'pi-canary-Q7z9-password-value rejected; it isn't valid",
            "--password [redacted]",
            &["pi-canary-Q7z9-password-value"][..],
        ),
        (
            r#"error: expected "KEY=" but got "x""#,
            r#"error: expected "KEY=" but got "x""#,
            &[][..],
        ),
        (
            r#"Bearer ""pi-canary-Q7z9-unclosed-bearer-value"#,
            "Bearer [redacted]",
            &["pi-canary-Q7z9-unclosed-bearer-value"][..],
        ),
        (
            r#"KEY="pi-canary-Q7z9-attached-quoted-value"pi-canary-Q7z9-attached-tail-value"#,
            "[redacted]",
            &[
                "pi-canary-Q7z9-attached-quoted-value",
                "pi-canary-Q7z9-attached-tail-value",
            ][..],
        ),
        (
            "risk-free task-runner disk-full",
            "risk-free task-runner disk-full",
            &[][..],
        ),
        (
            "KEY=  \"pi-canary-Q7z9-private multi word value\" denied",
            "[redacted]  [redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            "KEY = \"pi-canary-Q7z9-private multi word value\" denied",
            "KEY = [redacted] denied",
            &["pi-canary-Q7z9-private multi word value"][..],
        ),
        (
            r#"{"user":"bob","token":"pi-canary-Q7z9-compact-json-value"}"#,
            r#"{"user":"bob","token":[redacted]}"#,
            &["pi-canary-Q7z9-compact-json-value"][..],
        ),
        (
            r#"{"error":{"message":"x","authorization":"Bearer pi-canary-Q7z9-nested-auth-value"}}"#,
            r#"{"error":{"message":"x","authorization":[redacted]}}"#,
            &["pi-canary-Q7z9-nested-auth-value"][..],
        ),
        (
            r#""headers":{"Authorization":"Bearer pi-canary-Q7z9-header-auth-value"}"#,
            r#""headers":{"Authorization":[redacted]}"#,
            &["pi-canary-Q7z9-header-auth-value"][..],
        ),
        (
            r#"{\"token\":\"pi-canary-Q7z9-escaped-json-value\"}"#,
            r#"{\"token\":[redacted]}"#,
            &["pi-canary-Q7z9-escaped-json-value"][..],
        ),
        (
            "Proxy-Authorization: Basic pi-canary-Q7z9-proxy-auth-value",
            "Proxy-Authorization: [redacted] [redacted]",
            &["pi-canary-Q7z9-proxy-auth-value"][..],
        ),
        (
            "--password 'it's pi-canary-Q7z9-quoted-password-value' denied",
            "--password [redacted] denied",
            &["it's pi-canary-Q7z9-quoted-password-value"][..],
        ),
        (
            "--password 'it's pi-canary-Q7z9-unclosed-password-value pi-canary-Q7z9-unclosed-tail-value",
            "--password [redacted]",
            &[
                "pi-canary-Q7z9-unclosed-password-value",
                "pi-canary-Q7z9-unclosed-tail-value",
            ][..],
        ),
        (
            r"Cannot open C:\pi-canary-Q7z9-directory\file-value because C:\pi-canary-Q7z9-locked-file-value is locked",
            "Cannot open [path] because [path] is locked",
            &[
                r"C:\pi-canary-Q7z9-directory\file-value",
                r"C:\pi-canary-Q7z9-locked-file-value",
            ][..],
        ),
    ] {
        for retry in [false, true] {
            let fixture = Fixture::new();
            let mut pump = fixture.pump(ready_adapter());
            start(&mut pump).await;
            for event in [
                json!({"type":"agent_end","messages":[{"role":"assistant","stopReason":"error","errorMessage":detail}]}),
                json!({"type":"auto_retry_start","attempt":1,"errorMessage":detail}),
                json!({"type":"auto_retry_end","attempt":1,"success":false,"finalError":detail}),
                json!({"type":"agent_settled"}),
            ] {
                if !retry
                    && matches!(
                        event["type"].as_str(),
                        Some("auto_retry_start" | "auto_retry_end")
                    )
                {
                    continue;
                }
                pump.push_stdout_chunk(&line(event), 3).await.unwrap();
            }
            let reopened = Reopened::new(&fixture);
            for database in [&fixture.database, &reopened.database] {
                let connection = database.connection();
                let page =
                    load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
                let turn = &page.turns.as_slice()[0];
                assert_eq!(turn.state, TurnState::Failed);
                assert_eq!(
                    turn.terminal_reason
                        .as_ref()
                        .and_then(|reason| reason.message.as_deref()),
                    Some(expected)
                );
                let errors = turn
                    .items
                    .as_slice()
                    .iter()
                    .filter_map(|item| match &item.content {
                        ItemContent::Error {
                            kind,
                            code,
                            message,
                            ..
                        } => {
                            assert_eq!(*kind, Some(ErrorKind::Attempt));
                            assert_eq!(code.as_str(), "Attempt failed (attempt 1)");
                            Some(message.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(errors, if retry { vec![expected] } else { Vec::new() });
                let encoded = serde_json::to_string(&page).unwrap();
                for canary in canaries {
                    let escaped = serde_json::to_string(canary).unwrap();
                    assert!(
                        !encoded.contains(canary)
                            && !encoded.contains(&escaped[1..escaped.len() - 1]),
                        "leaked {canary:?} from {detail:?}"
                    );
                }
            }
            drop(reopened);
            fixture.finish().await;
        }
    }
}
