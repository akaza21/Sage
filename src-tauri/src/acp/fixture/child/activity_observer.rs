use serde_json::Value;

use super::super::{generated_cases::FrozenCase, host};

pub(crate) const CASE_IDS: &[&str] = &[];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    host::execute_child_activity(case)
}
