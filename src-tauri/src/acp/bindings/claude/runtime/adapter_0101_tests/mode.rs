//! 0.101.0 starts a new session in `auto`; Sage switches it to its own mode.

use super::*;

#[test]
fn a_new_session_that_starts_in_auto_is_switched_to_sages_mode_before_it_is_ready() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let batch = new_session(&mut runtime, frame(derived("newSessionInAuto")));
    assert!(
        matches!(batch.events(), [SharedEvent::SessionPolicyPending { .. }]),
        "the session is not ready while it runs in auto"
    );
    let [set_mode] = batch.egress() else {
        panic!("one starting-mode request")
    };
    let request = payload(set_mode);
    assert_eq!(request["method"], "session/set_mode");
    assert_eq!(
        request["params"],
        json!({"sessionId":"0b5d3c1e-7a42-4f0e-9d61-2c8e4f7a1b93","modeId":"default"})
    );
    let id = request_id(set_mode);
    runtime
        .resolve_egress(set_mode.token().clone(), EgressResolution::Written)
        .unwrap();
    let ready = runtime
        .ingest(&answer("setModeAccepted", id), &Mapper)
        .unwrap();
    assert!(matches!(
        ready.events(),
        [SharedEvent::SessionReady { loaded: false, .. }]
    ));
    assert_eq!(current_mode(&runtime), "default");
    // The new boolean option is presented as a boolean control.
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    let ultracode = config["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "ultracode")
        .expect("ultracode option");
    assert_eq!(ultracode["type"], "boolean");
    assert_eq!(ultracode["currentValue"], false);
}

#[test]
fn a_refused_starting_mode_fails_the_session_start() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let batch = new_session(&mut runtime, frame(derived("newSessionInAuto")));
    let id = request_id(&batch.egress()[0]);
    runtime
        .resolve_egress(batch.egress()[0].token().clone(), EgressResolution::Written)
        .unwrap();
    let refused = runtime.ingest(
        &frame(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Invalid params"}})),
        &Mapper,
    );
    assert!(matches!(
        refused,
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert!(runtime.session_config().is_none());
}

#[test]
fn a_session_already_in_sages_mode_is_ready_at_once_and_a_catalog_without_it_fails_closed() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let mut manual = derived("newSessionInAuto");
    manual["result"]["modes"]["currentModeId"] = json!("default");
    let batch = new_session(&mut runtime, frame(manual));
    assert!(matches!(batch.events(), [SharedEvent::SessionReady { .. }]));
    assert!(batch.egress().is_empty());

    let mut without = derived("newSessionInAuto");
    without["result"]["modes"]["availableModes"] = json!([
        {"id":"auto","name":"Auto"}
    ]);
    without["result"]["configOptions"] = json!([]);
    assert_session_start_fails_closed(without);
}

/// Sends `session/new`, answers it with `answer`, and checks that the start
/// is refused and the half-started session is dropped.
fn assert_session_start_fails_closed(mut answer: Value) {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    answer["id"] = json!(request_id(&egress));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(matches!(
        runtime.ingest(&frame(answer), &Mapper),
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert!(
        runtime.session_config().is_none(),
        "the half-started session is dropped"
    );
}

#[test]
fn a_session_without_a_mode_catalog_fails_closed() {
    let mut answer = derived("newSessionInAuto");
    answer["result"].as_object_mut().unwrap().remove("modes");
    assert_session_start_fails_closed(answer);
}

#[test]
fn activation_writes_the_starting_mode_and_commits_ready_only_after_it_is_accepted() {
    use super::resume_regression_tests::{Journal, Sink};
    let mut probe = ClaudeRuntime::fixture();
    initialize_0101(&mut probe);
    let ceiling = probe.capabilities().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = activation_intent(
            ActivationMode::New {
                native_root: r"C:\fixture\root".into(),
                model: None,
            },
            ceiling,
        );
        let mut pump =
            ActivationPump::new(ClaudeRuntime::fixture(), Journal, Mapper, intent).unwrap();
        pump.begin().unwrap();
        pump.flush_one(&mut Sink).unwrap();
        pump.push_stdout_chunk(&line(derived("initialize")), 1)
            .await
            .unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let mut session = derived("newSessionInAuto");
        session["id"] = json!(2);
        pump.push_stdout_chunk(&line(session), 2).await.unwrap();
        assert!(
            pump.try_into_idle_parts().is_err(),
            "not ready before the mode is set"
        );
    });
}

#[test]
fn activation_hands_off_once_the_starting_mode_is_accepted() {
    use super::resume_regression_tests::{Journal, Sink};
    let mut probe = ClaudeRuntime::fixture();
    initialize_0101(&mut probe);
    let ceiling = probe.capabilities().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = activation_intent(
            ActivationMode::New {
                native_root: r"C:\fixture\root".into(),
                model: None,
            },
            ceiling,
        );
        let mut pump =
            ActivationPump::new(ClaudeRuntime::fixture(), Journal, Mapper, intent).unwrap();
        pump.begin().unwrap();
        pump.flush_one(&mut Sink).unwrap();
        pump.push_stdout_chunk(&line(derived("initialize")), 1)
            .await
            .unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let mut session = derived("newSessionInAuto");
        session["id"] = json!(2);
        pump.push_stdout_chunk(&line(session), 2).await.unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let mut accepted = derived("setModeAccepted");
        accepted["id"] = json!(3);
        pump.push_stdout_chunk(&line(accepted), 3).await.unwrap();
        let parts = pump
            .try_into_idle_parts()
            .ok()
            .expect("ready after the starting mode");
        let (runtime, _, _, _) = parts.into_parts();
        assert_eq!(current_mode(&runtime), "default");
    });
}
