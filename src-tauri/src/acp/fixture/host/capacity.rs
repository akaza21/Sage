use std::sync::Arc;

use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    runtime::{CapacityScope, LiveCapacityError, LiveCapacityLease, LiveCapacityPool},
};
use sage_acp_protocol::contract::SageAgentSessionId;

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "rpc-session-16-global-32",
    "rpc-session-17",
    "rpc-global-33",
    "server-requests-session-8-global-16",
    "server-requests-session-9",
    "server-requests-global-17",
    "runtime-tasks-session-16-global-64",
    "runtime-tasks-session-17",
    "runtime-tasks-global-65",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    assert_eq!(steps.len(), 1);
    let step = &steps[0];
    let pool = match step["op"].as_str().unwrap() {
        "reserveRpc" => LiveCapacityPool::isolated_rpc(),
        "reserveServerRequests" => LiveCapacityPool::isolated_server_requests(),
        "reserveRuntimeTasks" => LiveCapacityPool::isolated_runtime_tasks(),
        operation => panic!("unexpected capacity operation {operation}"),
    };
    let sessions = step["sessions"].as_u64().unwrap();
    let per_session = step["perSession"].as_u64().unwrap();
    let requested_global = step["global"].as_u64().unwrap();
    let result = reserve_distribution(Arc::clone(&pool), sessions, per_session, requested_global);
    match result {
        Ok(leases) => {
            assert_eq!(pool.usage().1, requested_global);
            drop(leases);
            assert_eq!(pool.usage(), (0, 0));
            json!({"accepted": true})
        }
        Err(LiveCapacityError::Capacity { scope, maximum }) => {
            json!({
                "accepted": false,
                "scope": match scope {
                    CapacityScope::Session => "session",
                    CapacityScope::Global => "global",
                },
                "maximum": maximum,
            })
        }
        Err(error) => panic!("unexpected capacity error {error:?}"),
    }
}

fn reserve_distribution(
    pool: Arc<LiveCapacityPool>,
    sessions: u64,
    per_session: u64,
    requested_global: u64,
) -> Result<Vec<LiveCapacityLease>, LiveCapacityError> {
    assert!(sessions > 0);
    assert!(requested_global <= sessions.checked_mul(per_session).unwrap());
    let mut remaining = requested_global;
    let mut leases = Vec::new();
    leases
        .try_reserve_exact(sessions as usize)
        .map_err(|_| LiveCapacityError::State)?;
    for index in 0..sessions {
        if remaining == 0 {
            break;
        }
        let count = remaining.min(per_session);
        let owner = SageAgentSessionId::parse(format!("fixture-session-{index}"))
            .map_err(|_| LiveCapacityError::State)?;
        leases.push(pool.try_reserve(owner, count)?);
        remaining -= count;
    }
    assert_eq!(remaining, 0);
    Ok(leases)
}
