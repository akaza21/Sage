//! Branching a Pi chat: `clone`, read the copy's file, switch back.

use super::*;
use crate::acp::goal::NativeRpcReply;

const HOME: &str = "2026-11-11T10-00-00-000Z_019a2b3c-4d5e-7f00-8a9b-0c1d2e3f4a5b";
const COPY: &str = "2026-11-11T10-05-00-000Z_019a2b3c-0000-7f00-8a9b-0c1d2e3f4a5c";

struct Chat {
    runtime: PiRuntime,
    dir: std::path::PathBuf,
}

impl Drop for Chat {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Chat {
    fn file(&self, key: &str) -> std::path::PathBuf {
        self.dir.join(format!("{key}.jsonl"))
    }
}

/// A chat whose Pi session has a saved file, as after its first reply.
fn saved_chat() -> Chat {
    let dir = std::env::temp_dir().join(format!("sage-pi-fork-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut runtime = fixture_runtime(1);
    let home = dir.join(format!("{HOME}.jsonl"));
    initialize_with(
        &mut runtime,
        json!({"model":{"provider":"openai","id":"gpt","name":"GPT"},"thinkingLevel":"high",
            "sessionFile":home}),
    );
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let value = deliver(&mut runtime, session);
    runtime
        .ingest(&response(&value, json!({"sessionFile":home})), &Mapper)
        .unwrap();
    std::fs::write(&home, "{}\n").unwrap();
    Chat { runtime, dir }
}

/// Answers `request` and returns the next request the runtime wrote, if any.
fn answer(chat: &mut Chat, request: &Value, data: Value) -> Option<Value> {
    let batch = chat
        .runtime
        .ingest(&response(request, data), &Mapper)
        .unwrap();
    let (_, mut egress) = batch.into_parts();
    assert!(egress.len() <= 1);
    egress.pop().map(|next| deliver(&mut chat.runtime, next))
}

/// Pi clones the chat; returns its `get_state` request for the copy.
fn cloned(chat: &mut Chat) -> Value {
    let fork = chat.runtime.fork_session().unwrap();
    let clone = deliver(&mut chat.runtime, fork);
    assert_eq!(clone, json!({"type":"clone","id":clone["id"]}));
    let state = answer(chat, &clone, json!({"cancelled":false})).unwrap();
    assert_eq!(state, json!({"type":"get_state","id":state["id"]}));
    state
}

/// Pi reports the copy it moved onto; returns the switch back to the chat.
fn read_copy(chat: &mut Chat, state: &Value) -> Value {
    let copy = chat.file(COPY);
    std::fs::write(&copy, "{}\n").unwrap();
    let back = answer(chat, state, json!({"sessionFile":copy})).unwrap();
    assert_eq!(
        back,
        json!({"type":"switch_session","id":back["id"],
            "sessionPath":chat.file(HOME).to_str().unwrap()})
    );
    back
}

#[test]
fn pi_advertises_forking() {
    let chat = saved_chat();
    assert!(chat.runtime.capabilities().unwrap().native_session_fork);
}

#[test]
fn a_fork_clones_reads_the_copy_and_moves_pi_back_to_the_chat() {
    let mut chat = saved_chat();
    let state = cloned(&mut chat);
    assert!(chat.runtime.native_rpc_pending());
    let back = read_copy(&mut chat, &state);
    assert!(answer(&mut chat, &back, json!({"cancelled":false})).is_none());
    assert!(!chat.runtime.native_rpc_pending());
    let Some(NativeRpcReply::Forked(fork)) = chat.runtime.take_native_rpc_reply() else {
        panic!("fork reply expected");
    };
    assert_eq!(fork.as_str(), COPY);
    // The fork chat now owns the copy; it stays on disk.
    chat.runtime.release_fork(&fork, true).unwrap();
    assert_eq!(
        chat.runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(chat.file(COPY).is_file());
    // The chat is usable again.
    running(&mut chat.runtime);
}

#[test]
fn a_refused_fork_removes_pis_copy() {
    let mut chat = saved_chat();
    let state = cloned(&mut chat);
    let back = read_copy(&mut chat, &state);
    answer(&mut chat, &back, json!({"cancelled":false}));
    let Some(NativeRpcReply::Forked(fork)) = chat.runtime.take_native_rpc_reply() else {
        panic!("fork reply expected");
    };
    chat.runtime.release_fork(&fork, false).unwrap();
    assert_eq!(
        chat.runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(!chat.file(COPY).exists());
    assert!(chat.file(HOME).is_file());
    // Only forks this runtime made can be released, and only once.
    assert!(chat.runtime.release_fork(&fork, false).is_err());
}

#[test]
fn a_clone_pi_cancels_leaves_the_chat_where_it_was() {
    let mut chat = saved_chat();
    let fork = chat.runtime.fork_session().unwrap();
    let clone = deliver(&mut chat.runtime, fork);
    assert!(answer(&mut chat, &clone, json!({"cancelled":true})).is_none());
    assert_eq!(
        chat.runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    running(&mut chat.runtime);
}

#[test]
fn the_chat_cannot_prompt_or_change_settings_until_pi_is_back() {
    let mut chat = saved_chat();
    cloned(&mut chat);
    assert!(matches!(
        chat.runtime.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "hello").unwrap(),
            client_message_id: RequiredText::new("client", "client-pi-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::Operation)
    ));
    assert!(chat.runtime.read_usage().is_err());
    assert!(chat.runtime.fork_session().is_err());
}

#[test]
fn a_fork_its_caller_gave_up_on_still_moves_pi_back_and_removes_the_copy() {
    let mut chat = saved_chat();
    let state = cloned(&mut chat);
    chat.runtime.abandon_passive_read();
    assert!(!chat.runtime.native_rpc_pending());
    let back = read_copy(&mut chat, &state);
    assert!(answer(&mut chat, &back, json!({"cancelled":false})).is_none());
    assert_eq!(chat.runtime.take_native_rpc_reply(), None);
    assert!(!chat.file(COPY).exists());
    running(&mut chat.runtime);
}

#[test]
fn pi_left_on_the_copy_stops_the_runtime() {
    let mut chat = saved_chat();
    let state = cloned(&mut chat);
    let back = read_copy(&mut chat, &state);
    assert!(
        chat.runtime
            .ingest(&response(&back, json!({"cancelled":true})), &Mapper)
            .is_err()
    );
    assert!(chat.runtime.read_usage().is_err());
    assert!(chat.runtime.fork_session().is_err());
}

#[test]
fn a_chat_pi_never_saved_has_nothing_to_fork() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    assert!(runtime.fork_session().is_err());
    assert!(!runtime.native_rpc_pending());
}

#[test]
fn a_late_answer_to_an_abandoned_read_does_not_disturb_a_fork() {
    let mut chat = saved_chat();
    let read = chat.runtime.read_usage().unwrap();
    let stats = deliver(&mut chat.runtime, read);
    chat.runtime.abandon_passive_read();
    let state = cloned(&mut chat);
    // The stats Pi answers late belong to no one; the fork carries on.
    let late = chat
        .runtime
        .ingest(
            &response(
                &stats,
                json!({"tokens":{"input":1,"output":1,"cacheRead":0,
                "cacheWrite":0,"total":2},"cost":0}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(late.egress().is_empty());
    let back = read_copy(&mut chat, &state);
    answer(&mut chat, &back, json!({"cancelled":false}));
    assert!(matches!(
        chat.runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(_))
    ));
}

#[test]
fn a_copy_named_like_the_chats_own_file_is_never_taken_as_the_fork() {
    let mut chat = saved_chat();
    let state = cloned(&mut chat);
    let home = chat.file(HOME);
    let back = answer(&mut chat, &state, json!({"sessionFile":home})).unwrap();
    assert!(answer(&mut chat, &back, json!({"cancelled":false})).is_none());
    assert_eq!(
        chat.runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    assert!(chat.file(HOME).is_file());
}

#[test]
fn an_unwritten_clone_leaves_the_chat_usable() {
    let mut chat = saved_chat();
    let fork = chat.runtime.fork_session().unwrap();
    chat.runtime
        .resolve_egress(fork.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(!chat.runtime.native_rpc_pending());
    running(&mut chat.runtime);
}

#[test]
fn an_unwritten_step_after_the_clone_stops_the_runtime() {
    let mut chat = saved_chat();
    let fork = chat.runtime.fork_session().unwrap();
    let clone = deliver(&mut chat.runtime, fork);
    let batch = chat
        .runtime
        .ingest(&response(&clone, json!({"cancelled":false})), &Mapper)
        .unwrap();
    let (_, mut egress) = batch.into_parts();
    chat.runtime
        .resolve_egress(
            egress.remove(0).into_token(),
            EgressResolution::QueueRejected,
        )
        .unwrap();
    // Pi is on the copy and nothing will move it back.
    assert!(chat.runtime.read_usage().is_err());
    assert!(chat.runtime.fork_session().is_err());
}

#[test]
fn forks_waiting_on_their_chats_are_bounded() {
    let mut chat = saved_chat();
    for index in 0..8 {
        let state = cloned(&mut chat);
        let copy = chat.file(&format!("2026-11-11T11-00-0{index}-000Z_copy-{index}"));
        std::fs::write(&copy, "{}\n").unwrap();
        let back = answer(&mut chat, &state, json!({"sessionFile":copy})).unwrap();
        answer(&mut chat, &back, json!({"cancelled":false}));
        assert!(matches!(
            chat.runtime.take_native_rpc_reply(),
            Some(NativeRpcReply::Forked(_))
        ));
    }
    assert!(matches!(
        chat.runtime.fork_session(),
        Err(AdapterRuntimeError::BatchCount)
    ));
}
