use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use sage_core::limits::PREVIEW_LIMITS;

/// The only retained command classification. It deliberately says nothing
/// about a process exit code or test reporter output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandActivity {
    #[default]
    Ordinary,
    DeclaredTest,
}

/// Contains only manifest-declared invocations, never a harness-provided raw command.
#[derive(Debug, Clone, Default)]
pub struct DeclaredTestCommands(Vec<String>);

impl DeclaredTestCommands {
    pub fn activity_for(&self, command: &str) -> CommandActivity {
        if self.0.iter().any(|declared| declared == command) {
            CommandActivity::DeclaredTest
        } else {
            CommandActivity::Ordinary
        }
    }

    /// The declared test invocations of one parsed `package.json`.
    pub fn from_manifest(value: &Value) -> Self {
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
