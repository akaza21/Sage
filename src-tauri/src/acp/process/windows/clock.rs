use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use sage_process::error::LaunchError;

pub(super) fn performance_clock() -> Result<(u64, u64), LaunchError> {
    let mut frequency = 0_i64;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return Err(LaunchError::ContainmentFailure);
    }
    Ok((
        u64::try_from(frequency).map_err(|_| LaunchError::ContainmentFailure)?,
        monotonic_tick()?,
    ))
}

pub(crate) fn monotonic_tick() -> Result<u64, LaunchError> {
    let mut tick = 0_i64;
    if unsafe { QueryPerformanceCounter(&mut tick) } == 0 || tick < 0 {
        return Err(LaunchError::ContainmentFailure);
    }
    u64::try_from(tick).map_err(|_| LaunchError::ContainmentFailure)
}
