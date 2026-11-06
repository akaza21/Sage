//! The per-root admission cache, bounded by root count and by the bytes the
//! cached verdicts retain.
//!
//! A verdict keeps a path and a compact fingerprint per watched entry, never
//! file contents. Idle slots (no call holding them) are dropped once the map
//! holds [`MAX_CACHED_ROOTS`] roots or the verdicts retain more than
//! [`MAX_RETAINED_BYTES`] in total; slots in use stay, so an in-flight probe
//! keeps its lock.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::Admission;

pub(super) const MAX_CACHED_ROOTS: usize = 256;
pub(super) const MAX_RETAINED_BYTES: usize = 4 * 1024 * 1024;

pub(super) type AdmissionSlot = Arc<tokio::sync::Mutex<Option<Admission>>>;

static ADMISSIONS: LazyLock<Mutex<HashMap<PathBuf, AdmissionSlot>>> = LazyLock::new(Mutex::default);
/// The bytes every cached verdict reported when it was stored.
static RETAINED: AtomicUsize = AtomicUsize::new(0);

pub(super) fn admission_slot(root: &Path) -> AdmissionSlot {
    slot_in(&mut admissions(), root)
}

fn admissions() -> std::sync::MutexGuard<'static, HashMap<PathBuf, AdmissionSlot>> {
    ADMISSIONS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Returns the slot for `root`, first dropping every idle slot when the map is
/// full or over its byte budget. When every slot is in use, `root` gets a
/// private slot, so its admission is not cached.
pub(super) fn slot_in(
    admissions: &mut HashMap<PathBuf, AdmissionSlot>,
    root: &Path,
) -> AdmissionSlot {
    let full = !admissions.contains_key(root) && admissions.len() >= MAX_CACHED_ROOTS;
    if full || RETAINED.load(Ordering::SeqCst) > MAX_RETAINED_BYTES {
        admissions.retain(|_, slot| {
            if Arc::strong_count(slot) > 1 {
                return true;
            }
            if let Ok(admission) = slot.try_lock() {
                release(
                    admission
                        .as_ref()
                        .map_or(0, |admission| admission.accounted),
                );
            }
            false
        });
        if !admissions.contains_key(root) && admissions.len() >= MAX_CACHED_ROOTS {
            return AdmissionSlot::default();
        }
    }
    Arc::clone(admissions.entry(root.to_path_buf()).or_default())
}

/// Stores `admission` (or nothing) in `slot`, keeping the byte total exact.
pub(super) fn store(slot: &mut Option<Admission>, admission: Option<Admission>) {
    if let Some(previous) = slot.take() {
        release(previous.accounted);
    }
    *slot = admission.map(|mut admission| {
        account(&mut admission);
        admission
    });
}

/// Counts an admission's retained bytes into the budget.
pub(super) fn account(admission: &mut Admission) {
    let retained = admission.retained_bytes();
    RETAINED.fetch_add(retained, Ordering::SeqCst);
    release(std::mem::replace(&mut admission.accounted, retained));
}

fn release(bytes: usize) {
    let _ = RETAINED.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |total| {
        Some(total.saturating_sub(bytes))
    });
}

/// Admits `root` without checking its config, for tests that use a repository
/// filter as a deterministic way to keep a Git process running. The slot is
/// also kept here, so eviction never drops it while other tests fill the map.
#[cfg(test)]
pub(super) fn trust(root: PathBuf, admission: Admission) {
    static TRUSTED: Mutex<Vec<AdmissionSlot>> = Mutex::new(Vec::new());
    let slot = AdmissionSlot::new(tokio::sync::Mutex::new(Some(admission)));
    TRUSTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Arc::clone(&slot));
    admissions().insert(root, slot);
}
