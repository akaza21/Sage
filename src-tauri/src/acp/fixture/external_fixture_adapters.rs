use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::ResourceMapper,
        ipc::{IntentSnapshot, ProjectionChangedV1},
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
            RequiredContainment, TreeLimits,
        },
        runtime::{
            ProjectIntentAuthority, ProjectionPublishError, ProjectionPublishStatus,
            ProjectionPublisher,
        },
        store::PrepareTurnCommand,
    },
    persistence::Database,
    project_registry::ProjectRootAuthority,
};
use sage_acp_protocol::{
    contract::{ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId},
    intent::{SafeResourceRef, UnavailableResourceReason},
};

#[derive(Clone, Copy)]
pub(super) enum ExternalCase {
    TurnTextSuccess,
    ZeroByteFailure,
    CompleteDeliveryExit(&'static str),
}

impl ExternalCase {
    pub(super) fn id(&self) -> &'static str {
        match self {
            Self::TurnTextSuccess => "turn-text-success",
            Self::ZeroByteFailure => "send-zero-byte-failure",
            Self::CompleteDeliveryExit(case_id) => case_id,
        }
    }
}

pub(super) struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

pub(super) struct Authority(pub(super) IntentSnapshot);

impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _command: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

pub(super) struct CommitCheckingPublisher {
    pub(super) database: Database,
    pub(super) terminal_seen_after_commit: AtomicBool,
}

impl ProjectionPublisher for CommitCheckingPublisher {
    fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        let connection = self.database.connection();
        let revision: i64 = connection
            .query_row(
                "SELECT projection_revision FROM chats WHERE id=?1 AND project_id=?2",
                rusqlite::params![event.chat_id.as_str(), event.project_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(revision >= event.projection_revision.get() as i64);
        let completed: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE session_id=?1 AND state='completed'",
                [event.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        if completed > 0 {
            let items: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM items WHERE session_id=?1 AND lifecycle='completed'
                     AND payload_json LIKE '%fixture text%'",
                    [event.session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(items, 1);
            self.terminal_seen_after_commit
                .store(true, Ordering::SeqCst);
        }
        Ok(ProjectionPublishStatus::Published)
    }
}

pub(super) fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn fixture_artifact() -> (PathBuf, [u8; 32], usize) {
    let path = PathBuf::from(
        std::env::var_os("SAGE_ACP_FIXTURE_PATH")
            .expect("set SAGE_ACP_FIXTURE_PATH to the isolated fixture executable"),
    )
    .canonicalize()
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    let digest = Sha256::digest(&bytes).into();
    (path, digest, bytes.len())
}

pub(super) fn launch_spec(
    case_id: &'static str,
    epoch: RuntimeEpoch,
    fixture_path: PathBuf,
    digest: [u8; 32],
    fixture_byte_len: usize,
) -> LaunchSpec {
    LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: epoch.get(),
        artifact: ArtifactExpectation::new(
            Uuid::nil(),
            fixture_path,
            digest,
            fixture_byte_len as u64,
        )
        .unwrap(),
        fixed_args: BoundedOsArgs::new(vec![
            OsString::from("--case"),
            OsString::from(case_id),
            OsString::from("--seed"),
            OsString::from("0"),
        ])
        .unwrap(),
        environment: BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root: ProjectRootAuthority::for_test(
            SageProjectId::parse("project-external-fixture").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-external-fixture").unwrap(),
            &std::env::current_dir().unwrap(),
        )
        .unwrap(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    }
}

pub(super) fn send_started(database: &Database, session: &SageAgentSessionId) -> i64 {
    database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sendStarted'",
            [session.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}
