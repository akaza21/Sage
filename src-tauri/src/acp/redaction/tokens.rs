//! Credentials recognised by their shape alone, wherever they sit in a word,
//! such as inside a path.

/// Whether a provider token starts at `rest`.
fn provider_token_at(rest: &[u8]) -> bool {
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
        if provider_token_at(rest) {
            return true;
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
        pub(crate) aws_prefixes: usize,
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
