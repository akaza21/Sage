//! Whether a Node release meets a package's declared `engines.node` range.
//!
//! Packages declare their Node requirement in npm's range grammar, and a
//! harness release can move from `>=20` to `^20.19.0 || >=22.12.0` at any
//! time. Sage evaluates the grammar npm itself uses so a harness or Node
//! upgrade is not refused for the shape of its range. A range Sage cannot
//! read is never a pass.

/// Longest `engines.node` value Sage evaluates.
pub(crate) const RANGE_BYTES_MAX: usize = 256;

/// A release version: major, minor, patch.
pub(crate) type NodeVersion = (u64, u64, u64);

/// `Some(true)` when `node` is in `range`, `Some(false)` when it is not, and
/// `None` when the range is empty, a bare wildcard, too long, uses
/// prerelease versions, or is not npm range syntax.
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
    // A range of nothing but wildcards declares no requirement to check.
    let declared = !range
        .split("||")
        .all(|set| matches!(set.trim(), "*" | "x" | "X"));
    declared.then_some(satisfied)
}

/// A Node release written `22.18.0` or `v22.18.0`.
pub(crate) fn parse_node_version(text: &str) -> Option<NodeVersion> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.').map(number);
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// The versions from `low` up to, not including, `high`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    low: NodeVersion,
    high: Option<NodeVersion>,
}

impl Interval {
    const NONE: Self = Self {
        low: (0, 0, 0),
        high: Some((0, 0, 0)),
    };

    fn contains(self, version: NodeVersion) -> bool {
        version >= self.low && self.high.is_none_or(|high| version < high)
    }
}

/// One `||` alternative: a hyphen range or whitespace-separated comparators.
fn comparator_set(set: &str) -> Option<Vec<Interval>> {
    let tokens = joined_operators(set)?;
    match tokens.as_slice() {
        [] => None,
        [from, dash, to] if dash == "-" => {
            let from = partial(from)?;
            Some(vec![
                Interval {
                    low: from.floor(),
                    high: None,
                },
                Interval {
                    low: (0, 0, 0),
                    high: partial(to)?.next()?,
                },
            ])
        }
        _ => tokens.iter().map(|token| comparator(token)).collect(),
    }
}

/// Tokens with an operator written apart from its version (`>= 22`) joined.
fn joined_operators(set: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut pending: Option<&str> = None;
    for token in set.split_whitespace() {
        match pending.take() {
            Some(operator) => tokens.push(format!("{operator}{token}")),
            None if is_operator(token) => pending = Some(token),
            None => tokens.push(token.to_owned()),
        }
    }
    pending.is_none().then_some(tokens)
}

fn is_operator(token: &str) -> bool {
    matches!(token, ">=" | "<=" | ">" | "<" | "=" | "^" | "~")
}

fn comparator(token: &str) -> Option<Interval> {
    let (operator, version) = [">=", "<=", ">", "<", "=", "^", "~"]
        .iter()
        .find_map(|operator| token.strip_prefix(operator).map(|rest| (*operator, rest)))
        .unwrap_or(("", token));
    let version = partial(version)?;
    let floor = version.floor();
    Some(match operator {
        "" | "=" => Interval {
            low: floor,
            high: version.next()?,
        },
        ">=" => Interval {
            low: floor,
            high: None,
        },
        ">" => match version.next()? {
            Some(next) => Interval {
                low: next,
                high: None,
            },
            None => Interval::NONE,
        },
        "<" if version.major.is_none() => Interval::NONE,
        "<" => Interval {
            low: (0, 0, 0),
            high: Some(floor),
        },
        "<=" => Interval {
            low: (0, 0, 0),
            high: version.next()?,
        },
        "^" => Interval {
            low: floor,
            high: version.caret_ceiling()?,
        },
        _ => Interval {
            low: floor,
            high: version.tilde_ceiling()?,
        },
    })
}

/// A version whose trailing parts may be missing or `x`, `X` or `*`.
#[derive(Debug, Clone, Copy)]
struct Partial {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
}

fn partial(text: &str) -> Option<Partial> {
    let text = text.strip_prefix('v').unwrap_or(text);
    if text.is_empty() {
        return None;
    }
    let mut parts = [None; 3];
    let mut wildcard = false;
    for (index, part) in text.split('.').enumerate() {
        if index == 3 {
            return None;
        }
        if matches!(part, "x" | "X" | "*") {
            wildcard = true;
        } else if wildcard {
            return None;
        } else {
            parts[index] = Some(number(part)?);
        }
    }
    let [major, minor, patch] = parts;
    Some(Partial {
        major,
        minor,
        patch,
    })
}

impl Partial {
    fn floor(self) -> NodeVersion {
        (
            self.major.unwrap_or(0),
            self.minor.unwrap_or(0),
            self.patch.unwrap_or(0),
        )
    }

    /// The first version after every version this partial names: `None`
    /// inside for a bare wildcard, which names every version.
    fn next(self) -> Option<Option<NodeVersion>> {
        Some(match (self.major, self.minor, self.patch) {
            (None, _, _) => None,
            (Some(major), None, _) => Some((major.checked_add(1)?, 0, 0)),
            (Some(major), Some(minor), None) => Some((major, minor.checked_add(1)?, 0)),
            (Some(major), Some(minor), Some(patch)) => Some((major, minor, patch.checked_add(1)?)),
        })
    }

    /// `^`: changes that keep the leftmost nonzero part.
    fn caret_ceiling(self) -> Option<Option<NodeVersion>> {
        match (self.major, self.minor, self.patch) {
            (Some(0), Some(0), Some(_)) | (Some(0), Some(_), None) => self.next(),
            (Some(0), Some(minor), Some(_)) => Some(Some((0, minor.checked_add(1)?, 0))),
            (Some(major), _, _) => Some(Some((major.checked_add(1)?, 0, 0))),
            (None, _, _) => Some(None),
        }
    }

    /// `~`: patch changes when a minor is given, minor changes otherwise.
    fn tilde_ceiling(self) -> Option<Option<NodeVersion>> {
        match (self.major, self.minor) {
            (Some(major), Some(minor)) => Some(Some((major, minor.checked_add(1)?, 0))),
            _ => self.next(),
        }
    }
}

/// A decimal version part with no sign, leading zero, prerelease or build
/// suffix, as npm's strict grammar reads it.
fn number(part: &str) -> Option<u64> {
    if part.is_empty()
        || part.len() > 10
        || (part.len() > 1 && part.starts_with('0'))
        || !part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    part.parse().ok()
}

#[cfg(test)]
#[path = "node_engine_tests.rs"]
mod tests;
