use super::*;

#[test]
fn failed_spawn_retains_job_owned_process_until_cleanup_proves_exit() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        {
            let mut record = fixture.record.lock().unwrap();
            record.fail_spawn_retained = true;
            record.fail_terminate = true;
        }
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await,
            Err(TerminalError::Backend)
        );
        assert_eq!(
            fixture
                .port
                .close_chat(&fixture.scope.project, &fixture.scope.chat),
            Err(TerminalError::Backend)
        );
        fixture.record.lock().unwrap().fail_spawn_retained = false;
        for _ in 0..3 {
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await
                .unwrap();
        }
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await,
            Err(TerminalError::Capacity)
        );
        fixture.record.lock().unwrap().fail_terminate = false;
        fixture
            .port
            .close_chat(&fixture.scope.project, &fixture.scope.chat)
            .unwrap();
        assert_eq!(fixture.record.lock().unwrap().terminated, 4);
    });
}

#[test]
fn unsettled_previous_project_sessions_hold_global_capacity() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let scopes = [
            fixture.scope.clone(),
            fixture.add_chat_scope(),
            fixture.add_chat_scope(),
            fixture.add_chat_scope(),
        ];
        for scope in &scopes {
            for _ in 0..4 {
                fixture
                    .port
                    .create(&fixture.authority(), scope, 80, 24)
                    .await
                    .unwrap();
            }
        }
        let next_root =
            std::env::temp_dir().join(format!("sage-terminal-next-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&next_root).unwrap();
        let next_project = fixture
            .state
            .database
            .create_project_from_root(&next_root)
            .unwrap();
        let next_chat: String = fixture
            .state
            .database
            .connection()
            .query_row(
                "SELECT id FROM chats WHERE project_id=?1 LIMIT 1",
                [&next_project.id],
                |row| row.get(0),
            )
            .unwrap();
        *fixture.state.project_transitions.lock().await = 1;
        let next_scope = TerminalScope {
            project: SageProjectId::parse(next_project.id).unwrap(),
            generation: ProjectGeneration::new(1).unwrap(),
            chat: SageChatId::parse(next_chat).unwrap(),
        };
        fixture.record.lock().unwrap().fail_terminate = true;
        assert_eq!(
            fixture
                .port
                .project_changed(&next_scope.project, next_scope.generation),
            Err(TerminalError::Backend)
        );
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &next_scope, 80, 24)
                .await,
            Err(TerminalError::Capacity)
        );
        assert_eq!(fixture.record.lock().unwrap().roots.len(), 16);
        fixture.record.lock().unwrap().fail_terminate = false;
        fixture.port.shutdown().unwrap();
        assert!(
            fixture
                .port
                .create(&fixture.authority(), &next_scope, 80, 24)
                .await
                .is_ok()
        );
        std::fs::remove_dir_all(next_root).unwrap();
    });
}

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
