use super::{Mode, redact};

#[path = "corpus_data.rs"]
mod data;
type Case<'a> = (&'a str, &'a [&'a str], &'a [&'a str], Tail, Permission);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Permission {
    Show,
    Hide,
}

#[derive(Clone, Copy)]
enum Tail {
    All,
    LeadingOnly,
    DiagnosticOnly,
}

impl Tail {
    fn kept_by(self, mode: Mode) -> bool {
        match self {
            Self::All => true,
            Self::LeadingOnly => false,
            Self::DiagnosticOnly => mode == Mode::Diagnostic,
        }
    }
}

#[test]
fn union_canaries_cover_every_mode() {
    #[rustfmt::skip]
    let command_cases: &[Case<'_>] = &[
        ("auth:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("key:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("p:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("pw:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("pass:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("session:abc", &["abc"], &[], Tail::All, Permission::Show),
        ("PGPASSWORD=hunter2", &["hunter2"], &[], Tail::All, Permission::Show),
        ("accesstoken=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("authtoken=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("apitoken=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("secretkey=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("privatekey=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("REDISPASSWORD=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("SECRETS=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("--passphrase opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("sessionid=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        ("JSESSIONID=opaque", &["opaque"], &[], Tail::All, Permission::Show),
        (r#"{\"token\":\"abc\"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{\"password\": \"a b\"}"#, &["a b"], &[], Tail::All, Permission::Show),
        (r#"curl -d "{\"token\":\"abc\"}""#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"password="a"bc"#, &["bc"], &[], Tail::All, Permission::Show),
        (r#"DB_PASSWORD="x"'y'"#, &["y"], &[], Tail::All, Permission::Show),
        ("KEY='-----BEGIN PRIVATE KEY-----\nMIIEprivate\n-----END PRIVATE KEY-----' tail", &["MIIEprivate"], &[], Tail::All, Permission::Show),
        (r#"{\"token\":\"a\\\" private body\"}"#, &["private body"], &[], Tail::All, Permission::Show),
        (r#"curl -d "{\"token\":\"a\\\" private body\"}""#, &["private body"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","auth":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","key":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","p":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","pw":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","pass":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","session":"abc"}"#, &["abc"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","password":1234}"#, &["1234"], &[], Tail::All, Permission::Show),
        (r#"{"user":"bob","pw":1234}"#, &["1234"], &[], Tail::All, Permission::Show),
        ("--token [redacted]abc", &["abc"], &[], Tail::All, Permission::Show),
        ("--token [redacted],abc", &["abc"], &[], Tail::All, Permission::Show),
        ("Authorization: foo [redacted]abc", &["abc"], &[], Tail::All, Permission::Show),
        ("cc -idirafter/abs/path", &["/abs/path"], &[], Tail::All, Permission::Hide),
        ("cc -imacros/abs/path", &["/abs/path"], &[], Tail::All, Permission::Hide),
        ("cc -include/abs/path", &["/abs/path"], &[], Tail::All, Permission::Hide),
        ("cc -isysroot/abs/path", &["/abs/path"], &[], Tail::All, Permission::Hide),
        ("cc -unknown/abs/path", &["/abs/path"], &[], Tail::All, Permission::Hide),
        (r"C:\Users\Jane Doe Smith\x tail", &["Jane", "Doe", "Smith"], &[], Tail::All, Permission::Hide),
        (r"C:\Users\John Smith", &[r"C:\Users\John"], &[], Tail::All, Permission::Hide),
        (r"C:\home\Jane Doe Smith\x tail", &["Jane", "Doe", "Smith"], &[], Tail::All, Permission::Hide),
        (r"cc -isystemC:\Users\Jane\inc", &["Jane", "inc"], &[], Tail::All, Permission::Hide),
        ("C:/Users/Jane Doe/x", &["Jane", "Doe", "/x"], &[], Tail::All, Permission::Hide),
        ("/c/Users/Jane Doe/x", &["Jane", "Doe", "/x"], &[], Tail::All, Permission::Hide),
        (r#"password={"a":"b","c":"d"}"#, &[r#""a":"b""#, r#""c":"d""#], &[], Tail::All, Permission::Show),
        ("TOKEN='private value'", &["private", "value"], &[], Tail::All, Permission::Show),
        (">/home/alice/x", &["alice", "/x"], &[], Tail::All, Permission::Hide),
        ("Author: Jane", &[], &["Author: Jane"], Tail::All, Permission::Show),
        ("Tokens: 1,234", &[], &["Tokens: 1,234"], Tail::All, Permission::Show),
        ("Session: 3", &[], &["Session: 3"], Tail::All, Permission::Show),
        ("p=0.05", &[], &["p=0.05"], Tail::All, Permission::Show),
        ("\"Key: the point is\"", &[], &["\"Key: the point is\""], Tail::All, Permission::Show),
        (r#"{"user":"bob","message":"O'Brien"}"#, &[], &[r#"{"user":"bob","message":"O'Brien"}"#], Tail::All, Permission::Show),
        ("cc -includeinclude.h", &[], &["cc -includeinclude.h"], Tail::All, Permission::Show),
        ("sk-ab", &[], &["sk-ab"], Tail::All, Permission::Show),
        ("sk-___", &[], &["sk-___"], Tail::All, Permission::Show),
        ("provided:sk-!", &[], &["provided:sk-!"], Tail::All, Permission::Show),
        ("Authentication: done", &[], &["Authentication: done"], Tail::All, Permission::Show),
        ("preauthorization: done ordinary prose", &[], &["preauthorization: done ordinary prose"], Tail::All, Permission::Show),
        ("Keyboard: US", &[], &["Keyboard: US"], Tail::All, Permission::Show),
        ("Author: O'Brien\nordinary output\nlast line", &[], &["Author: O'Brien", "ordinary output", "last line"], Tail::All, Permission::Hide),
        ("3 / 4", &[], &["3 / 4"], Tail::All, Permission::Hide),
        ("AKIAIOSFODNN7EXAMPLE", &["AKIAIOSFODNN7EXAMPLE"], &[], Tail::All, Permission::Show),
        ("Basic abc123", &["abc123"], &["Basic"], Tail::All, Permission::Show),
        ("mysql -psecret", &["secret"], &["mysql"], Tail::All, Permission::Show),
        ("curl -uuser:pass", &["user:pass"], &["curl"], Tail::All, Permission::Show),
        ("Credentials : see below", &[], &["Credentials : see below"], Tail::All, Permission::Show),
        ("Passwords : must be 8 chars", &[], &["Passwords : must be 8 chars"], Tail::All, Permission::Show),
        ("tokenizer=normal", &[], &["tokenizer=normal"], Tail::All, Permission::Show),
        ("secretary=normal", &[], &["secretary=normal"], Tail::All, Permission::Show),
        ("Passwords: must be 8 chars", &[], &["Passwords: must be 8 chars"], Tail::All, Permission::Show),
        ("Credentials: see below", &[], &["Credentials: see below"], Tail::All, Permission::Show),
        (r"https:\/\/alice:canary@host\/", &["alice", "canary"], &[], Tail::All, Permission::Show),
        ("secretValue=compound-canary", &["compound-canary"], &[], Tail::All, Permission::Show),
        ("password_value=compound-canary", &["compound-canary"], &[], Tail::All, Permission::Show),
        (r#"{"passwords":["plural-canary"]}"#, &["plural-canary"], &[], Tail::All, Permission::Show),
        (r#"mysql -pmy" private-canary""#, &["private-canary"], &["mysql"], Tail::All, Permission::Show),
        (r#"curl -umy" private-canary""#, &["private-canary"], &["curl"], Tail::All, Permission::Show),
        (r"\/c\/Users\/Jane Doe\/x", &["Jane", "Doe"], &[], Tail::All, Permission::Hide),
    ];
    let mut failures = Vec::new();
    data::diagnostic_cases(|cases| {
        for mode in [
            Mode::Prose,
            Mode::CommandOutput,
            Mode::Permission,
            Mode::Diagnostic,
        ] {
            for (case_index, (input, secrets, public, keeps_tail, permission)) in
                cases.iter().chain(command_cases).enumerate()
            {
                let wrapped = if mode == Mode::Diagnostic {
                    format!("Diagnostic before {input} diagnostic after")
                } else {
                    format!("Diagnostic before\n{input}\ndiagnostic after")
                };
                let actual = match redact("corpus", &wrapped, 4_000_000, mode) {
                    Ok(_) if mode == Mode::Permission && *permission == Permission::Hide => {
                        let end = input
                            .char_indices()
                            .nth(120)
                            .map_or(input.len(), |(index, _)| index);
                        failures.push(format!(
                            "Permission unexpectedly shown case {case_index}: {:?}",
                            &input[..end]
                        ));
                        continue;
                    }
                    Ok(actual) => actual,
                    Err(crate::acp::contract::ContractError::ForbiddenContent { .. })
                        if mode == Mode::Permission && *permission == Permission::Hide =>
                    {
                        continue;
                    }
                    Err(error) => {
                        let end = input
                            .char_indices()
                            .nth(120)
                            .map_or(input.len(), |(index, _)| index);
                        failures.push(format!(
                            "{mode:?} unexpectedly rejected case {case_index}: {:?}: {error:?}",
                            &input[..end]
                        ));
                        continue;
                    }
                };
                for secret in *secrets {
                    if actual.contains(secret) {
                        failures.push(format!(
                            "{mode:?} leaked {secret:?}: {input:?} -> {actual:?}"
                        ));
                    }
                }
                if !actual.contains("Diagnostic before") {
                    failures.push(format!(
                        "{mode:?} lost leading prose: {input:?} -> {actual:?}"
                    ));
                }
                if keeps_tail.kept_by(mode) && !actual.contains("diagnostic after") {
                    failures.push(format!(
                        "{mode:?} lost trailing prose: {input:?} -> {actual:?}"
                    ));
                }
                if mode == Mode::Diagnostic
                    && redact("corpus", &actual, 4_000_000, mode).unwrap() != actual
                {
                    failures.push(format!("not idempotent: {input:?} -> {actual:?}"));
                }
                if mode == Mode::Diagnostic
                    || (secrets.is_empty() && !public.iter().any(|prose| prose.contains("[path]")))
                {
                    for prose in *public {
                        if !actual.contains(prose) {
                            failures
                                .push(format!("{mode:?} lost {prose:?}: {input:?} -> {actual:?}"));
                        }
                    }
                }
            }
        }
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
