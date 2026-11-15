use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[cfg(windows)]
use super::candidates::bounded_path_roots;
use super::{
    ProbeLimits,
    candidates::path_candidates_for_basename,
    validation::{authentic_descendant_file, authentic_harness_file},
    version_probe::run_version_command,
};

const ADAPTER_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";
const ADAPTER_ENTRY: &str = "dist/index.js";
const PACKAGE_JSON: &str = "package.json";
const SDK_PACKAGE: &str = "@agentclientprotocol/sdk";
const PACKAGE_JSON_BYTES_MAX: u64 = 64 * 1024;
const HASHED_FILE_BYTES_MAX: u64 = 512 * 1024 * 1024;
const DIGEST_CACHE_BYTES_MAX: u64 = 16 * 1024;
const DIGEST_CACHE_ENTRIES_MAX: usize = 32;

/// Paths are native-only and must never be projected to the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredClaudeLaunch {
    pub(crate) adapter_path: PathBuf,
    pub(crate) adapter_version: String,
    pub(crate) acp_sdk_version: String,
    pub(crate) node_engine: String,
    pub(crate) node_path: PathBuf,
    pub(crate) node_version: String,
    pub(crate) adapter_sha256: String,
    pub(crate) node_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClaudeLaunchDiscovery {
    AdapterAbsent,
    AdapterVersionUnreadable,
    NodeAbsent,
    NodeVersionUnreadable,
    Present(Box<DiscoveredClaudeLaunch>),
}

pub(crate) fn discover_claude_launch() -> ClaudeLaunchDiscovery {
    discover_claude_launch_from(
        npm_global_roots(),
        node_candidates(),
        ProbeLimits::PRODUCTION,
    )
}

fn discover_claude_launch_from(
    npm_roots: Vec<PathBuf>,
    node_paths: Vec<PathBuf>,
    limits: ProbeLimits,
) -> ClaudeLaunchDiscovery {
    let Some((npm_root, adapter_root)) = adapter_package_root_from(npm_roots) else {
        return ClaudeLaunchDiscovery::AdapterAbsent;
    };
    let Some(adapter) = read_adapter(&npm_root, &adapter_root) else {
        return ClaudeLaunchDiscovery::AdapterVersionUnreadable;
    };
    let Some((node_path, node_version)) = discover_node_from(node_paths.clone(), limits) else {
        return if node_candidate_exists(&node_paths) {
            ClaudeLaunchDiscovery::NodeVersionUnreadable
        } else {
            ClaudeLaunchDiscovery::NodeAbsent
        };
    };
    let Some(adapter_sha256) = cached_sha256_file(&adapter.adapter_path) else {
        return ClaudeLaunchDiscovery::AdapterVersionUnreadable;
    };
    let Some(node_sha256) = cached_sha256_file(&node_path) else {
        return ClaudeLaunchDiscovery::NodeVersionUnreadable;
    };
    ClaudeLaunchDiscovery::Present(Box::new(DiscoveredClaudeLaunch {
        adapter_path: adapter.adapter_path,
        adapter_version: adapter.adapter_version,
        acp_sdk_version: adapter.acp_sdk_version,
        node_engine: adapter.node_engine,
        node_path,
        node_version,
        adapter_sha256,
        node_sha256,
    }))
}

#[derive(Debug)]
struct AdapterMetadata {
    adapter_path: PathBuf,
    adapter_version: String,
    acp_sdk_version: String,
    node_engine: String,
}

fn adapter_package_root_from(npm_roots: Vec<PathBuf>) -> Option<(PathBuf, PathBuf)> {
    npm_roots
        .into_iter()
        .find(|root| {
            authentic_descendant_file(
                root,
                Path::new("node_modules")
                    .join(ADAPTER_PACKAGE)
                    .join(PACKAGE_JSON),
            )
        })
        .map(|root| {
            let adapter = root.join("node_modules").join(ADAPTER_PACKAGE);
            (root, adapter)
        })
}

pub(super) fn npm_global_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        super::candidates::known_install_roots_from_app_data(std::env::var_os("APPDATA").as_deref())
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Receives the npm root directly from discovery. Do not reconstruct it by
/// walking adapter parents: scoped npm packages add a layout segment.
fn read_adapter(npm_root: &Path, root: &Path) -> Option<AdapterMetadata> {
    let manifest_relative = Path::new("node_modules")
        .join(ADAPTER_PACKAGE)
        .join(PACKAGE_JSON);
    let entry_relative = Path::new("node_modules")
        .join(ADAPTER_PACKAGE)
        .join(ADAPTER_ENTRY);
    let sdk_relative = Path::new("node_modules")
        .join(ADAPTER_PACKAGE)
        .join("node_modules")
        .join(SDK_PACKAGE)
        .join(PACKAGE_JSON);
    if !authentic_descendant_file(npm_root, &manifest_relative)
        || !authentic_descendant_file(npm_root, &entry_relative)
        || !authentic_descendant_file(npm_root, &sdk_relative)
    {
        return None;
    }
    let manifest = read_json(&root.join(PACKAGE_JSON))?;
    let sdk_manifest = read_json(
        &root
            .join("node_modules")
            .join(SDK_PACKAGE)
            .join(PACKAGE_JSON),
    )?;
    Some(AdapterMetadata {
        adapter_path: root.join(ADAPTER_ENTRY),
        adapter_version: required_package_text(&manifest, "version")?,
        acp_sdk_version: required_package_text(&sdk_manifest, "version")?,
        node_engine: manifest
            .get("engines")
            .and_then(Value::as_object)
            .and_then(|engines| engines.get("node"))
            .and_then(Value::as_str)
            .filter(|value| valid_text(value))
            .map(str::to_owned)?,
    })
}

pub(super) fn discover_node_from(
    paths: Vec<PathBuf>,
    limits: ProbeLimits,
) -> Option<(PathBuf, String)> {
    paths.into_iter().find_map(|path| {
        authentic_harness_file(&path, node_basename())
            .then_some(path)
            .and_then(|path| {
                let output = run_version_command(&path, limits)?;
                let version = parse_node_version(&output.stdout)
                    .or_else(|| parse_node_version(&output.stderr))?;
                Some((path, version))
            })
    })
}

fn node_candidate_exists(paths: &[PathBuf]) -> bool {
    paths
        .iter()
        .any(|path| authentic_harness_file(path, node_basename()))
}

/// PATH first, as a shell would, so a version manager's active Node (nvm, fnm,
/// Volta) is the one Sage runs.
pub(super) fn node_candidates() -> Vec<PathBuf> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut candidates = path_candidates_for_basename(node_basename());
    #[cfg(windows)]
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let root = PathBuf::from(program_files).join("nodejs");
        if bounded_path_roots([root.clone()]).len() == 1 {
            candidates.push(root.join(node_basename()));
        }
    }
    first_occurrences(candidates)
}

fn first_occurrences(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

#[cfg(windows)]
fn node_basename() -> &'static str {
    "node.exe"
}
#[cfg(not(windows))]
fn node_basename() -> &'static str {
    "node"
}

fn parse_node_version(output: &super::version_probe::StreamOutput) -> Option<String> {
    let raw = std::str::from_utf8(&output.bytes).ok()?.trim();
    let version = raw.strip_prefix('v')?;
    valid_semver(version).then(|| version.to_owned())
}

pub(super) fn read_json(path: &Path) -> Option<Value> {
    let metadata = path.metadata().ok()?;
    if metadata.len() > PACKAGE_JSON_BYTES_MAX {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)
        .ok()?
        .take(PACKAGE_JSON_BYTES_MAX + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= PACKAGE_JSON_BYTES_MAX)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}

pub(super) fn required_package_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_semver(value))
        .map(str::to_owned)
}

fn valid_semver(value: &str) -> bool {
    let mut parts = value.split('.');
    valid_text(value)
        && parts.next().is_some_and(valid_component)
        && parts.next().is_some_and(valid_component)
        && parts.next().is_some_and(valid_component)
        && parts.next().is_none()
}

fn valid_component(value: &str) -> bool {
    !value.is_empty() && value.len() <= 10 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0')
}

pub(crate) fn sha256_file(path: &Path) -> Option<String> {
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;

        fs::OpenOptions::new()
            .read(true)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .open(path)
            .ok()?
    };
    #[cfg(not(windows))]
    let mut file = fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() > HASHED_FILE_BYTES_MAX || metadata.len() == 0 {
        return None;
    }
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        bytes = bytes.checked_add(read as u64)?;
        if bytes > metadata.len() || bytes > HASHED_FILE_BYTES_MAX {
            return None;
        }
        hash.update(&buffer[..read]);
    }
    (bytes == metadata.len()).then(|| format!("{:x}", hash.finalize()))
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct DigestCache {
    entries: Vec<DigestCacheEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DigestCacheEntry {
    path: PathBuf,
    length: u64,
    modified_seconds: u64,
    modified_nanos: u32,
    digest: String,
}

pub(crate) fn cached_sha256_file(path: &Path) -> Option<String> {
    let cache_path = digest_cache_path()?;
    digest_with_cache(path, &cache_path, sha256_file)
}

fn digest_cache_path() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("SAGE_DISPOSABLE_PROFILE_ROOT") {
        return Some(
            PathBuf::from(root)
                .join("durable")
                .join("claude-digest-cache-v1.json"),
        );
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| Some(std::env::temp_dir()))
        .map(|root| root.join("Sage").join("claude-digest-cache-v1.json"))
}

fn digest_with_cache(
    path: &Path,
    cache_path: &Path,
    hash: impl FnOnce(&Path) -> Option<String>,
) -> Option<String> {
    let path = path.canonicalize().ok()?;
    let metadata = path.metadata().ok()?;
    if metadata.len() == 0 || metadata.len() > HASHED_FILE_BYTES_MAX {
        return None;
    }
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let length = metadata.len();
    let modified_seconds = modified.as_secs();
    let modified_nanos = modified.subsec_nanos();
    let mut cache = read_digest_cache(cache_path);
    if let Some(entry) = cache.entries.iter().find(|entry| {
        entry.path == path
            && entry.length == length
            && entry.modified_seconds == modified_seconds
            && entry.modified_nanos == modified_nanos
    }) {
        return Some(entry.digest.clone());
    }
    let digest = hash(&path)?;
    cache.entries.retain(|entry| entry.path != path);
    cache.entries.push(DigestCacheEntry {
        path,
        length,
        modified_seconds,
        modified_nanos,
        digest: digest.clone(),
    });
    if cache.entries.len() > DIGEST_CACHE_ENTRIES_MAX {
        cache
            .entries
            .drain(0..cache.entries.len() - DIGEST_CACHE_ENTRIES_MAX);
    }
    write_digest_cache(cache_path, &cache);
    Some(digest)
}

fn read_digest_cache(path: &Path) -> DigestCache {
    let Ok(metadata) = path.metadata() else {
        return DigestCache::default();
    };
    if metadata.len() > DIGEST_CACHE_BYTES_MAX {
        return DigestCache::default();
    }
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_digest_cache(path: &Path, cache: &DigestCache) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(cache) else {
        return;
    };
    if bytes.len() as u64 > DIGEST_CACHE_BYTES_MAX {
        return;
    }
    if fs::create_dir_all(parent).is_ok() {
        let _ = fs::write(path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sage-claude-npm-layout-{nonce}"));
        fs::create_dir_all(&root).expect("create root");
        root
    }

    fn write_adapter_layout(npm_root: &Path) {
        let adapter = npm_root.join("node_modules").join(ADAPTER_PACKAGE);
        let sdk = adapter.join("node_modules").join(SDK_PACKAGE);
        fs::create_dir_all(adapter.join("dist")).expect("create adapter dist");
        fs::create_dir_all(&sdk).expect("create adapter sdk");
        fs::write(
            adapter.join(PACKAGE_JSON),
            r#"{"version":"0.76.0","engines":{"node":">=22"}}"#,
        )
        .expect("write adapter package");
        fs::write(adapter.join(ADAPTER_ENTRY), b"adapter").expect("write adapter entry");
        fs::write(sdk.join(PACKAGE_JSON), r#"{"version":"1.4.0"}"#).expect("write SDK package");
    }

    #[test]
    fn scoped_npm_layout_passes_adapter_metadata_resolution() {
        let root = scratch_root();
        let npm_root = root.join("npm");
        fs::create_dir_all(&npm_root).expect("create npm root");
        write_adapter_layout(&npm_root);
        let adapter_root = adapter_package_root_from(vec![npm_root.clone()]).expect("adapter root");
        let adapter = read_adapter(&adapter_root.0, &adapter_root.1).expect("adapter metadata");
        assert_eq!(adapter.adapter_path, adapter_root.1.join(ADAPTER_ENTRY));
        assert_eq!(adapter.adapter_version, "0.76.0");
        assert_eq!(adapter.acp_sdk_version, "1.4.0");
        assert_eq!(adapter.node_engine, ">=22");
        fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn node_candidates_keep_the_order_a_shell_would_pick_them_in() {
        let path = |name: &str| PathBuf::from(name).join(node_basename());
        assert_eq!(
            first_occurrences(vec![path("z-active"), path("a-older"), path("z-active")]),
            vec![path("z-active"), path("a-older")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn standard_program_files_node_candidate_is_included() {
        let candidates = node_candidates();
        let expected = PathBuf::from(r"C:\Program Files\nodejs\node.exe");
        assert!(candidates.iter().any(|candidate| candidate == &expected));
    }

    #[test]
    fn digest_cache_skips_unchanged_files_and_rehashes_metadata_changes() {
        let root = scratch_root();
        let node = root.join("node.exe");
        let cache = root.join("digest-cache.json");
        fs::write(&node, b"node").expect("write node");
        let digest = "a".repeat(64);
        let mut hashes = 0;

        assert_eq!(
            digest_with_cache(&node, &cache, |_| {
                hashes += 1;
                Some(digest.clone())
            }),
            Some(digest.clone())
        );
        assert_eq!(
            digest_with_cache(&node, &cache, |_| {
                hashes += 1;
                Some(digest.clone())
            }),
            Some(digest.clone())
        );
        assert_eq!(hashes, 1);

        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(&node, b"node").expect("rewrite node");
        assert_eq!(
            digest_with_cache(&node, &cache, |_| {
                hashes += 1;
                Some(digest.clone())
            }),
            Some(digest.clone())
        );
        assert_eq!(hashes, 2);

        fs::write(&node, b"node changed").expect("change node length");
        assert_eq!(
            digest_with_cache(&node, &cache, |_| {
                hashes += 1;
                Some(digest.clone())
            }),
            Some(digest.clone())
        );
        assert_eq!(hashes, 3);
        fs::remove_dir_all(root).expect("remove root");
    }
}
