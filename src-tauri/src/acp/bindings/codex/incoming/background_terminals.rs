use serde_json::Value;

use super::{
    RequestKind, ResourceMapper,
    support::{command_cwd, sanitized_command_line},
};
use crate::acp::{
    background_terminals::{
        BackgroundTerminal, MAX_BACKGROUND_TERMINALS, TERMINAL_COMMAND_BYTES, valid_handle,
    },
    goal::NativeRpcReply,
};

/// The reply for one answered request; `None` for a shape outside the
/// pinned schema. A listing skips an entry it cannot read and keeps its
/// first terminals past the cap.
pub(super) fn reply<M: ResourceMapper>(
    kind: RequestKind,
    result: &Value,
    mapper: &M,
) -> Option<NativeRpcReply> {
    let result = result.as_object()?;
    match kind {
        RequestKind::TerminalsList => Some(NativeRpcReply::BackgroundTerminals(
            result
                .get("data")?
                .as_array()?
                .iter()
                .filter_map(|terminal| listed(terminal, mapper))
                .take(MAX_BACKGROUND_TERMINALS)
                .collect(),
        )),
        // A terminal that already exited reports `terminated: false`; the
        // next listing shows either way.
        RequestKind::TerminalsStop => result
            .get("terminated")?
            .is_boolean()
            .then_some(NativeRpcReply::TerminalsStopped),
        RequestKind::TerminalsStopAll => Some(NativeRpcReply::TerminalsStopped),
        _ => None,
    }
}

fn listed<M: ResourceMapper>(terminal: &Value, mapper: &M) -> Option<BackgroundTerminal> {
    let terminal = terminal.as_object()?;
    let handle = terminal
        .get("processId")?
        .as_str()
        .filter(|handle| valid_handle(handle))?;
    let mut command = sanitized_command_line(terminal.get("command")?.as_str()?).ok()?;
    if command.len() > TERMINAL_COMMAND_BYTES {
        let mut end = TERMINAL_COMMAND_BYTES;
        while !command.is_char_boundary(end) {
            end -= 1;
        }
        command.truncate(end);
    }
    Some(BackgroundTerminal {
        handle: handle.to_owned(),
        command,
        cwd: command_cwd(terminal, mapper),
    })
}
