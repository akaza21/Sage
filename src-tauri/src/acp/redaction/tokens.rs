//! Provider credentials recognised by their shape alone, wherever they sit in
//! a word, such as inside a path.

/// Whether a provider token starts at `rest`.
pub(crate) fn provider_token_at(rest: &[u8]) -> bool {
    starts_with_jwt(rest) || PREFIXED_TOKENS.iter().any(|token| token.starts(rest))
}

/// The span of a `user:password@host` path segment, as in `/srv/u:pw@db/x`.
pub(crate) fn segment_userinfo(token: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    for segment in token.split(['/', '\\']) {
        if let Some((userinfo, host)) = segment.split_once('@')
            && let Some((user, password)) = userinfo.split_once(':')
            && !user.is_empty()
            && !password.is_empty()
            && !host.is_empty()
            && !user.eq_ignore_ascii_case("mailto")
            && !is_digest(host)
        {
            return Some((start, start + userinfo.len()));
        }
        start += segment.len() + 1;
    }
    None
}

/// An image digest such as `sha256:…`, as in `nginx:1.25@sha256:…`.
fn is_digest(host: &str) -> bool {
    host.split_once(':').is_some_and(|(algorithm, hex)| {
        algorithm.starts_with("sha")
            && !hex.is_empty()
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

/// A provider token known by a literal prefix and at least `minimum`
/// following characters of its alphabet; only that minimum is ever scanned.
struct PrefixedToken {
    prefix: &'static [u8],
    minimum: usize,
    alphabet: fn(&u8) -> bool,
}

impl PrefixedToken {
    fn starts(&self, rest: &[u8]) -> bool {
        rest.strip_prefix(self.prefix).is_some_and(|tail| {
            tail.iter()
                .take(self.minimum)
                .take_while(|byte| (self.alphabet)(byte))
                .count()
                == self.minimum
        })
    }
}

const PREFIXED_TOKENS: &[PrefixedToken] = &[
    PrefixedToken {
        prefix: b"glpat-",
        minimum: 20,
        alphabet: is_base64url,
    },
    PrefixedToken {
        prefix: b"github_pat_",
        minimum: 22,
        alphabet: is_word,
    },
    PrefixedToken {
        prefix: b"AIza",
        minimum: 35,
        alphabet: is_base64url,
    },
    PrefixedToken {
        prefix: b"npm_",
        minimum: 36,
        alphabet: u8::is_ascii_alphanumeric,
    },
];

/// Longest JWT header scanned for the `.eyJ` payload start.
const JWT_HEADER_SCAN: usize = 1024;

fn is_base64url(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn is_word(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || *byte == b'_'
}

/// A JSON Web Token: a base64url `{"…` header, then a base64url `{"…` payload.
fn starts_with_jwt(rest: &[u8]) -> bool {
    let Some(header) = rest.strip_prefix(b"eyJ") else {
        return false;
    };
    let length = header
        .iter()
        .take(JWT_HEADER_SCAN)
        .take_while(|byte| is_base64url(byte))
        .count();
    length >= 7
        && header[length..]
            .strip_prefix(b".eyJ")
            .is_some_and(|payload| {
                payload
                    .iter()
                    .take(7)
                    .take_while(|byte| is_base64url(byte))
                    .count()
                    == 7
            })
}
