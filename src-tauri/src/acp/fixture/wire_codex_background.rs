use std::io::Write;

use serde_json::{Value, json};

use super::super::FixtureError;
use super::codex::emit_json;
use super::{WireState, acp};

const MAX_SKILLS: usize = 16;
const MAX_CWDS: usize = 4;

/// The skills the replayed Codex install holds on disk, in Codex's
/// `SkillMetadata` shape, and the list Codex has cached from them: a change
/// on disk reaches only a read that skips the cache (`forceReload`). None
/// until a step installs some.
#[derive(Default)]
pub(super) struct Skills {
    on_disk: Vec<Value>,
    cached: Vec<Value>,
}

pub(super) fn answer(
    method: &str,
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<bool, FixtureError> {
    match method {
        "thread/turns/list" => child_read(request, output, state)?,
        "skills/list" => {
            let id = request_id(request)?;
            let data = listed(request, &mut state.codex_skills)?;
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
    params
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|child| matches!(*child, "fixture-child-1" | "fixture-child-2"))
        .ok_or(FixtureError::InvalidArguments)?;
    if params.get("itemsView").and_then(Value::as_str) != Some("full") {
        return Err(FixtureError::InvalidArguments);
    }
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","id":id,"result":{"nextCursor":null,"data":[{
            "id":"fixture-child-turn","itemsView":"full","status":"completed",
            "items":[{"id":"fixture-child-answer","type":"agentMessage","text":"Synthetic child summary"}]
        }]}}),
    )
}

/// One entry per cwd the read names, each holding the cached skills, as
/// Codex scopes repo skills to a cwd; a read that skips the cache reads the
/// disk first. An install with none lists nothing. The skills' paths are
/// under the case's project, where Sage launches the replay, which is the
/// cwd Sage asks about.
fn listed(request: &Value, skills: &mut Skills) -> Result<Vec<Value>, FixtureError> {
    if request.pointer("/params/forceReload") == Some(&json!(true)) {
        skills.cached = skills.on_disk.clone();
    }
    if skills.cached.is_empty() {
        return Ok(Vec::new());
    }
    let cwds = request
        .pointer("/params/cwds")
        .and_then(Value::as_array)
        .filter(|cwds| !cwds.is_empty() && cwds.len() <= MAX_CWDS)
        .ok_or(FixtureError::InvalidArguments)?;
    Ok(cwds
        .iter()
        .map(|cwd| json!({"cwd": cwd, "skills": skills.cached, "errors": []}))
        .collect())
}

/// Puts the skills a step names (`skills`) on disk, with `$ROOT` filled
/// in; `start` says Codex is starting, so it caches them as it loads.
pub(super) fn install(
    step: &Value,
    state: &mut WireState,
    start: bool,
) -> Result<(), FixtureError> {
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
    state.codex_skills.on_disk = skills.clone();
    if start {
        state.codex_skills.cached = skills.clone();
    }
    Ok(())
}

/// The skills changed on disk: the step's list replaces what is there, and
/// Codex says so with `skills/changed`, keeping its cache until a read
/// skips it.
pub(super) fn skills_changed(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if step.get("skills").is_none() {
        return Err(FixtureError::InvalidArguments);
    }
    install(step, state, false)?;
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","method":"skills/changed","params":{}}),
    )
}
