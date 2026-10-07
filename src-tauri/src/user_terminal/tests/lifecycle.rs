use super::*;

#[test]
fn failed_termination_retains_session_and_capacity() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        for _ in 0..4 {
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await
                .unwrap();
        }
        fixture.record.lock().unwrap().fail_terminate = true;
        assert_eq!(
            fixture
                .port
                .close_chat(&fixture.scope.project, &fixture.scope.chat),
            Err(TerminalError::Backend)
        );
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await,
            Err(TerminalError::Capacity)
        );
        fixture.record.lock().unwrap().fail_terminate = false;
        fixture.port.shutdown().unwrap();
        assert_eq!(fixture.record.lock().unwrap().terminated, 4);
    });
}

#[test]
fn stale_project_generation_rejects_create_and_input_before_backend_effect() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        *fixture.state.project_transitions.lock().await = 1;
        assert_eq!(
            fixture
                .port
                .write_input(&fixture.authority(), &fixture.scope, handle, "late")
                .await,
            Err(TerminalError::InvalidScope)
        );
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await,
            Err(TerminalError::InvalidScope)
        );
        assert!(fixture.record.lock().unwrap().writes.is_empty());
        assert_eq!(fixture.record.lock().unwrap().roots.len(), 1);
    });
}
