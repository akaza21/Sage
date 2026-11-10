//! Codex skills as the session's `/` commands: listed by `skills/list`,
//! listed again after `skills/changed`, and run by attaching the skill.
use std::sync::Arc;

use serde_json::{Map, Value};

use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::request::OutgoingRequest,
        standard_acp::surface::command_list::{MAX_COMMAND_BYTES, MAX_COMMANDS},
    },
    intent::Prompt,
    slash_commands::{AvailableCommand, CommandDispatch, CommandSource, CommandState},
};

/// A skill's harness path; a longer one is left out of the list.
const PATH_BYTES: usize = 1024;

/// Clones share the live list; equal when they share it and want the same.
#[derive(Clone)]
pub(super) struct Skills {
    list: Arc<CommandState>,
    /// The list is missing or stale and no read is pending.
    wanted: bool,
}

impl PartialEq for Skills {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.list, &other.list) && self.wanted == other.wanted
    }
}

impl std::fmt::Debug for Skills {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skills")
            .field("revision", &self.list.snapshot().revision)
            .field("wanted", &self.wanted)
            .finish()
    }
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            list: Arc::new(CommandState::default()),
            wanted: true,
        }
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// The enabled skills a `skills/list` result names, first of each name, in
/// listed order. A skill Sage cannot list is left out, not the whole list,
/// and the list ends at the first skill past the shared count or byte bound.
fn parse(result: &Map<String, Value>) -> Result<Vec<AvailableCommand>, AdapterRuntimeError> {
    let entries = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(AdapterRuntimeError::Frame)?;
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0usize;
    let skills = entries
        .iter()
        .filter_map(|entry| entry.get("skills")?.as_array())
        .flatten()
        .filter(|skill| skill.get("enabled").and_then(Value::as_bool) == Some(true))
        .filter_map(|skill| {
            let name = text(skill.get("name"))?;
            let path = text(skill.get("path")).filter(|path| path.len() <= PATH_BYTES)?;
            let description = text(skill.pointer("/interface/shortDescription"))
                .or_else(|| text(skill.get("shortDescription")))
                .or_else(|| text(skill.get("description")))
                .unwrap_or_default();
            let dispatch = CommandDispatch::Skill {
                path: path.to_owned(),
            };
            AvailableCommand::new(name, description, None, CommandSource::Skill, dispatch).ok()
        })
        .filter(|command| names.insert(command.name.clone()))
        .take(MAX_COMMANDS)
        .take_while(|command| {
            bytes += command.listed_bytes();
            bytes <= MAX_COMMAND_BYTES
        })
        .collect();
    Ok(skills)
}

impl CodexAdapter {
    pub(crate) fn skills(&self) -> Arc<CommandState> {
        Arc::clone(&self.skills.list)
    }

    /// The list is missing or stale, and no read of it is pending.
    pub(crate) fn skills_wanted(&self) -> bool {
        self.negotiated.is_some() && self.skills.wanted && !self.skills_read_pending()
    }

    pub(crate) fn skills_list(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if !self.skills_wanted() {
            return Err(AdapterError::TurnIntentConflict);
        }
        let request = self.reserve(OutgoingRequest::skills_list(id)?)?;
        self.skills.wanted = false;
        Ok(request)
    }

    /// A refused read empties the list, since a listed path may be gone;
    /// Codex says when skills change. A list equal to the shown one keeps
    /// its revision, so a pick made from it still runs.
    pub(crate) fn skills_result(&mut self, result: Option<&Map<String, Value>>) {
        let parsed = result.map_or_else(|| Ok(Vec::new()), parse);
        if parsed.as_ref().ok() != Some(&self.skills.list.snapshot().commands) {
            let _ = self.skills.list.replace(parsed);
        }
    }

    pub(crate) fn skills_read_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, kind)| *kind == RequestKind::SkillsList)
    }

    /// A skills reply past the frame bound is a refused read, not a broken
    /// session. The decoder reports only response frames here, so the id is
    /// the read's own.
    pub(crate) fn skills_oversized(&mut self, id: u64) -> bool {
        if self.pending_kind(id) != Some(RequestKind::SkillsList) {
            return false;
        }
        self.take_pending(id);
        self.skills_result(None);
        true
    }

    pub(crate) fn skills_changed(&mut self) {
        self.skills.wanted = true;
    }

    /// The listed skill this prompt runs, as its name and path.
    pub(crate) fn skill_input(&self, prompt: &Prompt) -> Option<(String, String)> {
        let revision = prompt.command_revision?;
        let listed = self.skills.list.snapshot();
        let command = listed.find(revision, prompt.as_str())?;
        match &command.dispatch {
            CommandDispatch::Skill { path } => Some((command.name.clone(), path.clone())),
            _ => None,
        }
    }
}
