//! Git alone decides whether a window can skip its Git refresh. Keep original
//! paths independently of Explorer's folded notices, with fixed count/byte caps.
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{MAX_ROOTS, MAX_WATCHES, batch::WorkspaceNotifyBatch, tracked};
use crate::{error::AppError, git_process};

pub(super) const MAX_GIT_PATHS: usize = super::batch::MAX_BATCH_NOTICES;
const MAX_GIT_PATH_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(super) struct GitWindow {
    paths: BTreeMap<String, bool>,
    bytes: usize,
    overflow: bool,
}

impl GitWindow {
    pub(super) fn push(&mut self, path: &str, directory: bool, moved: bool) {
        if self.overflow {
            return;
        }
        // Preserve directory-only rules for extant directories.
        let operand = if directory {
            format!("{path}/")
        } else {
            path.to_owned()
        };
        if let Some(previous) = self.paths.get_mut(&operand) {
            *previous |= moved;
            return;
        }
        if self.paths.len() >= MAX_GIT_PATHS || self.bytes + operand.len() + 1 > MAX_GIT_PATH_BYTES
        {
            self.overflow = true;
            self.paths.clear();
            return;
        }
        self.bytes += operand.len() + 1;
        self.paths.insert(operand, moved);
    }

    async fn irrelevant(&self, root: &Path, cancelled: Arc<AtomicBool>) -> Result<bool, AppError> {
        if self.overflow || self.paths.is_empty() {
            return Ok(false);
        }
        let input: Vec<u8> = self
            .paths
            .keys()
            .flat_map(|path| path.bytes().chain(std::iter::once(0)))
            .collect();
        let output = git_process::run_cancellable(
            root,
            &["check-ignore", "--stdin", "-z", "--no-index"],
            self.bytes,
            Some(input.clone()),
            Arc::clone(&cancelled),
        )
        .await?;
        // Git returns original operands in input order. A partial match,
        // unexpected output or truncation cannot prove every path ignored.
        if !output.status.success() || output.stdout_truncated || output.stdout != input {
            return Ok(false);
        }
        tracked::all_untracked(root, &self.paths, cancelled).await
    }
}

// Shared across watch generations: rebinding cannot overlap the old window's
// cleanup. An unconfirmed process exit blocks future queries for that root.
type RootGate = Arc<tokio::sync::Mutex<bool>>;
static ROOT_GATES: LazyLock<Mutex<HashMap<PathBuf, RootGate>>> = LazyLock::new(Mutex::default);

fn root_gate(root: &Path) -> Option<RootGate> {
    let mut gates = ROOT_GATES.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(windows)]
    let key = PathBuf::from(root.to_string_lossy().to_lowercase());
    #[cfg(not(windows))]
    let key = root.to_path_buf();
    if !gates.contains_key(&key) && gates.len() >= MAX_ROOTS * MAX_WATCHES {
        gates.retain(|_, gate| {
            Arc::strong_count(gate) > 1 || gate.try_lock().map_or(true, |safe| !*safe)
        });
    }
    if !gates.contains_key(&key) && gates.len() >= MAX_ROOTS * MAX_WATCHES {
        return None;
    }
    Some(Arc::clone(
        gates
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(true))),
    ))
}

pub(super) fn wait_for_queries() -> bool {
    let gates: Vec<_> = ROOT_GATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect();
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(
            crate::git_budget::REAP + std::time::Duration::from_secs(1),
            async {
                for gate in gates {
                    let _settled = gate.lock().await;
                }
                git_process::wait_for_cleanup().await
            },
        )
        .await
        .is_ok_and(|result| result.is_ok())
    })
}

/// Read Explorer's configured excludes off the delivery path. A full/busy
/// root falls back to ordinary listing notices, never a queued Git query.
pub(super) fn start_explorer_excludes(
    root: &Path,
    cancelled: Arc<AtomicBool>,
) -> std::sync::mpsc::Receiver<Option<PathBuf>> {
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    if let Some(mut safe) = root_gate(root).and_then(|gate| gate.try_lock_owned().ok()) {
        let root = root.to_path_buf();
        tauri::async_runtime::spawn(async move {
            let excludes = read_explorer_excludes(&root, cancelled, &mut safe).await;
            let _ = sent.send(excludes);
        });
    }
    received
}

#[cfg(test)]
fn explorer_excludes(root: &Path, cancelled: Arc<AtomicBool>) -> Option<PathBuf> {
    let gate = root_gate(root)?;
    tauri::async_runtime::block_on(async {
        let mut safe = gate.lock().await;
        read_explorer_excludes(root, cancelled, &mut safe).await
    })
}

async fn read_explorer_excludes(
    root: &Path,
    cancelled: Arc<AtomicBool>,
    safe: &mut bool,
) -> Option<PathBuf> {
    if !*safe || cancelled.load(Ordering::Acquire) {
        return None;
    }
    let output = match git_process::run_cancellable(
        root,
        &["config", "--path", "-z", "--get", "core.excludesFile"],
        64 * 1024,
        None,
        cancelled,
    )
    .await
    {
        Ok(output) => output,
        Err(AppError::GitContainment { .. }) => {
            *safe = false;
            return None;
        }
        Err(_) => return None,
    };
    if !output.status.success() || output.stdout_truncated {
        return None;
    }
    let path = std::str::from_utf8(output.stdout.strip_suffix(&[0])?).ok()?;
    (!path.is_empty() && !path.contains('\0')).then(|| root.join(path))
}

struct Root {
    path: PathBuf,
    gate: Option<RootGate>,
}

pub(super) struct GitRefresh {
    roots: HashMap<String, Root>,
}

impl GitRefresh {
    pub(super) fn new(roots: Vec<(String, PathBuf)>) -> Self {
        Self {
            roots: roots
                .into_iter()
                .map(|(id, path)| {
                    let gate = root_gate(&path);
                    (id, Root { path, gate })
                })
                .collect(),
        }
    }

    /// Listing/search notices never await Git. Each admitted root owns one
    /// asynchronous decision, with no queue behind a running/unsafe root.
    pub(super) fn emit(
        &self,
        batch: WorkspaceNotifyBatch,
        windows: HashMap<String, GitWindow>,
        cancelled: &Arc<AtomicBool>,
        emit: Arc<dyn Fn(WorkspaceNotifyBatch) + Send + Sync>,
    ) {
        self.emit_with(
            batch,
            windows,
            cancelled,
            emit,
            // Listing/search delivery is already independent. Allow the same
            // load headroom as other Git reads before falling back to refresh.
            crate::git_budget::READ,
            |window, path, stop| async move { window.irrelevant(&path, stop).await },
        );
    }

    pub(super) fn emit_with<F, Fut>(
        &self,
        mut batch: WorkspaceNotifyBatch,
        mut windows: HashMap<String, GitWindow>,
        cancelled: &Arc<AtomicBool>,
        emit: Arc<dyn Fn(WorkspaceNotifyBatch) + Send + Sync>,
        budget: std::time::Duration,
        runner: F,
    ) where
        F: Fn(GitWindow, PathBuf, Arc<AtomicBool>) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        let deadline = tokio::time::Instant::now() + budget;
        let mut pending = Vec::new();
        for (id, root) in &self.roots {
            if batch
                .notices
                .iter()
                .any(|n| &n.root_id == id && n.kind == "resync")
            {
                continue;
            }
            let Some(window) = windows.remove(id) else {
                continue;
            };
            let notices: Vec<_> = batch
                .notices
                .iter()
                .filter(|notice| &notice.root_id == id)
                .cloned()
                .map(|mut notice| {
                    notice.kind = "repository".into();
                    notice.git_relevant = None;
                    notice
                })
                .collect();
            if notices.is_empty() {
                continue;
            }
            let Some(guard) = root
                .gate
                .as_ref()
                .and_then(|gate| Arc::clone(gate).try_lock_owned().ok())
                .filter(|safe| **safe)
            else {
                continue;
            };
            for notice in &mut batch.notices {
                if &notice.root_id == id {
                    // Only the separate repository decision drives Git.
                    notice.git_relevant = Some(false);
                }
            }
            // Preserve every path in the bounded window: consumers may ignore
            // metadata-only notices, so one path cannot represent the others.
            pending.push((
                guard,
                window,
                root.path.clone(),
                WorkspaceNotifyBatch { notices },
            ));
        }
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        emit(batch);
        for (mut safe, window, path, decision) in pending {
            let cancelled = Arc::clone(cancelled);
            let emit = Arc::clone(&emit);
            let runner = runner.clone();
            tauri::async_runtime::spawn(async move {
                let stop = Arc::new(AtomicBool::new(false));
                let query = runner(window, path, Arc::clone(&stop));
                tokio::pin!(query);
                let result = tokio::select! {
                    result = &mut query => result,
                    () = tokio::time::sleep_until(deadline) => {
                        if !cancelled.load(Ordering::Acquire) {
                            emit(decision.clone());
                        }
                        stop.store(true, Ordering::Release);
                        // Do not drop/abort a process-owning future. Keep the
                        // root locked through cancellation and reaper handoff.
                        let result = query.await;
                        if matches!(result, Err(AppError::GitContainment { .. })) { *safe = false; }
                        return;
                    }
                    () = wait_cancelled(&cancelled) => {
                        stop.store(true, Ordering::Release);
                        let result = query.await;
                        if matches!(result, Err(AppError::GitContainment { .. })) { *safe = false; }
                        return;
                    }
                };
                let irrelevant = match result {
                    Ok(irrelevant) => irrelevant,
                    Err(AppError::GitContainment { .. }) => {
                        *safe = false;
                        false
                    }
                    Err(_) => false,
                };
                if !irrelevant && !cancelled.load(Ordering::Acquire) {
                    emit(decision);
                }
            });
        }
    }
}

async fn wait_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod delivery_tests;
