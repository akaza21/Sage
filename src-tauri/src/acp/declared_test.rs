use std::{collections::BTreeSet, path::Path};

use serde::{Deserialize, Serialize};

use crate::acp::{limits::PREVIEW_LIMITS, package_manifest::read_bounded_package_manifest};

/// The only retained command classification. It deliberately says nothing
/// about a process exit code or test reporter output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandActivity {
    #[default]
    Ordinary,
    DeclaredTest,
}

/// A small, bounded declaration set retained for one runtime. It contains only
/// manifest-declared invocations, never a harness-provided raw command.
#[derive(Debug, Clone, Default)]
pub(crate) struct DeclaredTestCommands(Vec<String>);

impl DeclaredTestCommands {
    pub(crate) fn activity_for(&self, command: &str) -> CommandActivity {
        if self.0.iter().any(|declared| declared == command) {
            CommandActivity::DeclaredTest
        } else {
            CommandActivity::Ordinary
        }
    }
}

/// Reads the declared test invocations once from the project's bounded manifest.
/// This is declaration lookup, not an inference about an arbitrary shell command.
pub(crate) fn declared_test_commands_for_project(root: &Path) -> DeclaredTestCommands {
    let manifest = root.join("package.json");
    let Ok(Some(value)) = read_bounded_package_manifest(&manifest) else {
        return DeclaredTestCommands::default();
    };
    let Some(script) = value
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .and_then(|scripts| scripts.get("test"))
        .and_then(serde_json::Value::as_str)
        .filter(|script| {
            script.len() <= PREVIEW_LIMITS.script_bytes as usize && !script.contains('\0')
        })
    else {
        return DeclaredTestCommands::default();
    };

    let manager = match value.get("packageManager") {
        // npm is the default only when the field is absent. A present but
        // malformed, versionless, or unknown declaration cannot authorize
        // synthesizing a cross-manager alias and therefore has no run forms.
        None => Some(PackageManager::Npm),
        Some(serde_json::Value::String(value)) => package_manager_name(value),
        Some(_) => None,
    };
    let mut commands = BTreeSet::from([script.to_owned()]);
    let runs: &[&str] = match manager {
        Some(PackageManager::Npm) => &["npm test", "npm run test"],
        Some(PackageManager::Pnpm) => &["pnpm test", "pnpm run test"],
        Some(PackageManager::Yarn) => &["yarn test", "yarn run test"],
        None => &[],
    };
    commands.extend(runs.iter().map(|command| (*command).to_owned()));
    DeclaredTestCommands(commands.into_iter().collect())
}

#[derive(Debug, Clone, Copy)]
enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

fn package_manager_name(value: &str) -> Option<PackageManager> {
    let (name, version) = value.split_once('@')?;
    if version.is_empty() {
        return None;
    }
    match name {
        "npm" => Some(PackageManager::Npm),
        "pnpm" => Some(PackageManager::Pnpm),
        "yarn" => Some(PackageManager::Yarn),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::{CommandActivity, declared_test_commands_for_project};
    use crate::acp::limits::PREVIEW_LIMITS;

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
