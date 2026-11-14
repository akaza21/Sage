use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use super::{
    StandardAcpHandshake, StandardAcpHandshakeError, StandardAcpProfile, admit,
    candidate_from_global_npm_shim, profile_for,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_LINES: usize = 32;
const SCRATCH_REMOVE_ATTEMPTS: usize = 20;
const SCRATCH_REMOVE_BACKOFF: Duration = Duration::from_millis(100);
const PROMPT_SETTLE_GRACE: Duration = Duration::from_millis(2500);
const DRAIN_POLL: Duration = Duration::from_millis(100);

#[test]
#[ignore = "requires exact SAGE_STANDARD_ACP_GEMINI_NODE and SAGE_STANDARD_ACP_GEMINI_ENTRYPOINT"]
fn real_gemini_initialize_then_session_new_reports_sign_in_required() {
    run_auth_refusal_probe(
        "gemini-cli",
        "SAGE_STANDARD_ACP_GEMINI_NODE",
        "SAGE_STANDARD_ACP_GEMINI_ENTRYPOINT",
    );
}

#[test]
#[ignore = "requires exact SAGE_STANDARD_ACP_CURSOR_NODE and SAGE_STANDARD_ACP_CURSOR_ENTRYPOINT"]
fn real_cursor_initialize_then_session_new_reports_sign_in_required() {
    run_auth_refusal_probe(
        "cursor-agent",
        "SAGE_STANDARD_ACP_CURSOR_NODE",
        "SAGE_STANDARD_ACP_CURSOR_ENTRYPOINT",
    );
}

fn run_auth_refusal_probe(harness_id: &str, runtime_key: &str, entrypoint_key: &str) {
    let profile = profile_for(harness_id).expect("curated standard ACP profile");
    let runtime_path = required_path(runtime_key);
    let entrypoint_path = required_path(entrypoint_key);
    let root = scratch_root(harness_id);
    let mut command = isolated_command(runtime_path, &root);
    command.arg(entrypoint_path);
    let mut child = spawn_acp(command, profile);
    let responses = response_stream(&mut child);
    let mut runtime = StandardAcpHandshake::new(profile);
    send(
        &mut child,
        runtime.initialize().expect("initialize request"),
    );
    let initialized = receive(&responses, 1);
    runtime
        .accept_initialize(&initialized)
        .expect("real binary initialize response");
    send(
        &mut child,
        runtime
            .new_session(root.to_str().expect("scratch path"))
            .expect("session request"),
    );
    let session_new = receive(&responses, 2);
    let result = runtime.accept_new_session(&session_new);
    finish(child, root);
    assert_eq!(result, Err(StandardAcpHandshakeError::SignInRequired));
}

#[test]
#[ignore = "requires exact SAGE_STANDARD_ACP_CURSOR_NODE and SAGE_STANDARD_ACP_CURSOR_ENTRYPOINT"]
fn real_cursor_still_answers_its_undocumented_acp_subcommand() {
    let root = scratch_root("cursor-handshake");
    let mut command = isolated_command(required_path("SAGE_STANDARD_ACP_CURSOR_NODE"), &root);
    command.arg(required_path("SAGE_STANDARD_ACP_CURSOR_ENTRYPOINT"));
    assert_acp_subcommand_survives("cursor-agent", command, root);
}

#[test]
#[ignore = "requires exact SAGE_STANDARD_ACP_OPENCODE_EXECUTABLE"]
fn real_opencode_still_answers_its_undocumented_acp_subcommand() {
    let root = scratch_root("opencode-handshake");
    let command = isolated_command(
        required_path("SAGE_STANDARD_ACP_OPENCODE_EXECUTABLE"),
        &root,
    );
    assert_acp_subcommand_survives("opencode", command, root);
}

#[test]
#[ignore = "requires the observed Windows global npm OpenCode installation"]
fn real_opencode_global_npm_shim_resolves_to_the_admitted_native_executable() {
    let app_data = std::env::var_os("APPDATA").expect("APPDATA");
    let shim = PathBuf::from(app_data).join("npm").join("opencode.cmd");
    let reported_version = crate::harness_discovery::revalidate_harness_version("opencode", &shim)
        .expect("OpenCode shim version");
    let candidate = candidate_from_global_npm_shim(&shim, reported_version)
        .expect("global npm OpenCode candidate");
    let expected = shim
        .parent()
        .expect("npm root")
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");

    assert_eq!(candidate.runtime.path, expected);
    let binding = admit(
        profile_for("opencode").expect("OpenCode profile"),
        candidate.clone(),
    )
    .expect("admitted OpenCode binding");
    let spec_root = scratch_root("opencode-launch-spec");
    let spec = super::AcpLaunchAuthority::from_admitted_binding(
        &binding,
        21,
        crate::acp::process::BoundedEnvironment::new(Vec::new()).expect("environment"),
    )
    .expect("launch authority")
    .into_launch_spec(
        crate::project_registry::ProjectRootAuthority::for_test(
            sage_acp_protocol::contract::SageProjectId::parse("project-opencode-real")
                .expect("project"),
            sage_acp_protocol::contract::ProjectGeneration::ZERO,
            sage_acp_protocol::contract::SageRootId::parse("root-opencode-real").expect("root"),
            &spec_root,
        )
        .expect("project root"),
    )
    .expect("launch spec");
    let recorded_manifest = spec
        .artifact
        .manifest
        .as_ref()
        .expect("the production launch spec carries the real package manifest");
    assert_eq!(
        recorded_manifest.path(),
        shim.parent()
            .expect("npm root")
            .join("node_modules")
            .join("opencode-ai")
            .join("package.json")
    );
    drop(
        recorded_manifest
            .open_guard()
            .expect("deny-write manifest guard over the real installation"),
    );
    drop(spec);
    std::fs::remove_dir_all(&spec_root).expect("remove launch spec scratch root");
    let root = scratch_root("opencode-admission");
    let command = isolated_command(candidate.runtime.path.into_os_string(), &root);
    assert_acp_subcommand_survives("opencode", command, root);
}

fn required_path(key: &str) -> OsString {
    std::env::var_os(key)
        .unwrap_or_else(|| panic!("{key} must name an installed binary before this probe can run"))
}

#[cfg(windows)]
#[test]
#[ignore = "requires the observed Windows global npm OpenCode installation"]
fn real_opencode_survives_its_production_launch_spec_environment() {
    use crate::acp::bindings::runtime::RuntimeFactory;

    let app_data = std::env::var_os("APPDATA").expect("APPDATA");
    let shim = PathBuf::from(app_data).join("npm").join("opencode.cmd");
    let reported_version = crate::harness_discovery::revalidate_harness_version("opencode", &shim)
        .expect("OpenCode shim version");
    let root = scratch_root("opencode-production-launch");
    let project_root = crate::project_registry::ProjectRootAuthority::for_test(
        sage_acp_protocol::contract::SageProjectId::parse("project-opencode-launch")
            .expect("project"),
        sage_acp_protocol::contract::ProjectGeneration::ZERO,
        sage_acp_protocol::contract::SageRootId::parse("root-opencode-launch").expect("root"),
        &root,
    )
    .expect("project root");
    let registration = std::sync::Arc::new(
        super::opencode::OpenCodeBindingRegistration::admitted_for_test(&shim, &reported_version),
    );
    let binding = registration.admitted().expect("admitted binding");
    let mut runtime = Box::new(super::StandardAcpRuntimeFactory::from_admitted_binding(
        registration,
        binding,
        project_root,
    ))
    .construct(
        sage_acp_protocol::contract::RuntimeEpoch::new(23).expect("epoch"),
        sage_acp_protocol::contract::SageAgentSessionId::parse("session-opencode-launch")
            .expect("session"),
    )
    .expect("construct runtime");
    let spec = runtime.launch_spec().expect("production launch spec");
    spec.project_root
        .revalidate()
        .expect("hr8 project root revalidate");
    drop(
        spec.artifact
            .manifest
            .as_ref()
            .expect("hr8 manifest")
            .open_guard()
            .expect("hr8 manifest guard"),
    );
    let pool = crate::acp::process::TreeCapacityPool::isolated_ade_0006a();
    let lease = pool
        .try_acquire(spec.limits.descendants)
        .expect("capacity lease");
    let tree = match crate::acp::process::PlatformSupervisor::launch(&spec, lease) {
        Ok(tree) => tree,
        Err(failure) => panic!(
            "the production launch spec must spawn the real OpenCode executable: {failure:?} env={:?}",
            spec.environment.as_slice()
        ),
    };
    let (_stdin, _stdout, control) = tree.into_parts();
    thread::sleep(Duration::from_millis(2500));
    let exit = control.native_exit_code_if_exited().expect("exit probe");
    control
        .terminate_zero(crate::acp::process::TreeExit::Cancelled)
        .expect("proven zero teardown");
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        exit, None,
        "the real OpenCode executable exited immediately under the production launch environment"
    );
}

#[test]
#[ignore = "requires exact SAGE_STANDARD_ACP_OPENCODE_EXECUTABLE"]
fn real_opencode_routes_the_standard_acp_cancel_and_close_session_surface() {
    let profile = profile_for("opencode").expect("curated standard ACP profile");
    let root = scratch_root("opencode-session-core");
    let command = isolated_command(
        required_path("SAGE_STANDARD_ACP_OPENCODE_EXECUTABLE"),
        &root,
    );
    let mut child = spawn_acp(command, profile);
    let responses = response_stream(&mut child);
    let mut runtime = StandardAcpHandshake::new(profile);
    send(
        &mut child,
        runtime.initialize().expect("initialize request"),
    );
    let initialized = receive(&responses, 1);
    assert!(
        initialized["result"]["agentCapabilities"]["sessionCapabilities"]
            .get("close")
            .is_some(),
        "the real OpenCode build no longer advertises sessionCapabilities.close"
    );
    runtime
        .accept_initialize(&initialized)
        .expect("real binary initialize response");
    send(
        &mut child,
        runtime
            .new_session(root.to_str().expect("scratch path"))
            .expect("session request"),
    );
    let session_id = runtime
        .accept_new_session(&receive(&responses, 2))
        .expect("real binary opens a session without an account");
    send(
        &mut child,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "Count slowly from 1 to 200, one number per line."}]
            }
        }),
    );
    let early = drain_for(&responses, PROMPT_SETTLE_GRACE);
    assert!(
        !early
            .iter()
            .any(|value| value.get("id").and_then(Value::as_u64) == Some(3)),
        "the real OpenCode build settled the prompt before Sage could cancel it"
    );
    send(
        &mut child,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": session_id}
        }),
    );
    let settled = receive(&responses, 3);
    send(
        &mut child,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "session/close",
            "params": {"sessionId": session_id}
        }),
    );
    let closed = receive(&responses, 4);
    finish(child, root);

    assert_eq!(
        settled["result"]["stopReason"], "cancelled",
        "the real OpenCode build no longer honours the session/cancel notification: {settled}"
    );
    assert!(
        closed["result"].is_object(),
        "the real OpenCode build no longer answers session/close with a result: {closed}"
    );
}

fn drain_for(responses: &Receiver<Value>, window: Duration) -> Vec<Value> {
    let deadline = SystemTime::now() + window;
    let mut seen = Vec::new();
    while SystemTime::now() < deadline {
        match responses.recv_timeout(DRAIN_POLL) {
            Ok(value) => seen.push(value),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    seen
}

fn assert_acp_subcommand_survives(harness_id: &str, command: Command, root: PathBuf) {
    let profile = profile_for(harness_id).expect("curated standard ACP profile");
    let mut child = spawn_acp(command, profile);
    let responses = response_stream(&mut child);
    let mut runtime = StandardAcpHandshake::new(profile);
    send(
        &mut child,
        runtime.initialize().expect("initialize request"),
    );
    let initialized = receive(&responses, 1);
    runtime
        .accept_initialize(&initialized)
        .expect("real binary initialize response");
    send(
        &mut child,
        runtime
            .new_session(root.to_str().expect("scratch path"))
            .expect("session request"),
    );
    let session_new = receive(&responses, 2);
    let result = runtime.accept_new_session(&session_new);
    finish(child, root);
    match result {
        Ok(session_id) => assert!(
            !session_id.is_empty(),
            "{harness_id} opened a session with an empty id"
        ),
        Err(StandardAcpHandshakeError::SignInRequired) => {}
        Err(other) => {
            panic!("{harness_id} no longer speaks ACP over its acp subcommand: {other:?}")
        }
    }
}

fn isolated_command(program: OsString, root: &PathBuf) -> Command {
    for state in [
        root.join("appdata"),
        root.join("localappdata"),
        root.join("home"),
        root.join("home").join("config"),
        root.join("home").join("data"),
        root.join("gemini"),
        root.join("cursor"),
    ] {
        std::fs::create_dir_all(state).expect("create isolated state");
    }
    let mut command = Command::new(program);
    command
        .current_dir(root)
        .env("APPDATA", root.join("appdata"))
        .env("LOCALAPPDATA", root.join("localappdata"))
        .env("USERPROFILE", root.join("home"))
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("home").join("config"))
        .env("XDG_DATA_HOME", root.join("home").join("data"))
        .env("GEMINI_CLI_HOME", root.join("gemini"))
        .env("GEMINI_CLI_NO_RELAUNCH", "true")
        .env("CURSOR_CONFIG_DIR", root.join("cursor"))
        .env("NO_OPEN_BROWSER", "1")
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY");
    command
}

fn spawn_acp(mut command: Command, profile: &'static StandardAcpProfile) -> Child {
    command
        .args(profile.fixed_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("the real ACP binary could not be started: {error}"))
}

fn response_stream(child: &mut Child) -> Receiver<Value> {
    let stdout = child.stdout.take().expect("child stdout");
    let (sender, receiver) = channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().take(MAX_RESPONSE_LINES) {
            let Ok(line) = line else {
                break;
            };
            if let Ok(value) = serde_json::from_str(&line)
                && sender.send(value).is_err()
            {
                break;
            }
        }
    });
    receiver
}

fn send(child: &mut Child, value: Value) {
    let stdin = child.stdin.as_mut().expect("child stdin");
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(&value).expect("serialize frame")
    )
    .expect("write frame");
    stdin.flush().expect("flush frame");
}

fn receive(responses: &Receiver<Value>, id: u64) -> Value {
    loop {
        match responses.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(value) if value.get("id").and_then(Value::as_u64) == Some(id) => return value,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => panic!("real ACP response timed out"),
            Err(RecvTimeoutError::Disconnected) => panic!("real ACP response stream closed"),
        }
    }
}

fn scratch_root(harness_id: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("sage-standard-acp-{harness_id}-{nonce}"));
    std::fs::create_dir_all(&root).expect("create scratch root");
    root
}

fn finish(mut child: Child, root: PathBuf) {
    let _ = child.kill();
    let _ = child.wait();
    for attempt in 1..=SCRATCH_REMOVE_ATTEMPTS {
        match std::fs::remove_dir_all(&root) {
            Ok(()) => return,
            Err(_) if attempt < SCRATCH_REMOVE_ATTEMPTS => thread::sleep(SCRATCH_REMOVE_BACKOFF),
            Err(error) => panic!(
                "{} is still held after the harness was killed and reaped, so the harness leaked a live process: {error}",
                root.display()
            ),
        }
    }
}
