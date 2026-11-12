//! Requests Sage sends a Codex replay outside the scripted steps: a child
//! thread's read and the skills read while a session is idle, with the
//! skills the replayed install holds.

use std::io::Write;

use serde_json::{Value, json};

use super::super::FixtureError;
use super::codex::{THREAD_ID, emit_json};
use super::{WireState, acp};

/// Skills an install may hold, and cwds one read may name.
const MAX_SKILLS: usize = 16;
const MAX_CWDS: usize = 4;

/// The skills the replayed Codex install holds, in Codex's `SkillMetadata`
/// shape; none until a step installs some.
#[derive(Default)]
pub(super) struct Skills(Vec<Value>);

/// Answers `request` when it is a background read, and says whether it was.
pub(super) fn answer(
    method: &str,
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<bool, FixtureError> {
    match method {
        "thread/read" => child_read(request, output, state)?,
        "skills/list" => {
            let id = request_id(request)?;
            let data = listed(request, &state.codex_skills)?;
            emit_json(
                output,
                state,
                &json!({"jsonrpc":"2.0","id":id,"result":{"data":data}}),
            )?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn request_id(request: &Value) -> Result<u64, FixtureError> {
    request
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(FixtureError::InvalidArguments)
}

fn child_read(
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    let id = request_id(request)?;
    let params = request
        .get("params")
        .ok_or(FixtureError::InvalidArguments)?;
    let child = params
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|child| matches!(*child, "fixture-child-1" | "fixture-child-2"))
        .ok_or(FixtureError::InvalidArguments)?;
    if params.get("includeTurns").and_then(Value::as_bool) != Some(true) {
        return Err(FixtureError::InvalidArguments);
    }
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","id":id,"result":{"thread":{
            "id":child,"parentThreadId":THREAD_ID,"turns":[{
                "id":"fixture-child-turn","itemsView":"full","status":"completed",
                "items":[{"id":"fixture-child-answer","type":"agentMessage","text":"Synthetic child summary"}]
            }]
        }}}),
    )
}

/// One entry per cwd the read names, each holding the installed skills, as
/// Codex scopes repo skills to a cwd. An install with none lists nothing.
fn listed(request: &Value, skills: &Skills) -> Result<Vec<Value>, FixtureError> {
    if skills.0.is_empty() {
        return Ok(Vec::new());
    }
    let cwds = request
        .pointer("/params/cwds")
        .and_then(Value::as_array)
        .filter(|cwds| !cwds.is_empty() && cwds.len() <= MAX_CWDS)
        .ok_or(FixtureError::InvalidArguments)?;
    Ok(cwds
        .iter()
        .map(|cwd| json!({"cwd": cwd, "skills": skills.0, "errors": []}))
        .collect())
}

/// Installs the skills a step names (`skills`), with `$ROOT` filled in.
pub(super) fn install(step: &Value, state: &mut WireState) -> Result<(), FixtureError> {
    let Some(skills) = step.get("skills") else {
        return Ok(());
    };
    let skills = acp::substitute(skills.clone(), state, 0)?;
    let skills = skills
        .as_array()
        .filter(|skills| skills.len() <= MAX_SKILLS)
        .filter(|skills| {
            skills.iter().all(|skill| {
                skill["name"].is_string()
                    && skill["path"].is_string()
                    && skill["enabled"].is_boolean()
            })
        })
        .ok_or(FixtureError::InvalidArguments)?;
    state.codex_skills = Skills(skills.clone());
    Ok(())
}

/// The skills changed on disk: the step's list replaces the installed one,
/// and Codex says so with `skills/changed`.
pub(super) fn skills_changed(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if step.get("skills").is_none() {
        return Err(FixtureError::InvalidArguments);
    }
    install(step, state)?;
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","method":"skills/changed","params":{}}),
    )
}
