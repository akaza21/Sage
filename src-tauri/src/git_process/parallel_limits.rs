//! `checkout.workers`, `fetch.parallel` and `submodule.fetchJobs` let a
//! repository's config start many Git children at once. On Windows they all
//! run inside the call's Job Object, whose process limit leaves room for
//! [`MAX_PARALLEL_JOBS`] of them, so every admitted call pins all three with
//! `-c`, which wins over every config file. Sage's Git never recurses into
//! submodules (see `SUBMODULE_ISOLATION`), so no fetch fans out further.
//!
//! Repository admission reads the values and keeps them in its cached
//! verdict, so a steady-state call starts no extra process and they go stale
//! exactly when the config does.

use std::collections::HashMap;

use super::{GitOutput, process_tree::MAX_PARALLEL_JOBS};

/// Each capped key, as listed lowercase by `git config`, with its name.
const KEYS: [(&str, &str); 3] = [
    ("checkout.workers", "checkout.workers"),
    ("fetch.parallel", "fetch.parallel"),
    ("submodule.fetchjobs", "submodule.fetchJobs"),
];
const GIT_DEFAULT: u32 = 1;

pub(super) const PROBE_ARGS: [&str; 4] = [
    "config",
    "-z",
    "--get-regexp",
    r"^(checkout\.workers|fetch\.parallel|submodule\.fetchjobs)$",
];
pub(super) const MAX_PROBE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ParallelLimits([u32; 3]);

impl Default for ParallelLimits {
    fn default() -> Self {
        Self([GIT_DEFAULT; 3])
    }
}

impl ParallelLimits {
    /// A probe that did not run or failed fails safe: every key is held to one.
    pub(super) fn from_probe(probe: Option<&GitOutput>) -> Self {
        let Some(probe) = probe.filter(|probe| !probe.stdout_truncated) else {
            return Self::default();
        };
        // `git config --get-regexp` exits 1 when no key is set.
        if !probe.status.success() {
            return Self::default();
        }
        Self::from_listing(&probe.stdout, cpu_count())
    }

    pub(super) fn overrides(self) -> impl Iterator<Item = String> {
        KEYS.iter()
            .zip(self.0)
            .map(|((_, name), value)| format!("{name}={value}"))
    }

    /// The last value of a key wins, as in Git.
    fn from_listing(listing: &[u8], cpus: u32) -> Self {
        let mut values = HashMap::new();
        for record in listing
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let (key, value) = match record.iter().position(|byte| *byte == b'\n') {
                Some(newline) => (&record[..newline], Some(&record[newline + 1..])),
                None => (record, None),
            };
            values.insert(key.to_ascii_lowercase(), value);
        }
        Self(KEYS.map(|(key, _)| {
            match values.get(key.as_bytes()) {
                None => GIT_DEFAULT,
                Some(value) => match value.and_then(parse_int) {
                    // Zero or less means one per CPU.
                    Some(requested) if requested <= 0 => cpus.min(MAX_PARALLEL_JOBS),
                    Some(requested) => u32::try_from(requested)
                        .unwrap_or(u32::MAX)
                        .min(MAX_PARALLEL_JOBS),
                    None => GIT_DEFAULT,
                },
            }
        }))
    }
}

pub(super) fn lists_any(listing: &[u8]) -> bool {
    listing.split(|byte| *byte == 0).any(|listed| {
        KEYS.iter()
            .any(|(key, _)| listed.eq_ignore_ascii_case(key.as_bytes()))
    })
}

fn cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX))
}

/// Git's integer syntax: an optional sign, digits, and an optional `k`, `m`
/// or `g` multiplier.
fn parse_int(value: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(value).ok()?.trim();
    let (digits, multiplier) = match text.chars().last()?.to_ascii_lowercase() {
        'k' => (&text[..text.len() - 1], 1_i64 << 10),
        'm' => (&text[..text.len() - 1], 1 << 20),
        'g' => (&text[..text.len() - 1], 1 << 30),
        _ => (text, 1),
    };
    digits.parse::<i64>().ok()?.checked_mul(multiplier)
}

#[cfg(test)]
mod tests;
