use super::turn_failures::Reopened;
use super::*;
use crate::acp::bindings::pi::runtime::support::ATTEMPT_FAILURE_CODE;
use crate::acp::{
    intent::ErrorKind, lifecycle::TurnState, projection::ItemContent, store::load_chat_page,
};

#[tokio::test]
async fn pi_credentials_never_reopen_in_terminal_reasons_or_retry_items() {
    // Whole-page scans include generated UUIDs: use non-hex credential canaries.
    // Exact terminal/retry message assertions below still verify the redacted text.
    for (detail, expected, canaries) in [
        (
            r#"{"user":"Bob Smith","token":"multiword-canary"}"#,
            r#"{"user":"Bob Smith","token":[redacted]}"#,
            &["multiword-canary"][..],
        ),
        (
            r#"{"credentials":["canary-one","canary-two"],"count":4}"#,
            r#"{"credentials":[redacted],"count":4}"#,
            &["canary-one", "canary-two"][..],
        ),
        (
            r#"{"to\u006ben":"unicode-canary"}"#,
            r#"{"to\u006ben":[redacted]}"#,
            &["unicode-canary"][..],
        ),
        (
            r#"{\"token\":\"prefix\\\"tail-canary\"}"#,
            r#"{\"token\":[redacted]}"#,
            &["prefix", "tail-canary"][..],
        ),
        (
            r#"{"token":"token-canary","message":"https://user:secret@host/ failed"}"#,
            r#"{"token":[redacted],"message":"https://[redacted]@host/ failed"}"#,
            &["token-canary", "user:secret"][..],
        ),
        (
            r"C:\Users\John Paul Smith\.ssh\id failed",
            "[path] failed",
            &["John", "Paul", "Smith"][..],
        ),
        (
            "Bearer  private-token",
            "Bearer  [redacted]",
            &["private-token"][..],
        ),
        (
            "Bearer private\u{0000}token",
            "Bearer [redacted]",
            &["privatetoken"][..],
        ),
        (
            "Bearer\r\rprivate-token",
            "Bearer  [redacted]",
            &["private-token"][..],
        ),
        (
            "--token\r\rprivate-token",
            "--token  [redacted]",
            &["private-token"][..],
        ),
        (
            "Bearer\u{000b}\u{000c}private-token",
            "Bearer  [redacted]",
            &["private-token"][..],
        ),
        (
            "Authorization: Basic private-token",
            "Authorization: [redacted] [redacted]",
            &["private-token"][..],
        ),
        (
            "Authorization:Basic private-token",
            "[redacted] [redacted]",
            &["private-token"][..],
        ),
        (
            r#""failed Bearer private-token""#,
            r#""failed Bearer [redacted]""#,
            &["private-token"][..],
        ),
        (
            "Bearer \t\n private-token",
            "Bearer \t\n [redacted]",
            &["private-token"][..],
        ),
        (
            "Bearer \"private multi word\" denied",
            "Bearer [redacted] denied",
            &["private multi word"][..],
        ),
        (
            "Authorization:  Bearer  private-token",
            "Authorization:  [redacted]  [redacted]",
            &["private-token"][..],
        ),
        (
            "Authorization:Bearer private-token",
            "[redacted] [redacted]",
            &["private-token"][..],
        ),
        (
            "Authorization: \"Bearer private token\" denied",
            "Authorization: [redacted] denied",
            &["Bearer private token"][..],
        ),
        (
            "--token  private-token denied",
            "--token  [redacted] denied",
            &["private-token"][..],
        ),
        (
            "--password 'private multi word' denied",
            "--password [redacted] denied",
            &["private multi word"][..],
        ),
        (
            "--secret \"private multi word\" denied",
            "--secret [redacted] denied",
            &["private multi word"][..],
        ),
        (
            "--api-key  private-token denied",
            "--api-key  [redacted] denied",
            &["private-token"][..],
        ),
        (
            "KEY=\"private multi word\" denied",
            "[redacted] denied",
            &["private multi word"][..],
        ),
        (
            "KEY='private multi word' denied",
            "[redacted] denied",
            &["private multi word"][..],
        ),
        (
            "spring.datasource.password=hunter2",
            "[redacted]",
            &["hunter2"][..],
        ),
        (
            "env:TOKEN=credential-canary",
            "[redacted]",
            &["credential-canary"][..],
        ),
        (
            "key[0]=credential-canary",
            "[redacted]",
            &["credential-canary"][..],
        ),
        (
            "api.key=credential-canary",
            "[redacted]",
            &["credential-canary"][..],
        ),
        (
            "Authorization :  private-token",
            "Authorization :  [redacted]",
            &["private-token"][..],
        ),
        (
            r#"{"Authorization": "Basic dXNlcjpwYXNz"}"#,
            r#"{"Authorization": [redacted]}"#,
            &["dXNlcjpwYXNz"][..],
        ),
        (
            r#""x-api-key": "credential-canary""#,
            r#""x-api-key": [redacted]"#,
            &["credential-canary"][..],
        ),
        (
            r#"{"token":"credential-canary-123"}"#,
            r#"{"token":[redacted]}"#,
            &["credential-canary-123"][..],
        ),
        (
            "password: hunter2",
            "password: [redacted]",
            &["hunter2"][..],
        ),
        (
            "token:credential-canary",
            "[redacted]",
            &["credential-canary"][..],
        ),
        (
            "--token:credential-canary",
            "[redacted]",
            &["credential-canary"][..],
        ),
        (
            "--client-secret credential-canary",
            "--client-secret [redacted]",
            &["credential-canary"][..],
        ),
        (
            "--access-token credential-canary",
            "--access-token [redacted]",
            &["credential-canary"][..],
        ),
        (
            "curl -d @/home/u/secret",
            "curl -d [path]",
            &["/home/u/secret"][..],
        ),
        ("config:/home/u/x", "[path]", &["/home/u/x"][..]),
        (
            "https://user:secret@host/",
            "https://[redacted]@host/",
            &["user:secret"][..],
        ),
        (
            r"C:\Users\John Smith\.ssh\id",
            "[path]",
            &["John", "Smith"][..],
        ),
        (
            "--password 'hunter2 rejected; it isn't valid",
            "--password [redacted]",
            &["hunter2"][..],
        ),
        (
            r#"error: expected "KEY=" but got "x""#,
            r#"error: expected "KEY=" but got "x""#,
            &[][..],
        ),
        (r#"Bearer ""secret"#, "Bearer [redacted]", &["secret"][..]),
        (
            r#"KEY="attached-canary"tail-canary"#,
            "[redacted]",
            &["attached-canary", "tail-canary"][..],
        ),
        (
            "risk-free task-runner disk-full",
            "risk-free task-runner disk-full",
            &[][..],
        ),
        (
            "KEY=  \"private multi word\" denied",
            "[redacted]  [redacted] denied",
            &["private multi word"][..],
        ),
        (
            "KEY = \"private multi word\" denied",
            "KEY = [redacted] denied",
            &["private multi word"][..],
        ),
        (
            r#"{"user":"bob","token":"compact-canary"}"#,
            r#"{"user":"bob","token":[redacted]}"#,
            &["compact-canary"][..],
        ),
        (
            r#"{"error":{"message":"x","authorization":"Bearer nested-canary"}}"#,
            r#"{"error":{"message":"x","authorization":[redacted]}}"#,
            &["nested-canary"][..],
        ),
        (
            r#""headers":{"Authorization":"Bearer header-canary"}"#,
            r#""headers":{"Authorization":[redacted]}"#,
            &["header-canary"][..],
        ),
        (
            r#"{\"token\":\"escaped-canary\"}"#,
            r#"{\"token\":[redacted]}"#,
            &["escaped-canary"][..],
        ),
        (
            "Proxy-Authorization: Basic proxy-canary",
            "Proxy-Authorization: [redacted] [redacted]",
            &["proxy-canary"][..],
        ),
        (
            "--password 'it's quoted-canary' denied",
            "--password [redacted] denied",
            &["it's quoted-canary"][..],
        ),
        (
            "--password 'it's unclosed-canary continues",
            "--password [redacted]",
            &["unclosed-canary", "continues"][..],
        ),
        (
            r"Cannot open C:\a\b because C:\c is locked",
            "Cannot open [path] because [path] is locked",
            &[r"C:\a\b", r"C:\c"][..],
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
                            assert_eq!(code.as_str(), ATTEMPT_FAILURE_CODE);
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
