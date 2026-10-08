use super::*;

#[test]
fn output_page_streams_only_new_bounded_bytes() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let sink = fixture.record.lock().unwrap().outputs[0].clone();
        sink.append(b"hello world").unwrap();
        let first = fixture
            .port
            .read_output(&fixture.authority(), &fixture.scope, handle, 0)
            .await
            .unwrap();
        assert_eq!(first.bytes, b"hello world");
        assert_eq!(first.next_cursor, 11);
        sink.append(b"!").unwrap();
        let second = fixture
            .port
            .read_output(
                &fixture.authority(),
                &fixture.scope,
                handle,
                first.next_cursor,
            )
            .await
            .unwrap();
        assert_eq!(second.bytes, b"!");
        assert_eq!(second.next_cursor, 12);
    });
}

#[test]
fn output_page_reports_dropped_bytes_after_large_burst() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let sink = fixture.record.lock().unwrap().outputs[0].clone();
        for _ in 0..(OUTPUT_BYTES / OUTPUT_CHUNK_BYTES + 1) {
            sink.append(&vec![b'x'; OUTPUT_CHUNK_BYTES]).unwrap();
        }
        let page = fixture
            .port
            .read_output(&fixture.authority(), &fixture.scope, handle, 0)
            .await
            .unwrap();
        assert!(page.truncated);
        assert_eq!(page.truncations, OUTPUT_CHUNK_BYTES as u64);
        assert_eq!(page.bytes.len(), OUTPUT_PAGE_BYTES);
        assert_eq!(
            page.next_cursor,
            OUTPUT_CHUNK_BYTES as u64 + OUTPUT_PAGE_BYTES as u64
        );
    });
}

#[test]
fn output_page_drains_four_megabytes_in_bounded_reads() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let sink = fixture.record.lock().unwrap().outputs[0].clone();
        let mut cursor = 0;
        for _ in 0..128 {
            for _ in 0..4 {
                sink.append(&vec![b'x'; OUTPUT_CHUNK_BYTES]).unwrap();
            }
            let page = fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, handle, cursor)
                .await
                .unwrap();
            assert_eq!(page.bytes.len(), 4 * OUTPUT_CHUNK_BYTES);
            assert!(!page.truncated);
            cursor = page.next_cursor;
        }
        assert_eq!(cursor, 4 * 1024 * 1024);
    });
}

#[test]
fn output_reader_waits_for_new_bytes_and_excludes_second_reader() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let authority = fixture.authority();
        let pending = fixture
            .port
            .read_output(&authority, &fixture.scope, handle, 0);
        tokio::pin!(pending);
        tokio::select! {
            biased;
            _ = &mut pending => panic!("empty output should wait"),
            _ = tokio::task::yield_now() => {},
        }
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, handle, 0)
                .await,
            Err(TerminalError::Busy)
        );
        fixture.record.lock().unwrap().outputs[0]
            .append(b"ready")
            .unwrap();
        let page = tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(page.bytes, b"ready");
    });
}

#[test]
fn cancelled_reader_releases_session_permit() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        {
            let authority = fixture.authority();
            let pending = fixture
                .port
                .read_output(&authority, &fixture.scope, handle, 0);
            tokio::pin!(pending);
            tokio::select! {
                biased;
                _ = &mut pending => panic!("empty output should wait"),
                _ = tokio::task::yield_now() => {},
            }
        }
        fixture.record.lock().unwrap().outputs[0]
            .append(b"after cancellation")
            .unwrap();
        let page = fixture
            .port
            .read_output(&fixture.authority(), &fixture.scope, handle, 0)
            .await
            .unwrap();
        assert_eq!(page.bytes, b"after cancellation");
    });
}

#[test]
fn read_rejects_foreign_chat_revision_closed_session_and_stale_project() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let other_chat = fixture.add_chat_scope();
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &other_chat, handle, 0)
                .await,
            Err(TerminalError::InvalidScope)
        );
        let wrong_revision = TerminalHandle {
            revision: TerminalRevision(2),
            ..handle
        };
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, wrong_revision, 0)
                .await,
            Err(TerminalError::InvalidScope)
        );
        *fixture.state.project_transitions.lock().await = 1;
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, handle, 0)
                .await,
            Err(TerminalError::InvalidScope)
        );
        *fixture.state.project_transitions.lock().await = 0;
        fixture
            .port
            .close(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, handle, 0)
                .await,
            Err(TerminalError::UnknownSession)
        );
    });
}

#[test]
fn read_rejects_cursor_ahead_of_output() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        assert_eq!(
            fixture
                .port
                .read_output(&fixture.authority(), &fixture.scope, handle, 1)
                .await,
            Err(TerminalError::InvalidInput)
        );
    });
}

#[test]
fn close_wakes_pending_read_without_releasing_stale_output() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let authority = fixture.authority();
        let pending = fixture
            .port
            .read_output(&authority, &fixture.scope, handle, 0);
        tokio::pin!(pending);
        tokio::select! {
            biased;
            _ = &mut pending => panic!("empty output should wait"),
            _ = tokio::task::yield_now() => {},
        }
        fixture
            .port
            .close(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .unwrap(),
            Err(TerminalError::UnknownSession)
        );
    });
}

#[test]
fn project_change_wakes_pending_read_with_invalid_scope() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let authority = fixture.authority();
        let pending = fixture
            .port
            .read_output(&authority, &fixture.scope, handle, 0);
        tokio::pin!(pending);
        tokio::select! {
            biased;
            _ = &mut pending => panic!("empty output should wait"),
            _ = tokio::task::yield_now() => {},
        }
        *fixture.state.project_transitions.lock().await = 1;
        fixture
            .port
            .project_changed(&fixture.scope.project, ProjectGeneration::new(1).unwrap())
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .unwrap(),
            Err(TerminalError::InvalidScope)
        );
    });
}

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
