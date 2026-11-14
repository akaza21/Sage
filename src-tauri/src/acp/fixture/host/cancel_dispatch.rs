use serde_json::{Value, json};

use crate::acp::runtime::{
    CancelDispatchObservation, observe_process_live_port_cancel_at_capacity,
};
use sage_core::limits::LIMITS;

pub(super) fn execute(steps: &[Value]) -> Value {
    let [fill, dispatch] = steps else {
        panic!("expected exact driver fill followed by cancel dispatch");
    };
    assert_eq!(fill["op"], "fillDriverData");
    assert_eq!(dispatch["op"], "dispatchCancel");
    let messages = field_u64(fill, "messages");
    let bytes = field_u64(fill, "bytes");
    let deadline_ms = field_u64(dispatch, "deadlineMs");
    assert_eq!(deadline_ms, LIMITS.deadlines_ms.cancel_dispatch);
    assert_eq!(messages, LIMITS.queues.driver_data_messages);
    assert_eq!(bytes, messages);

    let CancelDispatchObservation {
        cancel_dispatch_ms_maximum,
        control_capacity_borrowed,
    } = observe_process_live_port_cancel_at_capacity(messages, bytes, deadline_ms);
    json!({
        "cancelDispatchMsMaximum": cancel_dispatch_ms_maximum,
        "controlCapacityBorrowed": control_capacity_borrowed,
    })
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}
