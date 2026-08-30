/// Immutable identity observed through the retained root-process handle after
/// the executable image has passed supervisor verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessIdentityReceipt {
    runtime_epoch: u64,
    process_id: u32,
    creation_time_filetime_100ns: u64,
    executable_sha256: [u8; 32],
    executable_bytes: u64,
    canonical_image_path: std::path::PathBuf,
}

impl ProcessIdentityReceipt {
    pub(super) fn new(
        runtime_epoch: u64,
        process_id: u32,
        creation_time_filetime_100ns: u64,
        executable_sha256: [u8; 32],
        executable_bytes: u64,
        canonical_image_path: std::path::PathBuf,
    ) -> Self {
        Self {
            runtime_epoch,
            process_id,
            creation_time_filetime_100ns,
            executable_sha256,
            executable_bytes,
            canonical_image_path,
        }
    }

    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }

    pub(crate) fn process_id(&self) -> u32 {
        self.process_id
    }

    pub(crate) fn creation_time_filetime_100ns(&self) -> u64 {
        self.creation_time_filetime_100ns
    }

    pub(crate) fn executable_sha256(&self) -> [u8; 32] {
        self.executable_sha256
    }

    pub(crate) fn executable_bytes(&self) -> u64 {
        self.executable_bytes
    }

    pub(crate) fn canonical_image_path(&self) -> &std::path::Path {
        &self.canonical_image_path
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        runtime_epoch: u64,
        process_id: u32,
        creation_time_filetime_100ns: u64,
        executable_sha256: [u8; 32],
        executable_bytes: u64,
        canonical_image_path: std::path::PathBuf,
    ) -> Self {
        Self::new(
            runtime_epoch,
            process_id,
            creation_time_filetime_100ns,
            executable_sha256,
            executable_bytes,
            canonical_image_path,
        )
    }
}

/// Native monotonic anchors for the exact verified child launch. The receipt
/// stays attached to the retained process authority, so a collector cannot
/// accidentally combine a start anchor from one epoch with another process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnTimingReceipt {
    frequency_hz: u64,
    spawn_started_tick: u64,
    process_ready_tick: u64,
}

impl SpawnTimingReceipt {
    pub(super) fn new(
        frequency_hz: u64,
        spawn_started_tick: u64,
        process_ready_tick: u64,
    ) -> Result<Self, super::supervisor::LaunchError> {
        if frequency_hz == 0 || process_ready_tick < spawn_started_tick {
            return Err(super::supervisor::LaunchError::ContainmentFailure);
        }
        Ok(Self {
            frequency_hz,
            spawn_started_tick,
            process_ready_tick,
        })
    }

    pub(crate) fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }

    pub(crate) fn spawn_started_tick(&self) -> u64 {
        self.spawn_started_tick
    }

    pub(crate) fn process_ready_tick(&self) -> u64 {
        self.process_ready_tick
    }

    pub(crate) fn duration_micros(&self) -> u64 {
        let ticks = self.process_ready_tick - self.spawn_started_tick;
        u64::try_from(
            u128::from(ticks)
                .saturating_mul(1_000_000)
                .div_ceil(u128::from(self.frequency_hz)),
        )
        .unwrap_or(u64::MAX)
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        frequency_hz: u64,
        spawn_started_tick: u64,
        process_ready_tick: u64,
    ) -> Self {
        Self::new(frequency_hz, spawn_started_tick, process_ready_tick).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::SpawnTimingReceipt;

    #[test]
    fn spawn_timing_requires_one_monotonic_clock_and_rounds_up_microseconds() {
        assert!(SpawnTimingReceipt::new(0, 1, 2).is_err());
        assert!(SpawnTimingReceipt::new(10, 2, 1).is_err());
        let receipt = SpawnTimingReceipt::new(3_000_000, 10, 14).unwrap();
        assert_eq!(receipt.duration_micros(), 2);
    }
}
