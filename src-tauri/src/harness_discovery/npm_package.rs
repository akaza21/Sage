//! Releases move their bin (Pi 1.x ships `dist/bundle/cli.js`) and npm
//! changes its shim templates, so Sage reads the package's own declaration
//! instead of parsing a shim or assuming a path.

use std::path::{Path, PathBuf};

use serde_json::Value;

#[cfg(test)]
use super::claude::read_json;
use super::validation::authentic_descendant_file;

pub(super) const BIN_ENTRY_BYTES_MAX: usize = 256;

#[cfg(test)]
pub(crate) fn npm_package_bin(npm_root: &Path, package: &str, name: &str) -> Option<PathBuf> {
    let package_root = Path::new("node_modules").join(package);
    let manifest_relative = package_root.join("package.json");
    if !authentic_descendant_file(npm_root, &manifest_relative) {
        return None;
    }
    let manifest = read_json(&npm_root.join(manifest_relative))?;
    npm_package_bin_from_manifest(npm_root, package, name, &manifest)
}

/// Launch callers supply verified, pinned manifest contents.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn npm_package_bin_from_manifest(
    npm_root: &Path,
    package: &str,
    name: &str,
    manifest: &Value,
) -> Option<PathBuf> {
    let package_root = Path::new("node_modules").join(package);
    if !authentic_descendant_file(npm_root, package_root.join("package.json")) {
        return None;
    }
    if manifest.get("name").and_then(Value::as_str) != Some(package) {
        return None;
    }
    let entry = package_root.join(bin_entry(manifest, name)?);
    authentic_descendant_file(npm_root, &entry).then(|| npm_root.join(entry))
}

/// A single-string `bin` is named after the package without its scope, as npm
/// does.
pub(super) fn bin_entry(manifest: &Value, name: &str) -> Option<PathBuf> {
    let bin = manifest.get("bin")?;
    let target = match bin {
        Value::String(target) => {
            let package = manifest.get("name").and_then(Value::as_str)?;
            let unscoped = package.rsplit('/').next().unwrap_or(package);
            (unscoped == name).then_some(target.as_str())?
        }
        _ => bin.get(name).and_then(Value::as_str)?,
    };
    relative_module(target)
}

/// Each component is a portable npm file name, so no drive, stream, separator
/// or parent can reach the path on any host.
fn relative_module(bin: &str) -> Option<PathBuf> {
    let relative = bin.strip_prefix("./").unwrap_or(bin);
    if relative.is_empty() || relative.len() > BIN_ENTRY_BYTES_MAX {
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
mod tests {
    use super::*;

    fn npm_tree() -> PathBuf {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-npm-manifest-{}", uuid::Uuid::new_v4()));
        let package = root.join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::write(package.join("bin/codex.js"), "").unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        root
    }

    #[test]
    fn an_ordinary_manifest_resolves_the_declared_bin() {
        let root = npm_tree();
        assert_eq!(
            npm_package_bin(&root, "@openai/codex", "codex"),
            Some(root.join("node_modules/@openai/codex/bin/codex.js"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_symlinked_manifest_is_refused() {
        let root = npm_tree();
        let manifest = root.join("node_modules/@openai/codex/package.json");
        let target = root.join("real-package.json");
        std::fs::rename(&manifest, &target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &manifest).expect("create file symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &manifest).expect(
            "file symlink creation is required; enable Developer Mode or symlink privileges",
        );
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        std::fs::remove_file(manifest).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_manifest_under_a_linked_parent_is_refused() {
        let root = npm_tree();
        // Joined per component: `mklink` reads a `/` in its path as a switch.
        let package = root.join("node_modules").join("@openai").join("codex");
        let target = root.join("real-package");
        std::fs::rename(&package, &target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &package).expect("create directory symlink");
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&package)
                .arg(&target)
                .status()
                .expect("create directory junction");
            assert!(status.success(), "create junction");
        }
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        #[cfg(unix)]
        std::fs::remove_file(package).unwrap();
        #[cfg(windows)]
        std::fs::remove_dir(package).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_bin_target_is_a_portable_relative_module_path_on_every_host() {
        for accepted in [
            "dist/cli.js",
            "./dist/bundle/cli.js",
            "bin/pi-cli_1.mjs",
            "cli.cjs",
        ] {
            assert!(relative_module(accepted).is_some(), "{accepted}");
        }
        for refused in [
            "",
            "./",
            "/dist/cli.js",
            "C:/dist/cli.js",
            "dist\\cli.js",
            "../cli.js",
            "dist/../cli.js",
            "dist/./cli.js",
            "dist//cli.js",
            "dist/cli.js:x.js",
            "dist/cli .js",
            "dist/cli.ts",
            "dist/cli",
        ] {
            assert!(relative_module(refused).is_none(), "{refused:?}");
        }
        assert!(relative_module(&format!("{}.js", "a".repeat(BIN_ENTRY_BYTES_MAX))).is_none());
    }

    #[test]
    fn the_bin_is_the_module_the_named_package_declares() {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-npm-bin-{}", uuid::Uuid::new_v4()));
        let package = root.join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::write(package.join("bin/codex.js"), "").unwrap();
        let manifest = |name: &str, bin: &str| {
            std::fs::write(
                package.join("package.json"),
                serde_json::json!({"name": name, "bin": {"codex": bin}}).to_string(),
            )
            .unwrap();
        };
        manifest("@openai/codex", "bin/codex.js");
        assert_eq!(
            npm_package_bin(&root, "@openai/codex", "codex"),
            Some(root.join("node_modules/@openai/codex").join("bin/codex.js"))
        );
        manifest("@other/codex", "bin/codex.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        manifest("@openai/codex", "bin/moved.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        manifest("@openai/codex", "bin/codex.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex-exec"), None);
        std::fs::write(
            package.join("package.json"),
            serde_json::json!({"name": "@openai/codex", "bin": "bin/codex.js"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            npm_package_bin(&root, "@openai/codex", "codex"),
            Some(root.join("node_modules/@openai/codex").join("bin/codex.js"))
        );
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex-exec"), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
