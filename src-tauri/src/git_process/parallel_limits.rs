//! Caps the parallel children a repository asks Git to start, never raising
//! them.
//!
//! `checkout.workers`, `fetch.parallel` and `submodule.fetchJobs` let a
//! repository's config start many Git children at once (a value of 0 or less
//! means one per CPU). On Windows they all run inside the call's Job Object,
//! whose process limit leaves room for [`MAX_PARALLEL_JOBS`] of them. A key
//! the repository sets above that, or to a per-CPU value, is passed back to
//! Git as `-c key=<capped value>`. A key it leaves unset gets no override, so
//! Git keeps its own default of one.
//!
//! The values come from repository admission: when the name-only listing it
//! already reads shows one of these keys, it reads their values once with
//! [`PROBE_ARGS`] and keeps the caps in its cached verdict, so a steady-state
//! call starts no extra process and they go stale exactly when the config
//! does. Includes are refused there, and global and system config are
//! cleared for every Git call.

use std::collections::HashMap;

use super::{GitOutput, process_tree::MAX_PARALLEL_JOBS};

/// Each capped key, as listed lowercase by `git config`, with its name.
const KEYS: [(&str, &str); 3] = [
    ("checkout.workers", "checkout.workers"),
    ("fetch.parallel", "fetch.parallel"),
    ("submodule.fetchjobs", "submodule.fetchJobs"),
];

/// Lists only the capped keys, NUL-separated.
pub(super) const PROBE_ARGS: [&str; 4] = [
    "config",
    "-z",
    "--get-regexp",
    r"^(checkout\.workers|fetch\.parallel|submodule\.fetchjobs)$",
];
pub(super) const MAX_PROBE_BYTES: usize = 64 * 1024;

/// Whether a `--name-only -z` listing sets any capped key, so their values
/// need reading at all.
pub(super) fn lists_any(listing: &[u8]) -> bool {
    listing.split(|byte| *byte == 0).any(|listed| {
        KEYS.iter()
            .any(|(key, _)| listed.eq_ignore_ascii_case(key.as_bytes()))
    })
}

/// The overrides a probe's result calls for. A probe that did not run or
/// failed fails safe: every key is held to one.
pub(super) fn from_probe(probe: Option<&GitOutput>) -> Vec<String> {
    let Some(probe) = probe.filter(|probe| !probe.stdout_truncated) else {
        return fail_safe();
    };
    // `git config --get-regexp` exits 1 when no key is set.
    if !probe.status.success() {
        return if probe.status.code() == Some(1) && probe.stdout.is_empty() {
            Vec::new()
        } else {
            fail_safe()
        };
    }
    overrides_for(&probe.stdout, cpu_count())
}

fn fail_safe() -> Vec<String> {
    KEYS.iter().map(|(_, name)| format!("{name}=1")).collect()
}

fn cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX))
}

/// Overrides for a `-z` listing of `key\nvalue` records; the last value of a
/// key wins, as in Git. Values within the cap are left alone.
fn overrides_for(listing: &[u8], cpus: u32) -> Vec<String> {
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
    KEYS.iter()
        .filter_map(|(key, name)| {
            let value = values.get(key.as_bytes())?;
            let capped = match value.and_then(parse_int) {
                // Zero or less means one per CPU.
                Some(requested) if requested <= 0 => cpus.min(MAX_PARALLEL_JOBS),
                Some(requested) if requested > i64::from(MAX_PARALLEL_JOBS) => MAX_PARALLEL_JOBS,
                Some(_) => return None,
                None => 1,
            };
            Some(format!("{name}={capped}"))
        })
        .collect()
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
