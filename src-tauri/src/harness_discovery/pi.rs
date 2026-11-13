//! Pi launch discovery: the npm-global Pi package and the Node
//! runtime that hosts the entrypoint its `pi` bin names. Paths stay
//! native-only.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{
    ProbeLimits,
    claude::{
        cached_sha256_file, discover_node_from, node_candidates, npm_global_roots, read_json,
        required_package_text,
    },
    validation::authentic_descendant_file,
};

/// Longest `bin.pi` entry read from the package manifest.
const PI_ENTRY_BYTES_MAX: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredPiLaunch {
    pub(crate) entrypoint_path: PathBuf,
    pub(crate) package_version: String,
    pub(crate) node_engine: String,
    pub(crate) node_path: PathBuf,
    pub(crate) node_version: String,
    pub(crate) entrypoint_sha256: String,
    pub(crate) node_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PiLaunchDiscovery {
    Absent,
    PackageUnreadable,
    NodeAbsent,
    NodeVersionUnreadable,
    Present(Box<DiscoveredPiLaunch>),
}

pub(crate) fn discover_pi_launch() -> PiLaunchDiscovery {
    discover_pi_launch_from(
        npm_global_roots(),
        node_candidates(),
        ProbeLimits::PRODUCTION,
    )
}

fn package_relative(name: impl AsRef<Path>) -> PathBuf {
    Path::new("node_modules")
        .join(crate::acp::bindings::pi::PI_PACKAGE)
        .join(name)
}

pub(super) fn discover_pi_launch_from(
    npm_roots: Vec<PathBuf>,
    node_paths: Vec<PathBuf>,
    limits: ProbeLimits,
) -> PiLaunchDiscovery {
    let Some(npm_root) = npm_roots
        .into_iter()
        .find(|root| authentic_descendant_file(root, package_relative("package.json")))
    else {
        return PiLaunchDiscovery::Absent;
    };
    let Some(package) = read_package(&npm_root) else {
        return PiLaunchDiscovery::PackageUnreadable;
    };
    // npm can install a private Node beside the shim; prefer it, then PATH.
    let bundled = npm_root.join(if cfg!(windows) { "node.exe" } else { "node" });
    let mut candidates = vec![bundled];
    candidates.extend(node_paths);
    let Some((node_path, node_version)) = discover_node_from(candidates.clone(), limits) else {
        return if candidates.iter().any(|path| path.is_file()) {
            PiLaunchDiscovery::NodeVersionUnreadable
        } else {
            PiLaunchDiscovery::NodeAbsent
        };
    };
    let entrypoint_path = npm_root.join(package_relative(&package.entry));
    let Some(entrypoint_sha256) = cached_sha256_file(&entrypoint_path) else {
        return PiLaunchDiscovery::PackageUnreadable;
    };
    let Some(node_sha256) = cached_sha256_file(&node_path) else {
        return PiLaunchDiscovery::NodeVersionUnreadable;
    };
    PiLaunchDiscovery::Present(Box::new(DiscoveredPiLaunch {
        entrypoint_path,
        package_version: package.version,
        node_engine: package.node_engine,
        node_path,
        node_version,
        entrypoint_sha256,
        node_sha256,
    }))
}

struct PiPackage {
    version: String,
    node_engine: String,
    /// The `pi` bin target, relative to the package root. Releases move it
    /// (1.x ships `dist/bundle/cli.js`), so it is read rather than assumed.
    entry: PathBuf,
}

/// Reads only the curated package identity: name, version, the `pi` bin
/// entry and the Node engine. Nothing is inferred from the shim.
fn read_package(npm_root: &Path) -> Option<PiPackage> {
    let manifest = read_json(&npm_root.join(package_relative("package.json")))?;
    let name = manifest.get("name").and_then(Value::as_str)?;
    if name != crate::acp::bindings::pi::PI_PACKAGE {
        return None;
    }
    let entry = manifest
        .get("bin")
        .and_then(|bin| bin.get("pi"))
        .and_then(Value::as_str)
        .and_then(package_entry)?;
    if !authentic_descendant_file(npm_root, package_relative(&entry)) {
        return None;
    }
    let engine = manifest
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(Value::as_str)
        .filter(|engine| !engine.is_empty() && engine.len() <= 64)?;
    Some(PiPackage {
        version: required_package_text(&manifest, "version")?,
        node_engine: engine.to_owned(),
        entry,
    })
}

/// A bin target as a relative `/`-separated path naming a JavaScript module
/// inside the package. Each component is a portable npm file name: ASCII
/// letters, digits, `.`, `_` and `-`, never `.` or `..` alone, so no drive,
/// stream, separator or parent can reach the path on any host.
fn package_entry(bin: &str) -> Option<PathBuf> {
    let relative = bin.strip_prefix("./").unwrap_or(bin);
    if relative.is_empty() || relative.len() > PI_ENTRY_BYTES_MAX {
        return None;
    }
    let plain = relative.split('/').all(|component| {
        !matches!(component, "" | "." | "..")
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    });
    let path = Path::new(relative);
    let module = matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("js" | "mjs" | "cjs")
    );
    (plain && module).then(|| path.to_path_buf())
}

#[cfg(test)]
#[path = "pi_tests.rs"]
mod tests;
