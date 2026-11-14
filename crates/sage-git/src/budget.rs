use std::time::Duration;

const READ_SECS: u64 = 10;
const MUTATION_SECS: u64 = 20;
const REMOTE_SECS: u64 = 120;
const ADMISSION_SECS: u64 = REMOTE_SECS;
const REAP_SECS: u64 = 5;

pub const READ: Duration = deadline(READ_SECS);
pub(crate) const MUTATION: Duration = deadline(MUTATION_SECS);
pub(crate) const REMOTE: Duration = deadline(REMOTE_SECS);
pub(crate) const ADMISSION: Duration = deadline(ADMISSION_SECS);
pub const REAP: Duration = deadline(REAP_SECS);

#[cfg(not(any(test, feature = "test-support")))]
const fn deadline(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Tests share a loaded machine, so every Git budget is the shared test
/// budget, `sage_test_support::test_wait::BUDGET`. The app's tests reach it
/// through the `test-support` feature, which cannot take sage-test-support (a
/// dev-dependency only), so the value is spelled out and checked against it.
#[cfg(any(test, feature = "test-support"))]
const fn deadline(_seconds: u64) -> Duration {
    Duration::from_secs(60)
}

#[cfg(test)]
const _: () = assert!(deadline(0).as_nanos() == sage_test_support::test_wait::BUDGET.as_nanos());

#[cfg(test)]
mod tests {
    use super::{ADMISSION_SECS, MUTATION_SECS, READ_SECS, REAP_SECS, REMOTE_SECS};

    const READ_LOADED_MAX_MS: u64 = 1_487;
    const MUTATION_LOADED_MAX_MS: u64 = 1_837;
    const REMOTE_ADVERTISEMENT_MAX_MS: u64 = 2_976;
    const PROCESS_LOADED_MAX_MS: u64 = 616;

    #[test]
    fn every_budget_clears_its_measured_worst_case_with_headroom() {
        for (budget_secs, measured_ms, headroom, label) in [
            (READ_SECS, READ_LOADED_MAX_MS, 5, "read"),
            (MUTATION_SECS, MUTATION_LOADED_MAX_MS, 5, "mutation"),
            (REAP_SECS, PROCESS_LOADED_MAX_MS, 5, "reap"),
            (REMOTE_SECS, REMOTE_ADVERTISEMENT_MAX_MS, 25, "remote"),
        ] {
            assert!(
                budget_secs * 1_000 >= measured_ms * headroom,
                "the {label} budget of {budget_secs}s leaves no headroom over {measured_ms}ms"
            );
        }
    }

    #[test]
    fn admission_budget_outlasts_every_command_budget() {
        for command in [READ_SECS, MUTATION_SECS, REMOTE_SECS] {
            assert!(
                ADMISSION_SECS >= command,
                "a lease held for {command}s cannot be admitted within {ADMISSION_SECS}s"
            );
        }
    }
}
