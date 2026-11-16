mod ledger;
mod schema;

use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::acp::{
    characterization_observation::{
        CharacterizationObservation, CharacterizationObservationEvent,
        CharacterizationObservationSink,
    },
    process::{ProcessIdentityReceipt, SpawnTimingReceipt},
};
use sage_acp_protocol::scheduler::RuntimeOwner;
use sage_db::physical_storage::WalConvergenceReceipt;

use super::types::{CharacterizationCase, hex};
use ledger::{ObservationLedger, validate_safe_clock};
use schema::{FixtureProcess, NativeReceipt, RECEIPT_NAME};

#[derive(Clone)]
pub(super) struct CharacterizationReceiptSink {
    root: PathBuf,
    path: PathBuf,
    case: CharacterizationCase,
    state: Arc<Mutex<ReceiptState>>,
}

enum ReceiptState {
    Empty,
    Active(Box<ActiveReceipt>),
    Complete,
}

struct ActiveReceipt {
    owner: RuntimeOwner,
    file: File,
    process: Option<ProcessIdentityReceipt>,
    timing: Option<SpawnTimingReceipt>,
    observations: ObservationLedger,
}

impl CharacterizationReceiptSink {
    pub(super) fn new(root: &Path, case: CharacterizationCase) -> io::Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "receipt root"));
        }
        let path = root.join(RECEIPT_NAME);
        if path.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "receipt"));
        }
        Ok(Self {
            root,
            path,
            case,
            state: Arc::new(Mutex::new(ReceiptState::Empty)),
        })
    }

    pub(super) fn observation(&self) -> CharacterizationObservation {
        CharacterizationObservation::new(Arc::new(self.clone()))
    }

    pub(super) fn begin(&self, owner: &RuntimeOwner) -> io::Result<()> {
        let mut state = self.lock()?;
        if !matches!(*state, ReceiptState::Empty) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "receipt"));
        }
        if self
            .path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .as_ref()
            != Some(&self.root)
        {
            return Err(data("receipt root"));
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)?;
        if !file.metadata()?.is_file() {
            return Err(data("receipt file"));
        }
        *state = ReceiptState::Active(Box::new(ActiveReceipt {
            owner: owner.clone(),
            file,
            process: None,
            timing: None,
            observations: ObservationLedger::default(),
        }));
        Ok(())
    }

    pub(super) fn record_started(
        &self,
        owner: &RuntimeOwner,
        process: ProcessIdentityReceipt,
        timing: SpawnTimingReceipt,
    ) -> io::Result<()> {
        let mut state = self.lock()?;
        let ReceiptState::Active(active) = &mut *state else {
            return Err(data("receipt state"));
        };
        if &active.owner != owner
            || process.runtime_epoch() != owner.runtime_epoch.get()
            || active.process.is_some()
            || active.timing.is_some()
        {
            return Err(data("receipt owner"));
        }
        validate_safe_clock(timing)?;
        active.process = Some(process);
        active.timing = Some(timing);
        Ok(())
    }

    pub(super) fn complete(
        &self,
        owner: &RuntimeOwner,
        wal: WalConvergenceReceipt,
    ) -> io::Result<()> {
        let mut state = self.lock()?;
        let ReceiptState::Active(active) = &mut *state else {
            return Err(data("receipt state"));
        };
        if &active.owner != owner || owner.runtime_epoch.get() > schema::MAX_SAFE_INTEGER {
            return Err(data("receipt owner"));
        }
        // Keep the non-Copy receipt retained until every validation, write,
        // flush, and sync succeeds. A failed completion remains retryable with
        // the exact process identity originally recorded for this owner.
        let process = process_for_completion(&active.process)?;
        let timing = active.timing.ok_or_else(|| data("timing receipt"))?;
        if process.executable_bytes() > schema::MAX_SAFE_INTEGER {
            return Err(data("process receipt"));
        }
        active.observations.record_wal(wal)?;
        let observed = active.observations.validate(self.case, timing)?;
        let receipt = NativeReceipt {
            schema_version: 2,
            case: self.case.as_str(),
            runtime_epoch: owner.runtime_epoch.get(),
            fixture_process: FixtureProcess {
                pid: process.process_id(),
                creation_time_filetime_100ns: process.creation_time_filetime_100ns().to_string(),
                executable_sha256: hex(&process.executable_sha256()),
                executable_bytes: process.executable_bytes(),
            },
            monotonic_clock: observed.clock,
            wal_convergence: observed.wal,
            latency_us: observed.latency,
        };
        serde_json::to_writer(&mut active.file, &receipt)?;
        active.file.write_all(b"\n")?;
        active.file.flush()?;
        active.file.sync_all()?;
        *state = ReceiptState::Complete;
        Ok(())
    }

    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, ReceiptState>> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("receipt lock"))
    }
}

impl CharacterizationObservationSink for CharacterizationReceiptSink {
    fn record(&self, event: CharacterizationObservationEvent) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let ReceiptState::Active(active) = &mut *state else {
            return;
        };
        if active.observations.record(event).is_err() {
            active.observations.invalidate();
        }
    }
}

fn data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn process_for_completion(
    process: &Option<ProcessIdentityReceipt>,
) -> io::Result<&ProcessIdentityReceipt> {
    process.as_ref().ok_or_else(|| data("process receipt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_completion_can_reuse_the_retained_non_copy_process_receipt() {
        let process = Some(ProcessIdentityReceipt::test_only(
            7,
            41,
            77,
            [2; 32],
            13,
            std::path::PathBuf::from(r"C:\Program Files\fixture.exe"),
        ));

        assert_eq!(process_for_completion(&process).unwrap().process_id(), 41);
        assert!(process.is_some());
    }
}
