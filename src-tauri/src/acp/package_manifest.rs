use std::{fs::File, io::Read, path::Path};

use serde_json::Value;

use sage_acp_protocol::declared_test::DeclaredTestCommands;
use sage_core::limits::PREVIEW_LIMITS;

const MAX_MANIFEST_BYTES: u64 = PREVIEW_LIMITS.manifest_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageManifestError {
    Open,
    Metadata,
    Read,
    Malformed,
}

/// `Ok(None)` identifies a truthful over-cap manifest.
pub(crate) fn read_bounded_package_manifest(
    path: &Path,
) -> Result<Option<Value>, PackageManifestError> {
    if is_reparse_point(path)? {
        return Ok(None);
    }
    let file = File::open(path).map_err(|_| PackageManifestError::Open)?;
    let metadata = file
        .metadata()
        .map_err(|_| PackageManifestError::Metadata)?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageManifestError::Read)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| PackageManifestError::Malformed)
}

pub(crate) fn declared_test_commands_for_project(root: &Path) -> DeclaredTestCommands {
    let manifest = root.join("package.json");
    let Ok(Some(value)) = read_bounded_package_manifest(&manifest) else {
        return DeclaredTestCommands::default();
    };
    DeclaredTestCommands::from_manifest(&value)
}

/// Match Preview discovery's no-reparse policy before opening a manifest.
#[cfg(windows)]
fn is_reparse_point(path: &Path) -> Result<bool, PackageManifestError> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let metadata = path
        .symlink_metadata()
        .map_err(|_| PackageManifestError::Metadata)?;
    Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
fn is_reparse_point(path: &Path) -> Result<bool, PackageManifestError> {
    path.symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .map_err(|_| PackageManifestError::Metadata)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::declared_test_commands_for_project;
    use sage_acp_protocol::declared_test::CommandActivity;
    use sage_core::limits::PREVIEW_LIMITS;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("sage-declared-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn activity(root: &Path, command: &str) -> CommandActivity {
        declared_test_commands_for_project(root).activity_for(command)
    }

    #[test]
    fn recognizes_declared_test_script_and_documented_npm_forms() {
        let temp = TestDirectory::new();
        fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();

        for command in ["vitest", "npm test", "npm run test"] {
            assert_eq!(
                activity(temp.path(), command),
                CommandActivity::DeclaredTest
            );
        }
    }

    #[test]
    fn uses_only_the_declared_package_manager_forms() {
        let temp = TestDirectory::new();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        for command in ["vitest", "pnpm test", "pnpm run test"] {
            assert_eq!(
                activity(temp.path(), command),
                CommandActivity::DeclaredTest
            );
        }
        for command in ["npm test", "npm run test", "yarn test", "yarn run test"] {
            assert_eq!(activity(temp.path(), command), CommandActivity::Ordinary);
        }

        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"yarn@4.0.0","scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        for command in ["vitest", "yarn test", "yarn run test"] {
            assert_eq!(
                activity(temp.path(), command),
                CommandActivity::DeclaredTest
            );
        }
        assert_eq!(activity(temp.path(), "npm test"), CommandActivity::Ordinary);
    }

    #[test]
    fn present_unrecognized_manager_never_synthesizes_npm_forms() {
        let temp = TestDirectory::new();
        for manager in ["bun@1.1.0", "pnpm", ""] {
            fs::write(
                temp.path().join("package.json"),
                serde_json::json!({"packageManager": manager, "scripts": {"test": "vitest"}})
                    .to_string(),
            )
            .unwrap();
            assert_eq!(
                activity(temp.path(), "vitest"),
                CommandActivity::DeclaredTest
            );
            for command in ["npm test", "npm run test", "pnpm test", "pnpm run test"] {
                assert_eq!(
                    activity(temp.path(), command),
                    CommandActivity::Ordinary,
                    "{manager:?} must not admit {command:?}",
                );
            }
        }
    }

    #[test]
    fn fails_closed_for_absent_oversize_and_malformed_manifests() {
        let temp = TestDirectory::new();
        assert_eq!(activity(temp.path(), "npm test"), CommandActivity::Ordinary);

        fs::write(
            temp.path().join("package.json"),
            "x".repeat(PREVIEW_LIMITS.manifest_bytes as usize + 1),
        )
        .unwrap();
        assert_eq!(activity(temp.path(), "npm test"), CommandActivity::Ordinary);

        fs::write(temp.path().join("package.json"), "{").unwrap();
        assert_eq!(activity(temp.path(), "npm test"), CommandActivity::Ordinary);

        fs::write(
            temp.path().join("package.json"),
            serde_json::json!({"scripts": {"test": format!("{}vitest", '\0')}}).to_string(),
        )
        .unwrap();
        assert_eq!(activity(temp.path(), "vitest"), CommandActivity::Ordinary);
    }

    #[test]
    fn rejects_near_misses_without_shell_or_package_manager_inference() {
        let temp = TestDirectory::new();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"npm@10.0.0","scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        for command in [
            " vitest",
            "vitest ",
            "VITEST",
            "cmd /c vitest",
            "C:\\project\\node_modules\\.bin\\vitest",
            "yarn test",
            "npm  test",
            "npm test -- --runInBand",
        ] {
            assert_eq!(
                activity(temp.path(), command),
                CommandActivity::Ordinary,
                "{command}"
            );
        }
    }
}
