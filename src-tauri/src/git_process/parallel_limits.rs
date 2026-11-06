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
//! The values are read once per root with one `git config` call and cached
//! against the exact bytes of every config file Git would read there, so a
//! steady-state call starts no extra process. Global and system config are
//! already cleared for every Git call.

use std::{
    collections::HashMap,
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, PoisonError},
};

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
const MAX_CACHED_ROOTS: usize = 256;
/// Larger config files are not cached; they are probed on every call.
const MAX_STAMPED_FILE_BYTES: u64 = 256 * 1024;

/// What each config file Git would read at a root held: `None` is missing.
pub(super) type ConfigStamp = Vec<(PathBuf, Option<Vec<u8>>)>;

/// Per root: the config it was read from, and the overrides it called for.
type CachedLimits = (ConfigStamp, Vec<String>);

static CACHE: LazyLock<Mutex<HashMap<PathBuf, CachedLimits>>> = LazyLock::new(Mutex::default);

/// The cached overrides for `root`, if its config is unchanged.
pub(super) fn cached(root: &Path) -> Option<Vec<String>> {
    let stamp = stamp(root)?;
    let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .get(root)
        .filter(|(cached, _)| *cached == stamp)
        .map(|(_, overrides)| overrides.clone())
}

/// Remembers `overrides` for `root` against `stamp`, taken before the probe
/// so a change made while it ran is seen on the next call.
pub(super) fn store(root: &Path, stamp: ConfigStamp, overrides: &[String]) {
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if !cache.contains_key(root) && cache.len() >= MAX_CACHED_ROOTS {
        cache.clear();
    }
    cache.insert(root.to_path_buf(), (stamp, overrides.to_vec()));
}

/// The bytes of the config files Git reads at `root`: the `.git` entries
/// from `root` upwards until the first that exists, which must be a
/// directory. `None` when that cannot be fingerprinted cheaply (a `.git`
/// file, a link, an unreadable or oversized config), so nothing is cached.
pub(super) fn stamp(root: &Path) -> Option<ConfigStamp> {
    let mut stamp = Vec::new();
    for directory in root.ancestors() {
        let entry = directory.join(".git");
        match fs::symlink_metadata(&entry) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => stamp.push((entry, None)),
            Ok(metadata) if metadata.file_type().is_dir() => {
                for name in ["config", "config.worktree"] {
                    let file = entry.join(name);
                    let bytes = read_small(&file)?;
                    stamp.push((file, bytes));
                }
                return Some(stamp);
            }
            _ => return None,
        }
    }
    Some(stamp)
}

fn read_small(path: &Path) -> Option<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    let mut bytes = Vec::new();
    file.take(MAX_STAMPED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_STAMPED_FILE_BYTES).then_some(Some(bytes))
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
