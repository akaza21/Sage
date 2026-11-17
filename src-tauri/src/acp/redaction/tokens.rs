//! Credentials recognised by their shape alone, wherever they sit in a word,
//! such as inside a path.

/// The span of a `user:password@host` path segment, as in `/srv/u:pw@db/x`.
/// Only a segment after a separator counts: a bare `12:00@noon` is prose.
pub(crate) fn segment_userinfo(token: &str) -> Option<(usize, usize)> {
    let (first, path) = token.split_once(['/', '\\'])?;
    let mut start = first.len() + 1;
    for segment in path.split(['/', '\\']) {
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

fn is_base64url(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn is_word(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || *byte == b'_'
}

/// Finds JSON Web Tokens: a base64url `{"…` header, then a base64url `{"…`
/// payload. Every `eyJ` inside one base64url run shares that run's end, so
/// each run is measured once however many header starts it holds.
#[derive(Default)]
struct JwtScan {
    run_end: usize,
}

/// The base64url encoding of a JSON object's opening `{"`.
const JWT_SEGMENT_START: &[u8] = b"eyJ";
/// Base64url characters required after each segment start.
const JWT_SEGMENT_MINIMUM: usize = 7;

impl JwtScan {
    fn starts_at(&mut self, bytes: &[u8], index: usize) -> bool {
        if !bytes[index..].starts_with(JWT_SEGMENT_START) {
            return false;
        }
        if index >= self.run_end {
            let run = bytes[index..]
                .iter()
                .inspect(|_| work::record(|work| work.jwt_run_bytes += 1))
                .take_while(|byte| is_base64url(byte))
                .count();
            self.run_end = index + run;
        }
        let header = self.run_end - index - JWT_SEGMENT_START.len();
        header >= JWT_SEGMENT_MINIMUM
            && bytes[self.run_end..]
                .strip_prefix(b".")
                .and_then(|payload| payload.strip_prefix(JWT_SEGMENT_START))
                .is_some_and(|payload| {
                    payload
                        .iter()
                        .take(JWT_SEGMENT_MINIMUM)
                        .take_while(|byte| is_base64url(byte))
                        .count()
                        == JWT_SEGMENT_MINIMUM
                })
    }
}

/// Longest AWS access key tail `contains_token` inspects after a prefix: the
/// 16 key characters plus one to reject a longer run.
pub(crate) const AWS_KEY_TAIL_SCAN: usize = 17;

pub(crate) fn contains_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut jwt = JwtScan::default();
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
        if jwt.starts_at(bytes, index) || PREFIXED_TOKENS.iter().any(|token| token.starts(rest)) {
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

/// Test-only accounting of the work the shape scanners do on this thread, so
/// tests can bound it by input size instead of by wall-clock time.
pub(crate) mod work {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct ScanWork {
        /// Outer-loop byte positions visited.
        pub(crate) positions: usize,
        pub(crate) aws_prefixes: usize,
        pub(crate) aws_tail_bytes: usize,
        pub(crate) jwt_run_bytes: usize,
        /// Value bytes `file_reference` read as a line reference.
        pub(crate) reference_bytes: usize,
    }

    #[cfg(test)]
    thread_local! {
        static WORK: std::cell::Cell<ScanWork> =
            const {
                std::cell::Cell::new(ScanWork {
                    positions: 0,
                    aws_prefixes: 0,
                    aws_tail_bytes: 0,
                    jwt_run_bytes: 0,
                    reference_bytes: 0,
                })
            };
    }

    #[cfg(test)]
    pub(crate) fn record(update: impl FnOnce(&mut ScanWork)) {
        WORK.with(|cell| {
            let mut work = cell.get();
            update(&mut work);
            cell.set(work);
        });
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub(crate) fn record(_update: impl FnOnce(&mut ScanWork)) {}

    /// Returns the work recorded on this thread since the last call.
    #[cfg(test)]
    pub(crate) fn take() -> ScanWork {
        WORK.with(std::cell::Cell::take)
    }
}
