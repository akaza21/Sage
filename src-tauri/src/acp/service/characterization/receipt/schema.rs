use std::io;

use serde::Serialize;

pub(super) const RECEIPT_NAME: &str = "characterization-native-receipt-v2.json";
pub(super) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeReceipt {
    pub schema_version: u8,
    pub case: &'static str,
    pub runtime_epoch: u64,
    pub fixture_process: FixtureProcess,
    pub monotonic_clock: MonotonicClock,
    pub wal_convergence: WalConvergence,
    pub latency_us: Latency,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FixtureProcess {
    pub pid: u32,
    pub creation_time_filetime_100ns: String,
    pub executable_sha256: String,
    pub executable_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MonotonicClock {
    pub frequency_hz: u64,
    pub spawn_started_tick: u64,
    pub process_ready_tick: u64,
    pub initialize_write_started_tick: u64,
    pub ready_committed_tick: u64,
    pub prompt_write_completed_tick: u64,
    pub first_normalized_item_accepted_tick: Option<u64>,
    pub cancel_received_tick: Option<u64>,
    pub cancel_write_completed_tick: Option<u64>,
    pub cancel_terminal_committed_tick: Option<u64>,
    pub close_received_tick: u64,
    pub job_zero_tick: u64,
    pub close_committed_tick: u64,
    pub wal_converged_tick: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WalConvergence {
    pub wal_bytes: u64,
    pub checkpoint_target_bytes: u64,
    pub checkpointed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Latency {
    pub spawn_to_process_ready: u64,
    pub initialize: u64,
    pub first_normalized_output: Option<u64>,
    pub cancel_dispatch: Option<u64>,
    pub cancel_terminal: Option<u64>,
    pub native_process_convergence: u64,
}

pub(super) fn checked_duration(start: u64, end: u64, frequency_hz: u64) -> io::Result<u64> {
    let ticks = end
        .checked_sub(start)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tick order"))?;
    let duration = u64::try_from(
        u128::from(ticks)
            .saturating_mul(1_000_000)
            .div_ceil(u128::from(frequency_hz)),
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "duration"))?;
    (duration > 0 && duration <= MAX_SAFE_INTEGER)
        .then_some(duration)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "duration"))
}
