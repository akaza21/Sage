use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    ProbeLimits,
    candidates::{
        bounded_path_roots, known_install_roots_from_app_data, path_candidates_for_basename,
    },
    validation::{authentic_descendant_file, authentic_harness_file},
    version_probe::run_version_command,
};

const ADAPTER_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";
const ADAPTER_ENTRY: &str = "dist/index.js";
const PACKAGE_JSON: &str = "package.json";
const SDK_PACKAGE: &str = "@agentclientprotocol/sdk";
const PACKAGE_JSON_BYTES_MAX: u64 = 64 * 1024;
const HASHED_FILE_BYTES_MAX: u64 = 512 * 1024 * 1024;

/// All facts needed to construct the Node-hosted Claude ACP launch contract.
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

/// Discovery intentionally retains why a launch candidate cannot be made.
/// `PresentVersionUnreadable` covers bounded package-metadata, SDK, and Node
/// version reads: none are guessed from a shim or a filename.
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
    let Some(adapter_sha256) = sha256_file(&adapter.adapter_path) else {
        return ClaudeLaunchDiscovery::AdapterVersionUnreadable;
    };
    let Some(node_sha256) = sha256_file(&node_path) else {
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

fn npm_global_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        known_install_roots_from_app_data(std::env::var_os("APPDATA").as_deref())
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

fn discover_node_from(paths: Vec<PathBuf>, limits: ProbeLimits) -> Option<(PathBuf, String)> {
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

fn node_candidates() -> Vec<PathBuf> {
    let mut candidates = path_candidates_for_basename(node_basename());
    #[cfg(windows)]
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let root = PathBuf::from(program_files).join("nodejs");
        if bounded_path_roots([root.clone()]).len() == 1 {
            candidates.push(root.join(node_basename()));
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
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

fn read_json(path: &Path) -> Option<Value> {
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

fn required_package_text(value: &Value, key: &str) -> Option<String> {
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

#[allow(dead_code)]
pub(crate) fn sha256_file(path: &Path) -> Option<String> {
    let metadata = path.metadata().ok()?;
    if metadata.len() > HASHED_FILE_BYTES_MAX || metadata.len() == 0 {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
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
        // This fails under the former `parent()?.parent()?` reconstruction:
        // that produces `<npm>/node_modules`, then probes a duplicate
        // `node_modules/node_modules` segment. Passing the actual npm root
        // proves the scoped package layout is resolved correctly.
        let adapter = read_adapter(&adapter_root.0, &adapter_root.1).expect("adapter metadata");
        assert_eq!(adapter.adapter_path, adapter_root.1.join(ADAPTER_ENTRY));
        assert_eq!(adapter.adapter_version, "0.76.0");
        assert_eq!(adapter.acp_sdk_version, "1.4.0");
        assert_eq!(adapter.node_engine, ">=22");
        fs::remove_dir_all(root).expect("remove root");
    }

    #[cfg(windows)]
    #[test]
    fn standard_program_files_node_candidate_is_included() {
        let candidates = node_candidates();
        let expected = PathBuf::from(r"C:\Program Files\nodejs\node.exe");
        // This discriminates because omitting the standard Node install root
        // makes a PATH without Node unable to locate the installed runtime.
        assert!(candidates.iter().any(|candidate| candidate == &expected));
    }
}
