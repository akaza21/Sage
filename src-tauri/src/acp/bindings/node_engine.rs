//! Whether a Node release meets a package's declared `engines.node` range.

use std::ops::Bound::{self, Excluded, Included, Unbounded};

#[path = "node_engine_range.rs"]
mod range;

/// Longest `engines.node` value Sage evaluates.
pub(crate) const RANGE_BYTES_MAX: usize = 256;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// A release version: major, minor, patch.
pub(crate) type NodeVersion = (u64, u64, u64);

/// Evaluates npm's strict ranges against release runtimes only.
pub(crate) fn satisfies(range: &str, node: NodeVersion) -> Option<bool> {
    let range = range.trim_matches(range::npm_whitespace);
    // Sage policy: an empty declaration is unavailable, and input is bounded.
    if range.is_empty() || range.len() > RANGE_BYTES_MAX {
        return None;
    }
    let range = range::normalize_whitespace(range);
    let mut satisfied = false;
    for set in range.split("||") {
        let intervals = range::comparator_set(set.trim_matches(range::npm_whitespace))?;
        satisfied |= intervals.iter().all(|interval| interval.contains(node));
    }
    Some(satisfied)
}

/// A Node release written `22.18.0` or `v22.18.0`.
pub(crate) fn parse_node_version(text: &str) -> Option<NodeVersion> {
    let text = text.trim();
    // Sage policy: prerelease Node runtimes are never admitted.
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
