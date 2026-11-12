//! The module an installed npm package's bin runs, read from its manifest.
//!
//! Releases move their bin (Pi 1.x ships `dist/bundle/cli.js`) and npm
//! changes its shim templates, so Sage reads the package's own declaration
//! instead of parsing a shim or assuming a path.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{claude::read_json, validation::authentic_descendant_file};

/// Longest bin target read from a package manifest.
pub(super) const BIN_ENTRY_BYTES_MAX: usize = 256;

/// The installed module that `package`'s `bin.<name>` names, under the npm
/// root that holds `node_modules`. The package must be the named one and the
/// module an authentic file inside it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn npm_package_bin(npm_root: &Path, package: &str, name: &str) -> Option<PathBuf> {
    let package_root = Path::new("node_modules").join(package);
    let manifest = read_json(&npm_root.join(package_root.join("package.json")))?;
    if manifest.get("name").and_then(Value::as_str) != Some(package) {
        return None;
    }
    let entry = package_root.join(bin_entry(&manifest, name)?);
    authentic_descendant_file(npm_root, &entry).then(|| npm_root.join(entry))
}

/// The target of the bin `name` as a path relative to the package root:
/// `bin.<name>`, or a single string `bin`, which npm names after the package
/// without its scope.
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

/// A bin target as a relative `/`-separated path naming a JavaScript module
/// inside the package. Each component is a portable npm file name: ASCII
/// letters, digits, `.`, `_` and `-`, never `.` or `..` alone, so no drive,
/// stream, separator or parent can reach the path on any host.
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
        let root =
            crate::test_paths::temp_dir().join(format!("sage-npm-bin-{}", uuid::Uuid::new_v4()));
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
        // Another package's manifest, a missing module or another bin name.
        manifest("@other/codex", "bin/codex.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        manifest("@openai/codex", "bin/moved.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex"), None);
        manifest("@openai/codex", "bin/codex.js");
        assert_eq!(npm_package_bin(&root, "@openai/codex", "codex-exec"), None);
        // The single-string form names the bin after the unscoped package.
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
