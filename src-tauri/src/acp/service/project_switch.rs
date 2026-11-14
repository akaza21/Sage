use std::time::{Duration, Instant};

use crate::acp::{contract::SageProjectId, runtime::HostError};

use super::{AcpService, shutdown::map_discovery_shutdown};

/// P-02 settlement receives the original deadline, discovery receives only
/// the remainder, and the runtime receives the same original deadline so its
/// command executor derives the remainder itself. Projection advancement is
/// deliberately last: no scheduler, auth, or notifier state changes survive a
/// failed phase.
pub(super) fn coordinate_project_switch<Now, Settle, Discovery, Runtime, Advance>(
    deadline: Instant,
    mut now: Now,
    settle: Settle,
    discovery: Discovery,
    runtime: Runtime,
    advance: Advance,
) -> Result<(), HostError>
where
    Now: FnMut() -> Instant,
    Settle: FnOnce(Instant) -> bool,
    Discovery: FnOnce(Instant) -> Result<(), HostError>,
    Runtime: FnOnce(Instant, Duration) -> Result<(), HostError>,
    Advance: FnOnce(),
{
    if !settle(deadline) {
        return Err(HostError::Deadline);
    }
    if deadline.saturating_duration_since(now()).is_zero() {
        return Err(HostError::Deadline);
    }
    discovery(deadline)?;
    let runtime_remaining = deadline.saturating_duration_since(now());
    if runtime_remaining.is_zero() {
        return Err(HostError::Deadline);
    }
    runtime(deadline, runtime_remaining)?;
    if now() >= deadline {
        return Err(HostError::Deadline);
    }
    advance();
    Ok(())
}

impl AcpService {
    pub(crate) fn project_switched(&self, active_project: &SageProjectId) -> Result<(), HostError> {
        self.file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.revoke_browser_other_projects(active_project);
        // P-02 has no live adapter pump, but its conformance receivers are
        // owner-bound native work and must not survive a project replacement.
        let deadline = Instant::now()
            + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
        coordinate_project_switch(
            deadline,
            Instant::now,
            |deadline| self.read_text_files.terminalize_all().wait_until(deadline),
            |deadline| {
                self.codex_discovery
                    .project_switched_until(deadline)
                    .map_err(map_discovery_shutdown)
            },
            |deadline, _remaining| {
                for lane in self.lanes() {
                    if let Some(runtime) = lane.runtime.active() {
                        runtime.project_switched_until(active_project.clone(), deadline)?;
                    }
                }
                Ok(())
            },
            || {
                for lane in self.lanes() {
                    lane.scheduler.project_switched(active_project);
                }
                self.codex_auth.invalidate();
                if let Some(notifier) = &self.notifier {
                    notifier.set_active_project(active_project.clone());
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Barrier, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    #[derive(Clone)]
    struct ControlledMonotonicClock {
        readings: Arc<Mutex<VecDeque<Instant>>>,
    }

    impl ControlledMonotonicClock {
        fn new(readings: impl IntoIterator<Item = Instant>) -> Self {
            Self {
                readings: Arc::new(Mutex::new(readings.into_iter().collect())),
            }
        }

        fn now(&self) -> Instant {
            self.readings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .expect("controlled monotonic reading")
        }
    }

    #[derive(Default)]
    struct FakeCommandExecutor {
        calls: Vec<&'static str>,
        discovery_deadline: Option<Instant>,
        runtime_deadline: Option<Instant>,
        runtime_budget: Option<Duration>,
        advanced: usize,
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[test]
    fn p02_settlement_consumes_budget_then_discovery_receives_only_remaining_deadline() {
        let deadline = deadline();
        let clock = ControlledMonotonicClock::new([deadline - Duration::from_millis(6)]);
        let executor = Arc::new(Mutex::new(FakeCommandExecutor::default()));
        let discovery_executor = Arc::clone(&executor);
        let result = coordinate_project_switch(
            deadline,
            || clock.now(),
            |_| true,
            move |received_deadline| {
                let mut executor = discovery_executor.lock().unwrap();
                executor.calls.push("discovery");
                executor.discovery_deadline = Some(received_deadline);
                Err(HostError::Unavailable)
            },
            |_, _| unreachable!("discovery failure must stop runtime"),
            || unreachable!("discovery failure must stop advancement"),
        );

        assert_eq!(result, Err(HostError::Unavailable));
        let executor = executor.lock().unwrap();
        assert_eq!(executor.calls, ["discovery"]);
        assert_eq!(executor.discovery_deadline, Some(deadline));
    }

    #[test]
    fn discovery_consumes_remaining_deadline_so_runtime_is_not_started_and_deadline_is_typed() {
        let deadline = deadline();
        let clock = ControlledMonotonicClock::new([deadline - Duration::from_millis(4), deadline]);
        let executor = Arc::new(Mutex::new(FakeCommandExecutor::default()));
        let discovery_executor = Arc::clone(&executor);
        let result = coordinate_project_switch(
            deadline,
            || clock.now(),
            |_| true,
            move |received_deadline| {
                let mut executor = discovery_executor.lock().unwrap();
                executor.calls.push("discovery");
                executor.discovery_deadline = Some(received_deadline);
                Ok(())
            },
            |_, _| unreachable!("expired discovery budget must not start runtime"),
            || unreachable!("expired discovery budget must not advance project state"),
        );

        assert_eq!(result, Err(HostError::Deadline));
        let executor = executor.lock().unwrap();
        assert_eq!(executor.calls, ["discovery"]);
        assert_eq!(executor.discovery_deadline, Some(deadline));
    }

    #[test]
    fn runtime_receives_only_remaining_budget_failure_propagates_without_project_advancement() {
        let deadline = deadline();
        let clock = ControlledMonotonicClock::new([
            deadline - Duration::from_millis(7),
            deadline - Duration::from_millis(3),
        ]);
        let executor = Arc::new(Mutex::new(FakeCommandExecutor::default()));
        let discovery_executor = Arc::clone(&executor);
        let runtime_executor = Arc::clone(&executor);
        let result = coordinate_project_switch(
            deadline,
            || clock.now(),
            |_| true,
            move |received_deadline| {
                let mut executor = discovery_executor.lock().unwrap();
                executor.calls.push("discovery");
                executor.discovery_deadline = Some(received_deadline);
                Ok(())
            },
            move |received_deadline, remaining| {
                let mut executor = runtime_executor.lock().unwrap();
                executor.calls.push("runtime");
                executor.runtime_deadline = Some(received_deadline);
                executor.runtime_budget = Some(remaining);
                Err(HostError::Unavailable)
            },
            || unreachable!("runtime failure must stop scheduler/auth/notifier advancement"),
        );

        assert_eq!(result, Err(HostError::Unavailable));
        let executor = executor.lock().unwrap();
        assert_eq!(executor.calls, ["discovery", "runtime"]);
        assert_eq!(executor.discovery_deadline, Some(deadline));
        assert_eq!(executor.runtime_deadline, Some(deadline));
        assert_eq!(executor.runtime_budget, Some(Duration::from_millis(3)));
        assert_eq!(executor.advanced, 0);
    }

    #[test]
    fn all_project_switch_phases_inside_one_deadline_advance_once() {
        let deadline = deadline();
        let clock = ControlledMonotonicClock::new([
            deadline - Duration::from_millis(8),
            deadline - Duration::from_millis(5),
            deadline - Duration::from_millis(1),
        ]);
        let executor = Arc::new(Mutex::new(FakeCommandExecutor::default()));
        let discovery_executor = Arc::clone(&executor);
        let runtime_executor = Arc::clone(&executor);
        let advance_executor = Arc::clone(&executor);
        let runtime_reached = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&runtime_reached);
        let worker = thread::spawn(move || {
            coordinate_project_switch(
                deadline,
                || clock.now(),
                |_| true,
                move |received_deadline| {
                    let mut executor = discovery_executor.lock().unwrap();
                    executor.calls.push("discovery");
                    executor.discovery_deadline = Some(received_deadline);
                    Ok(())
                },
                move |received_deadline, remaining| {
                    let mut executor = runtime_executor.lock().unwrap();
                    executor.calls.push("runtime");
                    executor.runtime_deadline = Some(received_deadline);
                    executor.runtime_budget = Some(remaining);
                    drop(executor);
                    worker_barrier.wait();
                    Ok(())
                },
                move || {
                    let mut executor = advance_executor.lock().unwrap();
                    executor.calls.push("advance");
                    executor.advanced += 1;
                },
            )
        });
        runtime_reached.wait();
        assert_eq!(worker.join().unwrap(), Ok(()));

        let executor = executor.lock().unwrap();
        assert_eq!(executor.calls, ["discovery", "runtime", "advance"]);
        assert_eq!(executor.discovery_deadline, Some(deadline));
        assert_eq!(executor.runtime_deadline, Some(deadline));
        assert_eq!(executor.runtime_budget, Some(Duration::from_millis(5)));
        assert_eq!(executor.advanced, 1);
    }
}
