//! The Codex session's `/` commands: Codex's own commands that map to an
//! app-server method the session negotiated, then its skills, listed by
//! `skills/list`, listed again after `skills/changed`, and run by attaching
//! the skill.
use std::sync::Arc;

use serde_json::{Map, Value};

use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::{
    bindings::{
        codex::request::OutgoingRequest,
        standard_acp::surface::command_list::{MAX_COMMAND_BYTES, MAX_COMMANDS},
    },
    intent::{CapabilitySnapshotV1, Prompt},
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
    /// Codex said the skills changed, so reads skip its cache until one
    /// succeeds.
    reload: bool,
    /// A refused read was already retried once; reset by a good read.
    retried: bool,
    /// The bound thread's root, whose skills the menu lists.
    root: Option<String>,
}

impl PartialEq for Skills {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.list, &other.list)
            && (self.wanted, self.reload, self.retried)
                == (other.wanted, other.reload, other.retried)
            && self.root == other.root
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
            reload: false,
            retried: false,
            root: None,
        }
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// The enabled skills a `skills/list` result names, in listed order. A
/// skill Sage cannot list is left out, not the whole list.
fn parse(result: &Map<String, Value>) -> Option<Vec<AvailableCommand>> {
    let entries = result.get("data").and_then(Value::as_array)?;
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
        .collect();
    Some(skills)
}

/// Codex's own commands that run through an app-server method this session
/// negotiated, worded as Codex words them.
fn builtins(capabilities: &CapabilitySnapshotV1) -> Vec<AvailableCommand> {
    let builtin = |name, description, hint| {
        AvailableCommand::new(
            name,
            description,
            hint,
            CommandSource::Builtin,
            CommandDispatch::Action,
        )
    };
    [
        capabilities.native_session_compact.then(|| {
            builtin(
                "compact",
                "summarize conversation to prevent hitting the context limit",
                None,
            )
        }),
        capabilities.native_review.then(|| {
            builtin(
                "review",
                "review my current changes and find issues",
                Some("base <branch>, commit <sha>, or what to check"),
            )
        }),
    ]
    .into_iter()
    .flatten()
    .filter_map(Result::ok)
    .collect()
}

/// The built-ins, then the skills: first of each name, ending at the first
/// command past the shared count or byte bound.
fn bounded(commands: impl IntoIterator<Item = AvailableCommand>) -> Vec<AvailableCommand> {
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0usize;
    commands
        .into_iter()
        .filter(|command| names.insert(command.name.clone()))
        .take(MAX_COMMANDS)
        .take_while(|command| {
            bytes += command.listed_bytes();
            bytes <= MAX_COMMAND_BYTES
        })
        .collect()
}

impl CodexAdapter {
    pub(crate) fn skills(&self) -> Arc<CommandState> {
        Arc::clone(&self.skills.list)
    }

    /// The thread's root is known, the list is missing or stale, and no
    /// read of it is pending.
    pub(crate) fn skills_wanted(&self) -> bool {
        self.negotiated.is_some()
            && self.skills.root.is_some()
            && self.skills.wanted
            && !self.skills_read_pending()
    }

    /// The skills come from the thread's root, as Codex scopes them.
    pub(crate) fn skills_root(&mut self, root: &str) {
        if self.skills.root.as_deref() != Some(root) {
            self.skills.root = Some(root.to_owned());
            self.skills.wanted = true;
        }
    }

    pub(crate) fn skills_list(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        let root = match &self.skills.root {
            Some(root) if self.skills_wanted() => root.clone(),
            _ => return Err(AdapterError::TurnIntentConflict),
        };
        let request = OutgoingRequest::skills_list(id, &root, self.skills.reload)?;
        let request = self.reserve(request)?;
        self.skills.wanted = false;
        Ok(request)
    }

    /// A refused or malformed read lists no skills, since a listed path may
    /// be gone, and is tried once more at the next idle moment; after that
    /// Codex says when skills change.
    pub(crate) fn skills_result(&mut self, result: Option<&Map<String, Value>>) {
        let skills = result.and_then(parse);
        match &skills {
            // A good read has skipped the cache; a retry still skips it.
            Some(_) => (self.skills.retried, self.skills.reload) = (false, false),
            None if !self.skills.retried => {
                self.skills.retried = true;
                self.skills.wanted = true;
            }
            None => {}
        }
        self.list_commands(skills.unwrap_or_default());
    }

    /// Lists the built-ins this session negotiated beside these skills. A
    /// list equal to the shown one keeps its revision, so a pick made from
    /// it still runs.
    pub(crate) fn list_commands(&mut self, skills: Vec<AvailableCommand>) {
        let builtins = self
            .negotiated
            .as_ref()
            .map(|methods| builtins(&methods.snapshot))
            .unwrap_or_default();
        let _ = self
            .skills
            .list
            .replace_if_changed(bounded(builtins.into_iter().chain(skills)));
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
        // The same catalogue would be as large again, so it is not retried.
        self.skills.retried = true;
        self.skills_result(None);
        true
    }

    pub(crate) fn skills_changed(&mut self) {
        self.skills.wanted = true;
        self.skills.reload = true;
        self.skills.retried = false;
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
