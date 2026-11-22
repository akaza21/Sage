use super::*;
use sage_acp_protocol::{intent::CapabilitySnapshotV1, scheduler::RuntimeOwner};

#[test]
fn catalog_capabilities_replace_durable_controls_only_for_the_running_epoch() {
    let mut fixture = migrated_fixture();
    let owner = RuntimeOwner {
        project_id: fixture.owner.project_id.clone(),
        chat_id: fixture.owner.chat_id.clone(),
        session_id: fixture.owner.session_id.clone(),
        runtime_epoch: fixture.owner.runtime_epoch,
    };
    let mut capabilities: CapabilitySnapshotV1 = serde_json::from_str(&capabilities()).unwrap();
    for supported in [true, false] {
        capabilities.goal.set = supported;
        capabilities.goal.clear = supported;
        let event = super::super::catalog_capabilities::update(
            &mut fixture.connection,
            &owner,
            &capabilities,
        )
        .unwrap()
        .unwrap();
        assert_eq!(event.session_id, owner.session_id);
        let json: String = fixture
            .connection
            .query_row(
                "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                [owner.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<CapabilitySnapshotV1>(&json)
                .unwrap()
                .goal
                .supported(),
            supported
        );
        assert!(
            super::super::catalog_capabilities::update(
                &mut fixture.connection,
                &owner,
                &capabilities
            )
            .unwrap()
            .is_none()
        );
    }
    let stale = RuntimeOwner {
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        ..owner
    };
    capabilities.goal.set = true;
    assert!(
        super::super::catalog_capabilities::update(&mut fixture.connection, &stale, &capabilities)
            .unwrap()
            .is_none()
    );
}
