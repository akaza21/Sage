use super::*;
use crate::acp::{
    contract::ProjectionRevision,
    intent::{AcpConflictResource, AcpUnavailableReason},
};

#[test]
fn snapshot_read_failures_preserve_the_frontend_ipc_contract() {
    let unavailable =
        snapshot_error(crate::acp_protocol::file_diff::SnapshotReadError::Unavailable);
    assert!(matches!(
        unavailable,
        AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::SessionUnavailable,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(unavailable).unwrap(),
        serde_json::json!({
            "kind": "unavailable",
            "reason": "sessionUnavailable",
            "message": "No admitted file snapshot exists for this change"
        })
    );
    assert!(matches!(
        snapshot_error(crate::acp_protocol::file_diff::SnapshotReadError::Stale),
        AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            ..
        }
    ));
    assert!(matches!(
        snapshot_error(crate::acp_protocol::file_diff::SnapshotReadError::Corrupt),
        AcpIpcError::RecoveryRequired { .. }
    ));
    assert!(matches!(
        snapshot_error(crate::acp_protocol::file_diff::SnapshotReadError::Forbidden),
        AcpIpcError::InvalidRequest { .. }
    ));
}

#[test]
fn snapshot_command_rejects_stale_generation_and_forged_refs_without_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = crate::commands::tests::state_with_project();
        let root_id: String = state
            .database
            .connection()
            .query_row(
                "SELECT id FROM project_roots WHERE project_id=?1",
                [&project],
                |r| r.get(0),
            )
            .unwrap();
        let request: GetAcpFileSnapshotRequest = serde_json::from_value(serde_json::json!({
            "projectId":project, "projectGeneration":"0", "expectedProjectionRevision":"0",
            "chatId":chat, "sessionId":"absent-session", "runtimeEpoch":"1", "turnId":"absent-turn",
            "itemId":"absent-item", "diffOrdinal":0, "snapshotRevision":"1", "rootId":root_id,
            "relativePath":"src/a.ts", "contentDigest":"a".repeat(64),
        }))
        .unwrap();
        let before = state.database.connection().total_changes();
        let mut stale = request.clone();
        stale.project_generation = ProjectGeneration::new(1).unwrap();
        assert!(matches!(
            get_acp_file_snapshot_inner(&state, stale).await,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::ProjectGeneration,
                ..
            })
        ));
        for path in ["C:/secret", "../secret", "src//a.ts", "src\\a.ts"] {
            let mut forged = request.clone();
            forged.relative_path = path.into();
            assert!(matches!(
                get_acp_file_snapshot_inner(&state, forged).await,
                Err(AcpIpcError::InvalidRequest { .. })
            ));
        }
        let mut forged = request.clone();
        forged.snapshot_revision = i64::MAX as u64 + 1;
        assert!(matches!(
            get_acp_file_snapshot_inner(&state, forged).await,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::Session,
                ..
            })
        ));
        let mut stale = request;
        stale.expected_projection_revision = ProjectionRevision::new(9).unwrap();
        assert!(matches!(
            get_acp_file_snapshot_inner(&state, stale).await,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::Session,
                ..
            })
        ));
        assert_eq!(state.database.connection().total_changes(), before);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    });
}
