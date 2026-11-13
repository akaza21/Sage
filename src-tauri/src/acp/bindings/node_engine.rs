//! Whether a Node release meets a package's declared `engines.node` range.

use std::ops::Bound::{self, Excluded, Included, Unbounded};
use std::sync::LazyLock;

use regex::Regex;

/// Longest `engines.node` value Sage evaluates.
pub(crate) const RANGE_BYTES_MAX: usize = 256;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const NPM_WHITESPACE: &str = concat!(
    "\t\n\u{b}\u{c}\r \u{a0}\u{1680}",
    "\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}",
    "\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}",
    "\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}",
);

/// A release version: major, minor, patch.
pub(crate) type NodeVersion = (u64, u64, u64);

/// Evaluates npm ranges for release runtimes, including prerelease bounds.
pub(crate) fn satisfies(range: &str, node: NodeVersion) -> Option<bool> {
    let range = range.trim_matches(npm_whitespace);
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
    if text.contains('-') {
        return None;
    }
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
    let tokens = range_tokens(set);
    if let [from, dash, to] = tokens.as_slice()
        && dash == "-"
    {
        let from = Partial::parse(from)?;
        let to = Partial::parse(to)?;
        if !from.valid_comparator() || (!to.prerelease && !to.valid_comparator()) {
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

fn range_tokens(set: &str) -> Vec<String> {
    static TRIMS: LazyLock<[(Regex, &str); 3]> = LazyLock::new(|| {
        let compile = |pattern: &str| {
            Regex::new(&pattern.replace(r"\s", &format!("[{NPM_WHITESPACE}]")))
                .expect("npm range trim regex")
        };
        let numeric = r"(?:0|[1-9][0-9]*)";
        let nonnumeric = r"(?:[0-9]*[a-zA-Z-][a-zA-Z0-9-]*)";
        let prerelease = format!(r"(?:{nonnumeric}|{numeric})");
        let loose_id = format!(r"(?:{nonnumeric}|[0-9]+)");
        let build = r"(?:\+[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*)?";
        let loose =
            format!(r"[v=\s]*[0-9]+\.[0-9]+\.[0-9]+(?:-?{loose_id}(?:\.{loose_id})*)?{build}");
        let x = format!(r"(?:{numeric}|x|X|\*)");
        let partial =
            format!(r"[v=\s]*{x}(?:\.{x}(?:\.{x}(?:-{prerelease}(?:\.{prerelease})*)?{build})?)?");
        [
            (
                compile(&format!(r"(\s*)((?:<|>)?=?)\s*({loose}|{partial})")),
                "${1}${2}${3}",
            ),
            (compile(r"(\s*)~>?\s+"), "${1}~"),
            (compile(r"(\s*)\^\s+"), "${1}^"),
        ]
    });
    let mut normalized = set.to_owned();
    for (pattern, replacement) in TRIMS.iter() {
        normalized = pattern.replace_all(&normalized, *replacement).into_owned();
    }
    normalized
        .split(npm_whitespace)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn npm_whitespace(character: char) -> bool {
    NPM_WHITESPACE.contains(character)
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
            Self::Equal if version.prerelease => Interval::NONE,
            Self::Equal => Interval {
                low: Included(floor),
                high: version.upper_bound()?,
            },
            Self::GreaterEqual => Interval {
                low: Included(floor),
                high: Unbounded,
            },
            Self::Greater if version.is_full() && !version.prerelease => Interval {
                low: Excluded(floor),
                high: Unbounded,
            },
            Self::Greater if version.prerelease => Interval {
                low: Included(floor),
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
    prerelease: bool,
}

impl Partial {
    fn parse(text: &str) -> Option<Self> {
        let core = text.trim_start_matches(['v', '=']);
        let canonical_prefix = matches!(&text[..text.len() - core.len()], "" | "v");
        let text = core;
        let mut qualified = text.split('+');
        let core = qualified.next()?;
        for build in qualified {
            if !identifiers(build, false) {
                return None;
            }
        }
        let (core, prerelease) = match core.split_once('-') {
            Some((core, tag)) if core.split('.').count() == 3 && identifiers(tag, true) => {
                (core, true)
            }
            Some(_) => return None,
            None => (core, false),
        };
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
            prerelease: prerelease && patch.is_some(),
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
            _ if self.prerelease => Excluded(self.floor()),
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

fn identifiers(text: &str, prerelease: bool) -> bool {
    text.split('.').all(|id| {
        !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !(prerelease
                && id.len() > 1
                && id.starts_with('0')
                && id.bytes().all(|b| b.is_ascii_digit()))
    })
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
