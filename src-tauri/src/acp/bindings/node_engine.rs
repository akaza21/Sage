//! Whether a Node release meets a package's declared `engines.node` range.

use std::ops::Bound::{self, Excluded, Included, Unbounded};

/// Longest `engines.node` value Sage evaluates.
pub(crate) const RANGE_BYTES_MAX: usize = 256;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// A release version: major, minor, patch.
pub(crate) type NodeVersion = (u64, u64, u64);

/// Evaluates npm ranges for release versions. Empty declarations, oversized or
/// invalid ranges, and explicit prerelease requirements remain unavailable.
pub(crate) fn satisfies(range: &str, node: NodeVersion) -> Option<bool> {
    let range = range.trim();
    if range.is_empty() || range.len() > RANGE_BYTES_MAX {
        return None;
    }
    let mut satisfied = false;
    for set in range.split("||") {
        let intervals = comparator_set(set)?;
        satisfied |= intervals.iter().all(|interval| interval.contains(node));
    }
    Some(satisfied)
}

/// A Node release written `22.18.0` or `v22.18.0`.
pub(crate) fn parse_node_version(text: &str) -> Option<NodeVersion> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.').map(number);
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

struct Interval {
    low: Bound<NodeVersion>,
    high: Bound<NodeVersion>,
}

impl Interval {
    const ANY: Self = Self {
        low: Unbounded,
        high: Unbounded,
    };
    const NONE: Self = Self {
        low: Unbounded,
        high: Excluded((0, 0, 0)),
    };

    fn contains(&self, version: NodeVersion) -> bool {
        let above = match self.low {
            Included(low) => version >= low,
            Excluded(low) => version > low,
            Unbounded => true,
        };
        let below = match self.high {
            Included(high) => version <= high,
            Excluded(high) => version < high,
            Unbounded => true,
        };
        above && below
    }
}

fn comparator_set(set: &str) -> Option<Vec<Interval>> {
    let tokens = range_tokens(set)?;
    if let [from, dash, to] = tokens.as_slice()
        && dash == "-"
    {
        let from = Partial::parse(from)?;
        let to = Partial::parse(to)?;
        if !from.valid_comparator() || !to.valid_comparator() {
            return None;
        }
        return Some(vec![Interval {
            low: Included(from.floor()),
            high: to.upper_bound()?,
        }]);
    }
    tokens
        .iter()
        .map(|token| {
            let (operator, version) = Operator::parse(token);
            operator.expand(Partial::parse(version)?)
        })
        .collect()
}

fn range_tokens(set: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut pending = String::new();
    for token in set.split_whitespace() {
        pending.push_str(token);
        if !pending
            .bytes()
            .all(|byte| matches!(byte, b'<' | b'>' | b'=' | b'~' | b'^'))
        {
            tokens.push(std::mem::take(&mut pending));
        }
    }
    pending.is_empty().then_some(tokens)
}

#[derive(Clone, Copy)]
enum Operator {
    Equal,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Tilde,
    Caret,
}

impl Operator {
    fn parse(token: &str) -> (Self, &str) {
        for (prefix, operator) in [
            (">=", Self::GreaterEqual),
            ("<=", Self::LessEqual),
            ("~>", Self::Tilde),
            (">", Self::Greater),
            ("<", Self::Less),
            ("=", Self::Equal),
            ("~", Self::Tilde),
            ("^", Self::Caret),
        ] {
            if let Some(rest) = token.strip_prefix(prefix) {
                return (operator, rest);
            }
        }
        (Self::Equal, token)
    }

    fn expand(self, version: Partial) -> Option<Interval> {
        if !version.valid_comparator() && !matches!(self, Self::Tilde | Self::Caret) {
            return None;
        }
        let floor = version.floor();
        let interval = match self {
            Self::Equal => Interval {
                low: Included(floor),
                high: version.upper_bound()?,
            },
            Self::GreaterEqual => Interval {
                low: Included(floor),
                high: Unbounded,
            },
            Self::Greater if version.is_full() => Interval {
                low: Excluded(floor),
                high: Unbounded,
            },
            Self::Greater => match version.upper_bound()? {
                Excluded(next) => Interval {
                    low: Included(next),
                    high: Unbounded,
                },
                _ => Interval::NONE,
            },
            Self::Less if version.major.is_none() => Interval::NONE,
            Self::Less => Interval {
                low: Unbounded,
                high: Excluded(floor),
            },
            Self::LessEqual => Interval {
                low: Unbounded,
                high: version.upper_bound()?,
            },
            Self::Tilde | Self::Caret if version.major.is_none() => Interval::ANY,
            Self::Tilde => Interval {
                low: Included(floor),
                high: Excluded(version.ceiling(if version.minor.is_some() { 1 } else { 0 })?),
            },
            Self::Caret => {
                let index = match (version.major, version.minor, version.patch) {
                    (Some(0), Some(0), Some(_)) => 2,
                    (Some(0), Some(_), _) => 1,
                    _ => 0,
                };
                Interval {
                    low: Included(floor),
                    high: Excluded(version.ceiling(index)?),
                }
            }
        };
        Some(interval)
    }
}

#[derive(Clone, Copy)]
struct Partial {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    canonical_prefix: bool,
}

impl Partial {
    fn parse(text: &str) -> Option<Self> {
        let core = text.trim_start_matches(['v', '=']);
        let canonical_prefix = matches!(&text[..text.len() - core.len()], "" | "v");
        let text = core;
        let mut qualified = text.split('+');
        let core = qualified.next()?;
        for build in qualified {
            if !build.split('.').all(|identifier| {
                !identifier.is_empty()
                    && identifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }) {
                return None;
            }
        }
        let mut parts = [None; 3];
        let mut wildcard = false;
        for (index, part) in core.split('.').enumerate() {
            if index == 3 {
                return None;
            }
            if matches!(part, "x" | "X" | "*") {
                wildcard = true;
            } else {
                let value = number(part)?;
                if !wildcard {
                    parts[index] = Some(value);
                }
            }
        }
        let [major, minor, patch] = parts;
        Some(Self {
            major,
            minor,
            patch,
            canonical_prefix,
        })
    }

    fn floor(self) -> NodeVersion {
        (
            self.major.unwrap_or(0),
            self.minor.unwrap_or(0),
            self.patch.unwrap_or(0),
        )
    }

    fn is_full(self) -> bool {
        self.patch.is_some()
    }

    fn valid_comparator(self) -> bool {
        !self.is_full() || self.canonical_prefix
    }

    fn upper_bound(self) -> Option<Bound<NodeVersion>> {
        Some(match (self.major, self.minor, self.patch) {
            (None, _, _) => Unbounded,
            (_, None, _) => Excluded(self.ceiling(0)?),
            (_, _, None) => Excluded(self.ceiling(1)?),
            _ => Included(self.floor()),
        })
    }

    fn ceiling(self, index: usize) -> Option<NodeVersion> {
        let (major, minor, patch) = self.floor();
        let mut parts = [major, minor, patch];
        parts[index] = parts[index]
            .checked_add(1)
            .filter(|n| *n <= MAX_SAFE_INTEGER)?;
        parts[index + 1..].fill(0);
        Some((parts[0], parts[1], parts[2]))
    }
}

fn number(part: &str) -> Option<u64> {
    if part.is_empty()
        || (part.len() > 1 && part.starts_with('0'))
        || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    part.parse()
        .ok()
        .filter(|number| *number <= MAX_SAFE_INTEGER)
}

#[cfg(test)]
#[path = "node_engine_tests.rs"]
mod tests;
