use super::Mode;

const SECRET_PARTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "credential",
    "privatekey",
    "apikey",
    "accesskey",
    "secretkey",
    "cookie",
    "sessionid",
];

pub(crate) fn sensitive_key(key: &str) -> bool {
    let key = key.trim_matches(['-', '\'', '"', '\x60', '{', '}', ':', '\\']);
    let key = key.strip_prefix("$env:").unwrap_or(key);
    let decoded = decode_key(key, false);
    let lower = decoded.to_ascii_lowercase();
    let normalized: String = lower
        .chars()
        .filter(|character| !matches!(character, '-' | '_' | '.' | '[' | ']'))
        .collect();
    let normalized = normalized.trim_end_matches(|character: char| character.is_ascii_digit());
    if matches!(
        normalized,
        "secrets"
            | "passwords"
            | "credentials"
            | "awsaccesskeyid"
            | "authorization"
            | "proxyauthorization"
    ) || SECRET_PARTS.iter().any(|part| normalized.contains(part))
        || normalized
            .match_indices("secret")
            .any(|(index, _)| !normalized[index + 6..].starts_with("ary"))
        || sensitive_token_key(&lower)
        || sensitive_token_key(normalized)
    {
        return true;
    }
    let bytes = decoded.as_bytes();
    let mut start = 0;
    for index in 0..=bytes.len() {
        let separator = bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'_' | b'-' | b'.' | b'\\' | b'"' | b'[' | b']'));
        let camel = index > start
            && bytes.get(index).is_some_and(u8::is_ascii_uppercase)
            && (bytes[index - 1].is_ascii_lowercase()
                || bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase));
        if index == bytes.len() || separator || camel {
            if [
                "p",
                "pass",
                "pw",
                "key",
                "pwd",
                "auth",
                "authorization",
                "session",
            ]
            .iter()
            .chain(SECRET_PARTS.iter())
            .any(|part| decoded[start..index].eq_ignore_ascii_case(part))
            {
                return true;
            }
            start = index + usize::from(separator);
        }
    }
    false
}

pub(crate) fn public_token_key(key: &str) -> bool {
    matches!(
        decode_key(key, false).to_ascii_lowercase().as_str(),
        "token_count"
            | "token-count"
            | "token_counts"
            | "token_count_total"
            | "tokenization"
            | "detokenize"
    )
}

fn sensitive_token_key(key: &str) -> bool {
    key.match_indices("token").any(|(index, _)| {
        let suffix = &key[index + 5..];
        !suffix.starts_with('s')
            && !suffix.starts_with("iz")
            && !matches!(
                suffix,
                "count"
                    | "_count"
                    | "-count"
                    | "counts"
                    | "_counts"
                    | "counttotal"
                    | "_count_total"
            )
    })
}

pub(crate) fn sensitive_assignment(key: &str, separator: char, value: &str) -> bool {
    let bare = key.trim_matches(['-', '\'', '"', '\x60', '{', '}', '\\']);
    if separator == ':' {
        if ["passwords", "credentials"]
            .iter()
            .any(|part| bare.eq_ignore_ascii_case(part))
        {
            return false;
        }
        if ["key", "session"]
            .iter()
            .any(|part| bare.eq_ignore_ascii_case(part))
            && (value.is_empty()
                || (bare.eq_ignore_ascii_case("session") && value.parse::<f64>().is_ok()))
        {
            return false;
        }
    }
    if bare == "p" && value.parse::<f64>().is_ok() {
        return false;
    }
    sensitive_key(key)
}

pub(crate) fn sensitive_flag(flag: &str) -> bool {
    [
        "-u",
        "--user",
        "--proxy-user",
        "--userpwd",
        "--username",
        "--proxy-username",
    ]
    .iter()
    .any(|candidate| flag.eq_ignore_ascii_case(candidate))
}

pub(crate) fn is_sensitive_prose_flag(input: &str) -> bool {
    let flag = input.trim_matches(['"', '\'', '\x60', ',', ';']);
    flag.starts_with('-')
        && !attached_short_value(flag)
        && !flag.contains(['=', ':'])
        && (sensitive_flag(flag) || sensitive_key(flag))
}

pub(crate) fn is_sensitive_prose_token(input: &str) -> bool {
    let token = input.trim_matches(['"', '\'', '\x60', '(', ')', '[', ']', '{', '}', ',', ';']);
    let assigned = token.split(['?', '&']).any(|part| {
        part.split_once('=')
            .or_else(|| part.split_once(':'))
            .is_some_and(|(key, value)| {
                !value.is_empty()
                    && sensitive_assignment(key, if part.contains('=') { '=' } else { ':' }, value)
            })
    });
    assigned
        || inline_auth(token)
        || attached_short_value(token)
        || url_userinfo(token).is_some()
        || contains_token(token)
}

/// Longest AWS access key tail `contains_token` inspects after a prefix: the
/// 16 key characters plus one to reject a longer run.
pub(crate) const AWS_KEY_TAIL_SCAN: usize = 17;

pub(crate) fn contains_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    for index in 0..bytes.len() {
        work::record(|work| work.positions += 1);
        if index > 0 && bytes[index - 1].is_ascii_alphabetic() {
            continue;
        }
        let rest = &bytes[index..];
        if rest.starts_with(b"AKIA") || rest.starts_with(b"ASIA") {
            work::record(|work| work.aws_prefixes += 1);
            let tail = rest[4..]
                .iter()
                .take(AWS_KEY_TAIL_SCAN)
                .inspect(|_| work::record(|work| work.aws_tail_bytes += 1))
                .take_while(|byte| byte.is_ascii_alphanumeric());
            let (length, uppercase) = tail.fold((0usize, true), |(length, uppercase), byte| {
                (
                    length + 1,
                    uppercase && (byte.is_ascii_uppercase() || byte.is_ascii_digit()),
                )
            });
            if length == 16 && uppercase {
                return true;
            }
            continue;
        }
        let prefix = if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"sk-") {
            3
        } else if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b"ghp_") {
            4
        } else if rest.len() >= 5
            && rest[..3].eq_ignore_ascii_case(b"xox")
            && rest[3].is_ascii_alphabetic()
            && rest[4] == b'-'
        {
            5
        } else {
            continue;
        };
        let tail = rest[prefix..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        let (length, alphanumeric) = tail.fold((0usize, false), |(length, alphanumeric), byte| {
            (length + 1, alphanumeric || byte.is_ascii_alphanumeric())
        });
        if length >= 3 && alphanumeric {
            return true;
        }
    }
    false
}

/// Test-only accounting of the work `contains_token` does on this thread, so
/// tests can bound it by input size instead of by wall-clock time.
pub(crate) mod work {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct ContainsTokenWork {
        /// Outer-loop byte positions visited.
        pub(crate) positions: usize,
        /// Positions that start with an AWS access key prefix.
        pub(crate) aws_prefixes: usize,
        /// Bytes inspected after those prefixes.
        pub(crate) aws_tail_bytes: usize,
    }

    #[cfg(test)]
    thread_local! {
        static WORK: std::cell::Cell<ContainsTokenWork> =
            const { std::cell::Cell::new(ContainsTokenWork { positions: 0, aws_prefixes: 0, aws_tail_bytes: 0 }) };
    }

    #[cfg(test)]
    pub(crate) fn record(update: impl FnOnce(&mut ContainsTokenWork)) {
        WORK.with(|cell| {
            let mut work = cell.get();
            update(&mut work);
            cell.set(work);
        });
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub(crate) fn record(_update: impl FnOnce(&mut ContainsTokenWork)) {}

    /// Returns the work recorded on this thread since the last call.
    #[cfg(test)]
    pub(crate) fn take() -> ContainsTokenWork {
        WORK.with(std::cell::Cell::take)
    }
}

pub(crate) fn url_userinfo(token: &str) -> Option<(usize, usize)> {
    let start = ["://", r":\/\/"]
        .iter()
        .find_map(|delimiter| token.find(delimiter).map(|index| index + delimiter.len()))?;
    let authority = token[start..].split(['/', '?', '#']).next()?;
    let end = start + authority.rfind('@')?;
    Some((start, end))
}

pub(crate) fn decode_key(key: &str, escaped: bool) -> String {
    let decoded = if escaped {
        key.replace("\\\\", "\\")
    } else {
        key.to_owned()
    };
    let mut characters = decoded.chars();
    let mut output = String::with_capacity(decoded.len());
    while let Some(character) = characters.next() {
        if character == '\\' && characters.as_str().starts_with('u') {
            let digits = characters.as_str().as_bytes();
            if digits.len() >= 5 && digits[1..5].iter().all(u8::is_ascii_hexdigit) {
                let code = u32::from_str_radix(&characters.as_str()[1..5], 16).unwrap();
                if let Some(decoded) = char::from_u32(code) {
                    output.push(decoded);
                    for _ in 0..5 {
                        characters.next();
                    }
                    continue;
                }
            }
        }
        if character == '\\'
            && let Some(escaped) = characters.clone().next()
        {
            let decoded = match escaped {
                '"' | '\\' | '/' => Some(escaped),
                'n' => Some('\n'),
                'r' => Some('\r'),
                't' => Some('\t'),
                'b' => Some('\u{0008}'),
                'f' => Some('\u{000c}'),
                _ => None,
            };
            if let Some(decoded) = decoded {
                output.push(decoded);
                characters.next();
                continue;
            }
        }
        output.push(character);
    }
    output
}

pub(crate) fn auth_scheme(value: &str) -> bool {
    classify_scheme(value).is_some()
}

#[derive(Clone, Copy)]
enum AuthScheme {
    Bearer,
    Basic,
    Token,
    Digest,
    Negotiate,
}

fn classify_scheme(value: &str) -> Option<AuthScheme> {
    [
        ("bearer", AuthScheme::Bearer),
        ("basic", AuthScheme::Basic),
        ("token", AuthScheme::Token),
        ("digest", AuthScheme::Digest),
        ("negotiate", AuthScheme::Negotiate),
    ]
    .into_iter()
    .find_map(|(name, scheme)| value.eq_ignore_ascii_case(name).then_some(scheme))
}

pub(crate) fn auth_marker(value: &str, mode: Mode) -> bool {
    let value = if mode == Mode::Permission {
        value
    } else {
        value.trim_matches(['"', '\'', '\x60', '(', ')', '[', ']', '{', '}', ',', ';'])
    };
    let scheme = classify_scheme(value.trim_end_matches(':'));
    if mode == Mode::Permission {
        matches!(scheme, Some(AuthScheme::Bearer | AuthScheme::Basic))
    } else {
        scheme.is_some()
    }
}

pub(crate) fn auth_continuation(value: &str, mode: Mode) -> bool {
    let scheme = classify_scheme(value);
    if mode == Mode::Permission {
        matches!(
            scheme,
            Some(AuthScheme::Bearer | AuthScheme::Basic | AuthScheme::Token)
        )
    } else {
        scheme.is_some()
    }
}

pub(crate) fn assigned_auth_marker(value: &str, mode: Mode) -> bool {
    let assignment = if mode == Mode::Permission {
        value.rsplit_once('=')
    } else {
        value.split_once('=')
    };
    assignment.is_some_and(|(_, scheme)| {
        if mode == Mode::Permission {
            matches!(classify_scheme(scheme), Some(AuthScheme::Bearer))
        } else {
            auth_marker(scheme, mode)
        }
    })
}

pub(crate) fn inline_auth(value: &str) -> bool {
    value.split_once(':').is_some_and(|(kind, credential)| {
        auth_marker(kind, Mode::Permission) && !credential.is_empty()
    })
}

pub(crate) fn sensitive_header(body: &str, mode: Mode) -> bool {
    let body = body
        .strip_prefix("-H")
        .or_else(|| body.strip_prefix("-h"))
        .unwrap_or(body);
    let body = body.trim_start_matches(|character| {
        matches!(character, '\'' | '"' | '\x60') || (mode == Mode::Permission && character == '{')
    });
    body.split_once(':').is_some_and(|(name, _)| {
        let name = if mode == Mode::Permission {
            name.trim_end_matches(['\'', '"', '\x60'])
        } else {
            name
        };
        sensitive_header_name(name, mode)
    })
}

pub(crate) fn sensitive_header_name(name: &str, mode: Mode) -> bool {
    ["authorization", "proxy-authorization"]
        .iter()
        .any(|header| name.eq_ignore_ascii_case(header))
        || (mode == Mode::Permission
            && ["cookie", "x-api-key", "x-auth-token"]
                .iter()
                .any(|header| name.eq_ignore_ascii_case(header)))
}

pub(crate) fn quoted_assignment_syntax(body: &str) -> bool {
    body.len() > 2
        && body.starts_with(['\'', '"'])
        && body.ends_with(['\'', '"'])
        && body[..body.len() - 1].ends_with('=')
}

pub(crate) fn attached_short_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() > 2
        && bytes[0] == b'-'
        && matches!(bytes[1], b'p' | b'P' | b'u' | b'U')
        && !value.contains('=')
        && !["-password", "-pass", "-pw", "-passwd", "-pwd"]
            .iter()
            .any(|flag| value.eq_ignore_ascii_case(flag))
}
