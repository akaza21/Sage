//! Reading Claude Code's saved transcripts: one JSON entry per line, each
//! message linked by `parentUuid` to the one it follows. A resumed session
//! appends to its transcript, so a thread cut by `--resume-session-at`
//! leaves its abandoned branch in the file; only the parent chain says
//! which reply a prompt followed.

use std::{
    collections::HashMap,
    io::BufRead,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use uuid::Uuid;

/// Parent links followed from a prompt back to its reply. Claude Code puts
/// at most a few system or attachment entries between them.
const MAX_HOPS: usize = 16;

/// Entries a cut remembers before giving up on the transcript; each costs
/// a few dozen bytes, so the bound keeps a runaway file from growing Sage.
const MAX_ENTRIES: usize = 200_000;

/// The fields of a transcript entry Sage reads; the rest of the line,
/// message content included, is skipped without being kept. Ids and types
/// carry no escapes, so they borrow from the line.
#[derive(Deserialize)]
struct Entry<'a> {
    #[serde(rename = "type", default)]
    kind: Option<&'a str>,
    #[serde(default)]
    uuid: Option<&'a str>,
    #[serde(rename = "parentUuid", default)]
    parent: Option<&'a str>,
    #[serde(rename = "isSidechain", default)]
    sidechain: Option<bool>,
}

impl Entry<'_> {
    fn id(&self) -> Option<Uuid> {
        self.uuid.and_then(parse_uuid)
    }

    fn is_main_reply(&self) -> bool {
        self.kind == Some("assistant") && self.sidechain != Some(true)
    }
}

/// A session's transcript: Claude Code keeps each project's sessions in
/// their own directory under `projects`.
pub(super) fn transcript(config: &Path, session: &str) -> Option<PathBuf> {
    let name = format!("{session}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|project| project.path().join(&name))
        .find(|path| path.is_file())
}

pub(super) fn is_uuid(value: &str) -> bool {
    parse_uuid(value).is_some()
}

/// Only the canonical hyphenated form: the id goes back to Claude Code as
/// written.
fn parse_uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|parsed| parsed.hyphenated().to_string() == value)
}

/// The id of the transcript's last assistant message in the main thread.
pub(super) fn last_reply(path: &Path) -> Option<String> {
    let mut last = None;
    for_each_entry(path, |entry| {
        if entry.is_main_reply() {
            last = entry.id().or(last);
        }
        true
    })?;
    last.map(|id| id.hyphenated().to_string())
}

/// The reply the resumed thread in `path` is cut at to drop the undone
/// `prompt` and all after it: the nearest main-thread assistant message up
/// the prompt's parent chain. None for a first prompt, or when the
/// transcript cannot be read or the chain cannot be followed.
///
/// One forward pass records each entry's parent until the prompt is found;
/// a parent always precedes its child, so the chain is then in the map.
pub(super) fn cut_at(path: &Path, prompt: &str) -> Option<String> {
    let prompt = parse_uuid(prompt)?;
    // Each entry's parent, and whether it is a main-thread reply.
    let mut entries: HashMap<Uuid, (Option<Uuid>, bool)> = HashMap::new();
    let mut start = None;
    for_each_entry(path, |entry| {
        let Some(id) = entry.id() else {
            return true;
        };
        let parent = entry.parent.and_then(parse_uuid);
        if id == prompt {
            start = (entry.kind == Some("user")).then_some(parent).flatten();
            return false;
        }
        entries.insert(id, (parent, entry.is_main_reply()));
        entries.len() < MAX_ENTRIES
    })?;
    let mut next = start?;
    for _ in 0..MAX_HOPS {
        let &(parent, is_reply) = entries.get(&next)?;
        if is_reply {
            return Some(next.hyphenated().to_string());
        }
        next = parent?;
    }
    None
}

/// Calls `visit` with each parsed entry, one line at a time, until it
/// returns false. A line that is not an entry is skipped; an unreadable one
/// ends the pass. None when the file cannot be opened.
fn for_each_entry(path: &Path, mut visit: impl FnMut(Entry<'_>) -> bool) -> Option<()> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|read| read > 0) {
        if let Ok(entry) = serde_json::from_str::<Entry<'_>>(&line)
            && !visit(entry)
        {
            break;
        }
        line.clear();
    }
    Some(())
}
