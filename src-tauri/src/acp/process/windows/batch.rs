use std::ffi::OsString;
use std::path::PathBuf;

use super::super::manifest::ManifestGuard;
use super::super::supervisor::{BoundedEnvironment, BoundedOsArgs, LaunchError};
use super::artifact::VerifiedArtifact;

/// Every npm shim for the package names its folder; the rest of the shim
/// is npm's template, which changes between npm releases.
const CODEX_PACKAGE_DIRECTORY: &str = r"\node_modules\@openai\codex\";
const MAX_LAUNCHER_BYTES: u64 = 16 * 1024;

pub(super) struct BatchLaunchPlan {
    pub(super) node: PathBuf,
    pub(super) args: Vec<OsString>,
}

pub(super) fn parse_npm_codex_launcher(
    artifact: &VerifiedArtifact,
    manifest: Option<&ManifestGuard>,
    environment: &BoundedEnvironment,
    fixed_args: &BoundedOsArgs,
) -> Result<BatchLaunchPlan, LaunchError> {
    let contents = artifact.read_launcher_bytes(MAX_LAUNCHER_BYTES)?;
    if !observed_npm_codex_launcher(&contents) {
        return Err(LaunchError::IdentityMismatch);
    }
    let launcher_directory = artifact
        .canonical_path
        .parent()
        .ok_or(LaunchError::InvalidAbsolutePath)?;
    let manifest = manifest.ok_or(LaunchError::IdentityMismatch)?;
    let contents =
        serde_json::from_slice(manifest.bytes()).map_err(|_| LaunchError::IdentityMismatch)?;
    let script = crate::harness_discovery::npm_package_bin_from_manifest(
        launcher_directory,
        "@openai/codex",
        "codex",
        &contents,
    )
    .ok_or(LaunchError::IdentityMismatch)?;
    let adjacent_node = launcher_directory.join("node.exe");
    let node = if adjacent_node.is_file() {
        adjacent_node
    } else {
        let path = environment
            .as_slice()
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
            .ok_or(LaunchError::IdentityMismatch)?;
        node_from_path(path)?
    };
    let mut args = Vec::with_capacity(fixed_args.as_slice().len().saturating_add(1));
    // Launch rewrites the verbatim script path to its ordinary spelling.
    args.push(script.into_os_string());
    args.extend(fixed_args.as_slice().iter().cloned());
    Ok(BatchLaunchPlan { node, args })
}

/// Whether the shim is npm's launcher for the Codex package, whatever
/// template the installing npm release used. Sage never runs the shim.
fn observed_npm_codex_launcher(contents: &[u8]) -> bool {
    std::str::from_utf8(contents).is_ok_and(|contents| {
        contents
            .to_ascii_lowercase()
            .contains(&CODEX_PACKAGE_DIRECTORY.to_ascii_lowercase())
    })
}

fn node_from_path(path: &std::ffi::OsStr) -> Result<PathBuf, LaunchError> {
    path.to_string_lossy()
        .split(';')
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
        .map(|directory| directory.join("node.exe"))
        .find(|node| node.is_file())
        .ok_or(LaunchError::IdentityMismatch)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn npm_tree() -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("sage-batch-plan-{}", uuid::Uuid::new_v4()));
        let package = directory.join("node_modules/@openai/codex");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(package.join("bin/codex.js"), b"").unwrap();
        fs::write(
            package.join("package.json"),
            br#"{"name":"@openai/codex","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        let contents = concat!(
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n",
            ":start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n",
            "  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n",
            "  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
            "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  ",
            "\"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n"
        );
        fs::write(directory.join("codex.cmd"), contents).unwrap();
        fs::write(directory.join("node.exe"), b"node").unwrap();
        assert!(observed_npm_codex_launcher(contents.as_bytes()));
        directory.canonicalize().unwrap()
    }

    #[test]
    fn accepts_the_observed_npm_codex_launcher_and_resolves_node_from_path() {
        let directory = npm_tree();
        assert_eq!(
            crate::harness_discovery::npm_package_bin(&directory, "@openai/codex", "codex")
                .unwrap(),
            directory
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("bin")
                .join("codex.js")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_a_manifest_only_bin_redirect_after_admission() {
        use crate::acp::{
            bindings::codex::{AdmittedCodexBinding, CodexProjectRootAuthority, CodexRuntime},
            contract::{
                ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
            },
        };
        use crate::project_registry::ProjectRootAuthority;

        let directory = npm_tree();
        let launcher = directory.join("codex.cmd");
        let package = directory.join("node_modules/@openai/codex");
        fs::write(package.join("bin/other.js"), b"").unwrap();
        let manifest = package.join("package.json");
        let binding =
            AdmittedCodexBinding::admit_observed_for_test(launcher, "0.167.1".into()).unwrap();
        let root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-batch-plan").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-batch-plan").unwrap(),
            &directory,
        )
        .unwrap();
        let spec = CodexRuntime::from_admitted_binding(
            std::sync::Arc::new(binding),
            CodexProjectRootAuthority::for_test(root),
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("session-batch-plan").unwrap(),
        )
        .unwrap()
        .launch_spec()
        .unwrap();
        let plan = || {
            let (artifact, guard) = VerifiedArtifact::open_with_manifest(&spec)?;
            parse_npm_codex_launcher(
                &artifact,
                guard.as_ref(),
                &spec.environment,
                &spec.fixed_args,
            )
        };
        assert!(plan().is_ok());
        fs::write(
            manifest,
            br#"{"name":"@openai/codex","bin":{"codex":"bin/other.js"}}"#,
        )
        .unwrap();

        assert_eq!(plan().err(), Some(LaunchError::IdentityMismatch));
        drop(spec);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rediscovery_admits_and_plans_a_manifest_only_codex_reinstall() {
        use crate::acp::{
            bindings::codex::{CodexBindingRegistration, CodexProjectRootAuthority, CodexRuntime},
            contract::{
                ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
            },
        };
        use crate::project_registry::ProjectRootAuthority;

        let directory = npm_tree();
        let launcher = directory.join("codex.cmd");
        let package = directory.join("node_modules/@openai/codex");
        fs::write(package.join("bin/reinstalled.js"), b"").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();
        let discover = || {
            registration
                .authenticate_observed_for_test(launcher.clone(), "0.167.1".into())
                .unwrap()
        };
        let earlier = discover();

        // Rediscovery sees only a new manifest, before refresh has detected
        // drift. The shim and reported CLI version remain unchanged.
        fs::write(
            package.join("package.json"),
            br#"{"name":"@openai/codex","bin":{"codex":"bin/reinstalled.js"}}"#,
        )
        .unwrap();
        discover();
        let current = registration.refresh().unwrap();
        assert!(registration.is_available());
        assert_ne!(current.binding_digest(), earlier.binding_digest());
        assert_eq!(current.artifact_digest(), earlier.artifact_digest());
        assert_eq!(current.harness_version(), earlier.harness_version());

        let root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-manifest-reinstall").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-manifest-reinstall").unwrap(),
            &directory,
        )
        .unwrap();
        let spec = CodexRuntime::from_admitted_binding(
            current,
            CodexProjectRootAuthority::for_test(root),
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("session-manifest-reinstall").unwrap(),
        )
        .unwrap()
        .launch_spec()
        .unwrap();
        let (artifact, guard) = VerifiedArtifact::open_with_manifest(&spec).unwrap();
        let plan = parse_npm_codex_launcher(
            &artifact,
            guard.as_ref(),
            &spec.environment,
            &spec.fixed_args,
        )
        .unwrap();
        assert_eq!(plan.node, directory.join("node.exe"));
        assert_eq!(plan.args[0], package.join("bin/reinstalled.js").into_os_string());
        drop((artifact, guard, spec));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn accepts_npm_codex_shims_from_any_npm_template() {
        let current = concat!(
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n",
            ":start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n",
            "  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n",
            "  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
            "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  ",
            "\"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n"
        );
        // A different template, as a later npm might write it.
        let rewritten =
            "@echo off\r\nnode \"%~dp0\\node_modules\\@OpenAI\\Codex\\bin\\codex.js\" %*\r\n";
        assert!(observed_npm_codex_launcher(current.as_bytes()));
        assert!(observed_npm_codex_launcher(rewritten.as_bytes()));
    }

    #[test]
    fn rejects_launcher_content_that_is_not_the_observed_npm_codex_shape() {
        assert!(!observed_npm_codex_launcher(
            b"@echo off\r\npowershell -EncodedCommand arbitrary\r\n"
        ));
    }

    #[test]
    fn resolves_only_an_existing_absolute_node_executable() {
        let directory = std::env::temp_dir().join("sage-batch-plan-node-test");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let node = directory.join("node.exe");
        fs::write(&node, b"node").unwrap();
        let path = OsString::from(directory.as_os_str());
        assert_eq!(node_from_path(&path).unwrap(), node);
        fs::remove_dir_all(directory).unwrap();
    }
}
