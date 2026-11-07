use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use super::*;
use crate::{persistence::Database, state::AppState};

mod lifecycle;

const HOLD_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct FixtureRecord {
    roots: Vec<std::path::PathBuf>,
    writes: Vec<Vec<u8>>,
    sizes: Vec<(u16, u16)>,
    terminated: usize,
    fail_terminate: bool,
    fail_spawn_retained: bool,
    outputs: Vec<TerminalOutputSink>,
    /// Spawn reports its arrival on the sender, then waits on the receiver.
    spawn_hold: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

struct FixtureProcess(Arc<Mutex<FixtureRecord>>);

impl TerminalProcess for FixtureProcess {
    fn shell_path(&self) -> &str {
        "fixture-shell"
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        self.0.lock().unwrap().writes.push(bytes.to_vec());
        Ok(())
    }
    fn resize(&mut self, columns: u16, rows: u16) -> Result<(), TerminalError> {
        self.0.lock().unwrap().sizes.push((columns, rows));
        Ok(())
    }
    fn terminate(&mut self) -> Result<(), TerminalError> {
        if self.0.lock().unwrap().fail_terminate {
            return Err(TerminalError::Backend);
        }
        self.0.lock().unwrap().terminated += 1;
        Ok(())
    }
}

struct FixtureFactory(Arc<Mutex<FixtureRecord>>);

impl TerminalFactory for FixtureFactory {
    fn spawn(
        &self,
        root: &crate::project_registry::ProjectRootAuthority,
        _columns: u16,
        _rows: u16,
        output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalSpawnFailure> {
        let hold = self.0.lock().unwrap().spawn_hold.take();
        if let Some((arrived, release)) = hold {
            arrived.send(()).unwrap();
            // Bounded, so a broken fix fails the test instead of hanging it.
            let _ = release.recv_timeout(HOLD_TIMEOUT);
        }
        let mut record = self.0.lock().unwrap();
        record.roots.push(root.as_path().to_owned());
        record.outputs.push(output);
        if record.fail_spawn_retained {
            return Err(TerminalSpawnFailure::Retained(Box::new(FixtureProcess(
                self.0.clone(),
            ))));
        }
        Ok(Box::new(FixtureProcess(self.0.clone())))
    }
}

struct Fixture {
    root: std::path::PathBuf,
    state: AppState,
    scope: TerminalScope,
    record: Arc<Mutex<FixtureRecord>>,
    port: TerminalSessionPort,
}

impl Fixture {
    fn authority(&self) -> TerminalAuthority<'_> {
        TerminalAuthority(&self.state)
    }

    fn add_chat_scope(&self) -> TerminalScope {
        let snapshot = self.state.database.bootstrap_snapshot().unwrap();
        let layout = crate::domain::ProjectLayoutRequest {
            project_id: self.scope.project.as_str().to_owned(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        };
        let id = self
            .state
            .database
            .create_chat_on(self.scope.project.as_str(), &layout)
            .unwrap();
        TerminalScope {
            chat: SageChatId::parse(id).unwrap(),
            ..self.scope.clone()
        }
    }

    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("sage-terminal-capacity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let chat_id: String = database
            .connection()
            .query_row(
                "SELECT id FROM chats WHERE project_id=?1 LIMIT 1",
                [&project.id],
                |row| row.get(0),
            )
            .unwrap();
        let state = AppState::try_new(database).unwrap();
        let scope = TerminalScope {
            project: SageProjectId::parse(project.id).unwrap(),
            generation: ProjectGeneration::ZERO,
            chat: SageChatId::parse(chat_id).unwrap(),
        };
        let record = Arc::new(Mutex::new(FixtureRecord::default()));
        let port = TerminalSessionPort::new(Box::new(FixtureFactory(record.clone())));
        Self {
            root,
            state,
            scope,
            record,
            port,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn four_sessions_per_chat_admit_exact_limit_and_reject_plus_one_before_spawn() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        for _ in 0..4 {
            assert!(
                fixture
                    .port
                    .create(&fixture.authority(), &fixture.scope, 80, 24)
                    .await
                    .is_ok()
            );
        }
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await,
            Err(TerminalError::Capacity)
        );
        let canonical = fixture.root.canonicalize().unwrap();
        assert_eq!(
            fixture.record.lock().unwrap().roots.as_slice(),
            &[
                canonical.clone(),
                canonical.clone(),
                canonical.clone(),
                canonical
            ]
        );
    });
}

#[test]
fn input_and_resize_accept_exact_bounds_and_reject_plus_one_without_effect() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let exact = "x".repeat(4_096);
        fixture
            .port
            .write_input(&fixture.authority(), &fixture.scope, handle, &exact)
            .await
            .unwrap();
        assert_eq!(
            fixture
                .port
                .write_input(
                    &fixture.authority(),
                    &fixture.scope,
                    handle,
                    &"x".repeat(4_097)
                )
                .await,
            Err(TerminalError::InvalidInput)
        );
        assert_eq!(
            fixture
                .port
                .write_input(&fixture.authority(), &fixture.scope, handle, "")
                .await,
            Err(TerminalError::InvalidInput)
        );
        fixture
            .port
            .resize(
                &fixture.authority(),
                &fixture.scope,
                handle,
                MAX_COLUMNS,
                MAX_ROWS,
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .port
                .resize(
                    &fixture.authority(),
                    &fixture.scope,
                    handle,
                    MAX_COLUMNS + 1,
                    MAX_ROWS
                )
                .await,
            Err(TerminalError::InvalidDimensions)
        );
        assert_eq!(
            fixture
                .port
                .resize(&fixture.authority(), &fixture.scope, handle, MAX_COLUMNS, 0)
                .await,
            Err(TerminalError::InvalidDimensions)
        );
        let record = fixture.record.lock().unwrap();
        assert_eq!(record.writes, vec![exact.into_bytes()]);
        assert_eq!(record.sizes, vec![(MAX_COLUMNS, MAX_ROWS)]);
    });
}

#[test]
fn create_and_resize_admit_only_a_one_to_500_by_one_to_300_grid() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let rejected = [(0, 24), (80, 0), (501, 24), (80, 301)];
        for (columns, rows) in rejected {
            assert_eq!(
                fixture
                    .port
                    .create(&fixture.authority(), &fixture.scope, columns, rows)
                    .await,
                Err(TerminalError::InvalidDimensions),
                "create {columns}x{rows}"
            );
        }
        assert!(fixture.record.lock().unwrap().roots.is_empty());
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 500, 300)
            .await
            .unwrap();
        fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 1, 1)
            .await
            .unwrap();
        for (columns, rows) in rejected {
            assert_eq!(
                fixture
                    .port
                    .resize(&fixture.authority(), &fixture.scope, handle, columns, rows)
                    .await,
                Err(TerminalError::InvalidDimensions),
                "resize {columns}x{rows}"
            );
        }
        fixture
            .port
            .resize(&fixture.authority(), &fixture.scope, handle, 1, 1)
            .await
            .unwrap();
        let record = fixture.record.lock().unwrap();
        assert_eq!(record.roots.len(), 2);
        assert_eq!(record.sizes, vec![(1, 1)]);
    });
}

#[test]
fn output_ring_rejects_oversized_chunks_and_discloses_byte_and_line_truncation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let output = fixture.record.lock().unwrap().outputs[0].clone();
        assert_eq!(
            output.append(&vec![b'x'; 8_193]),
            Err(TerminalError::Backend)
        );
        for _ in 0..128 {
            output.append(&vec![b'x'; 8_192]).unwrap();
        }
        let exact = fixture
            .port
            .output_snapshot(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(exact.bytes.len(), 1_048_576);
        assert_eq!(exact.truncations, 0);
        output.append(b"y").unwrap();
        let overflow = fixture
            .port
            .output_snapshot(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(overflow.bytes.len(), 1_048_576);
        assert_eq!(overflow.truncations, 1);
        for _ in 0..4_096 {
            output.append(b"z\n").unwrap();
        }
        let lines = fixture
            .port
            .output_snapshot(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(
            lines.bytes.iter().filter(|&&byte| byte == b'\n').count(),
            4_095
        );
        assert!(lines.truncations > overflow.truncations);
    });
}

#[test]
fn create_rejects_a_chat_without_a_tab_in_the_project_before_spawn() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let unknown = TerminalScope {
            chat: SageChatId::new(),
            ..fixture.scope.clone()
        };
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &unknown, 80, 24)
                .await,
            Err(TerminalError::InvalidScope)
        );
        assert!(fixture.record.lock().unwrap().roots.is_empty());
        assert!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await
                .is_ok()
        );
    });
}

#[test]
fn chat_close_kills_only_owned_sessions_and_releases_capacity() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        let foreign = TerminalScope {
            chat: SageChatId::new(),
            ..fixture.scope.clone()
        };
        assert_eq!(
            fixture
                .port
                .write_input(&fixture.authority(), &foreign, handle, "x")
                .await,
            Err(TerminalError::InvalidScope)
        );
        fixture
            .port
            .close_chat(&fixture.scope.project, &fixture.scope.chat)
            .unwrap();
        assert_eq!(fixture.record.lock().unwrap().terminated, 1);
        assert_eq!(
            fixture
                .port
                .write_input(&fixture.authority(), &fixture.scope, handle, "x")
                .await,
            Err(TerminalError::UnknownSession)
        );
        assert!(
            fixture
                .port
                .create(&fixture.authority(), &fixture.scope, 80, 24)
                .await
                .is_ok()
        );
    });
}

#[test]
fn sixteen_sessions_per_project_admit_exact_limit_and_reject_plus_one_before_spawn() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let mut scopes = vec![fixture.scope.clone()];
        for _ in 0..4 {
            let snapshot = fixture.state.database.bootstrap_snapshot().unwrap();
            let layout = crate::domain::ProjectLayoutRequest {
                project_id: fixture.scope.project.as_str().to_owned(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: snapshot.selected_root_id,
                selected_resource: snapshot.selected_resource,
            };
            let id = fixture
                .state
                .database
                .create_chat_on(fixture.scope.project.as_str(), &layout)
                .unwrap();
            scopes.push(TerminalScope {
                chat: SageChatId::parse(id).unwrap(),
                ..fixture.scope.clone()
            });
        }
        for scope in scopes.iter().take(4) {
            for _ in 0..4 {
                fixture
                    .port
                    .create(&fixture.authority(), scope, 80, 24)
                    .await
                    .unwrap();
            }
        }
        assert_eq!(
            fixture
                .port
                .create(&fixture.authority(), &scopes[4], 80, 24)
                .await,
            Err(TerminalError::Capacity)
        );
        assert_eq!(fixture.record.lock().unwrap().roots.len(), 16);
    });
}

#[test]
fn explicit_close_and_project_change_kill_and_reject_late_effects() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let handle = fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        fixture
            .port
            .close(&fixture.authority(), &fixture.scope, handle)
            .await
            .unwrap();
        assert_eq!(fixture.record.lock().unwrap().terminated, 1);
        assert_eq!(
            fixture
                .port
                .write_input(&fixture.authority(), &fixture.scope, handle, "late")
                .await,
            Err(TerminalError::UnknownSession)
        );
        fixture
            .port
            .create(&fixture.authority(), &fixture.scope, 80, 24)
            .await
            .unwrap();
        fixture
            .port
            .project_changed(&SageProjectId::new(), ProjectGeneration::ZERO)
            .unwrap();
        assert_eq!(fixture.record.lock().unwrap().terminated, 2);
        assert_eq!(fixture.port.shutdown(), Ok(()));
    });
}

#[test]
fn a_shutdown_during_a_spawn_ends_the_new_shell_instead_of_orphaning_it() {
    let fixture = Fixture::new();
    let (arrived_tx, arrived) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    fixture.record.lock().unwrap().spawn_hold = Some((arrived_tx, release_rx));
    let created = std::thread::scope(|scope| {
        let create = scope.spawn(|| {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(
                    fixture
                        .port
                        .create(&fixture.authority(), &fixture.scope, 80, 24),
                )
        });
        arrived
            .recv_timeout(HOLD_TIMEOUT)
            .expect("create reaches the spawn");
        assert_eq!(fixture.port.shutdown(), Ok(()));
        release.send(()).unwrap();
        create.join().unwrap()
    });
    assert_eq!(created, Err(TerminalError::Ended));
    assert_eq!(fixture.record.lock().unwrap().terminated, 1);
    assert!(fixture.port.sessions.lock().unwrap().is_empty());
}
