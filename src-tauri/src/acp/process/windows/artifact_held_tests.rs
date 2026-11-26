//! A verified native executable is held so it cannot change, which is what
//! lets its later rechecks skip hashing the whole build again.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::VerifiedArtifact;
use crate::acp::process::{
    ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
    RequiredContainment, TreeLimits,
};
use crate::project_registry::ProjectRootAuthority;
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId, SageRootId};

fn spec(path: PathBuf, sha256: [u8; 32], bytes: u64) -> LaunchSpec {
    LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 1,
        artifact: ArtifactExpectation::new(Uuid::nil(), path, sha256, bytes).unwrap(),
        fixed_args: BoundedOsArgs::new(Vec::new()).unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root: ProjectRootAuthority::for_test(
            SageProjectId::parse("project-artifact-held").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-artifact-held").unwrap(),
            &std::env::temp_dir(),
        )
        .unwrap(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    }
}

#[test]
fn a_held_native_executable_cannot_change_and_rechecks_as_the_same_file() {
    let directory = std::env::temp_dir().join(format!("sage-artifact-held-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("harness.exe");
    let content = vec![0x5a_u8; 256 * 1024];
    std::fs::write(&path, &content).unwrap();
    let path = path.canonicalize().unwrap();
    let spec = spec(
        path.clone(),
        Sha256::digest(&content).into(),
        content.len() as u64,
    );

    let artifact = VerifiedArtifact::open(&spec).unwrap();
    assert!(
        std::fs::OpenOptions::new().write(true).open(&path).is_err(),
        "no writer while verified"
    );
    assert!(std::fs::rename(&path, directory.join("moved.exe")).is_err());
    assert!(std::fs::remove_file(&path).is_err());
    artifact.revalidate(&spec, "held").unwrap();

    drop(artifact);
    std::fs::remove_dir_all(directory).unwrap();
}
