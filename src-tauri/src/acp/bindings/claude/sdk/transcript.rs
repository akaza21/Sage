//! Reading Claude Code's saved transcripts: one JSON entry per line, each
//! message linked by `parentUuid` to the one it follows. A resumed session
//! appends to its transcript, so a thread cut by `--resume-session-at`
//! leaves its abandoned branch in the file; only the parent chain says
//! which reply a prompt followed.

use std::{
    collections::HashMap,
    io::{BufRead, Read},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use uuid::Uuid;

/// Parent links followed from a prompt back to its reply. Claude Code puts
/// at most a few system or attachment entries between them.
const MAX_HOPS: usize = 16;

/// Entries a cut remembers before giving up on the transcript; each costs
/// a few dozen bytes, so the bound keeps a runaway file from growing Sage.
pub(super) const MAX_ENTRIES: usize = 200_000;
/// Tool results can be large; no single JSONL entry may exceed 8 MiB.
pub(super) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
/// One transcript pass reads at most 64 MiB, including invalid entries.
pub(super) const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TranscriptError {
    #[error("Cannot undo: the Claude transcript cannot be read.")]
    Unreadable,
    #[error("Cannot undo: a Claude transcript entry exceeds the 8 MiB limit.")]
    LineLimit,
    #[error("Cannot undo: the Claude transcript exceeds the 64 MiB limit.")]
    ByteLimit,
    #[error("Cannot undo: the Claude transcript exceeds the 200,000 entry limit.")]
    EntryLimit,
    #[error("Cannot undo: the Claude transcript parent chain cannot be resolved.")]
    ParentChain,
}

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
    parent: Parent,
    #[serde(rename = "isSidechain", default)]
    sidechain: Option<bool>,
}

/// An omitted link is unresolved; only explicit null proves a root.
#[derive(Clone, Copy, Default)]
enum Parent {
    #[default]
    Missing,
    Root,
    Id(Uuid),
    Invalid,
}

impl<'de> Deserialize<'de> for Parent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<&str>::deserialize(deserializer)? {
            None => Self::Root,
            Some(value) => parse_uuid(value).map_or(Self::Invalid, Self::Id),
        })
    }
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
    if !is_uuid(session) {
        return None;
    }
    let name = format!("{session}.jsonl");
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .take(MAX_ENTRIES)
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
    })
    .ok()?;
    last.map(|id| id.hyphenated().to_string())
}

/// Resolve the cut during Undo. `None` proves this is the thread's first
/// prompt; an unreadable or unresolved chain is never treated as a restart.
pub(super) fn cut_at(path: &Path, prompt: &str) -> Result<Option<String>, TranscriptError> {
    let prompt = parse_uuid(prompt).ok_or(TranscriptError::ParentChain)?;
    let mut entries: HashMap<Uuid, (Option<Uuid>, bool, bool)> = HashMap::new();
    let mut start = None;
    let mut invalid = false;
    for_each_entry(path, |entry| {
        if start.is_some() {
            return true;
        }
        let Some(id) = entry.id() else {
            return true;
        };
        let parent = match entry.parent {
            Parent::Root => None,
            Parent::Id(id) => Some(id),
            _ => {
                invalid = true;
                None
            }
        };
        // Resumed transcripts can repeat history; only conflicting links or
        // message roles make a repeated UUID ambiguous.
        let ancestry = (
            parent,
            entry.is_main_reply(),
            entry.kind == Some("user") && entry.sidechain != Some(true),
        );
        if id == prompt {
            if entry.kind == Some("user") && entry.sidechain != Some(true) {
                start = Some(parent);
            } else {
                invalid = true;
            }
        } else if entries
            .insert(id, ancestry)
            .is_some_and(|previous| previous != ancestry)
        {
            invalid = true;
        }
        true
    })?;
    if invalid {
        return Err(TranscriptError::ParentChain);
    }
    let mut next = start.ok_or(TranscriptError::ParentChain)?;
    for _ in 0..MAX_HOPS {
        let Some(id) = next else {
            return Ok(None);
        };
        let &(parent, is_reply, is_prompt) =
            entries.get(&id).ok_or(TranscriptError::ParentChain)?;
        if is_reply {
            return Ok(Some(id.hyphenated().to_string()));
        }
        if is_prompt {
            return Err(TranscriptError::ParentChain);
        }
        next = parent;
    }
    Err(TranscriptError::ParentChain)
}

/// The bounded reader admits every physical entry, including malformed,
/// duplicate and UUID-less entries. It never allocates an unbounded line.
fn for_each_entry(
    path: &Path,
    mut visit: impl FnMut(Entry<'_>) -> bool,
) -> Result<(), TranscriptError> {
    let file = std::fs::File::open(path).map_err(|_| TranscriptError::Unreadable)?;
    if file
        .metadata()
        .map_err(|_| TranscriptError::Unreadable)?
        .len()
        > MAX_TOTAL_BYTES
    {
        return Err(TranscriptError::ByteLimit);
    }
    let mut reader = std::io::BufReader::new(file.take(MAX_TOTAL_BYTES + 1));
    let mut line = Vec::new();
    let (mut total, mut count) = (0_u64, 0_usize);
    loop {
        line.clear();
        let read = (&mut reader)
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| TranscriptError::Unreadable)?;
        if read == 0 {
            return Ok(());
        }
        total += read as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(TranscriptError::ByteLimit);
        }
        if read > MAX_LINE_BYTES {
            return Err(TranscriptError::LineLimit);
        }
        count += 1;
        if count > MAX_ENTRIES {
            return Err(TranscriptError::EntryLimit);
        }
        if let Ok(entry) = serde_json::from_slice::<Entry<'_>>(&line)
            && !visit(entry)
        {
            return Ok(());
        }
    }
}

/// Revalidate a staged target on every launch; never silently resume past it.
pub(super) fn validate_cut(path: &Path, at: &str) -> Result<(), TranscriptError> {
    let at = parse_uuid(at).ok_or(TranscriptError::ParentChain)?;
    let mut found = false;
    for_each_entry(path, |entry| {
        found |= entry.id() == Some(at) && entry.is_main_reply();
        true
    })?;
    if found {
        Ok(())
    } else {
        Err(TranscriptError::ParentChain)
    }
}

pub(crate) fn resolve_undo(
    config: &Path,
    session: &str,
    turn: &str,
) -> Result<Option<String>, TranscriptError> {
    let prompt = super::bridge::message_uuid(turn).ok_or(TranscriptError::ParentChain)?;
    let session = super::fork::claude_session(session);
    let path = transcript(config, session).ok_or(TranscriptError::Unreadable)?;
    cut_at(&path, &prompt)
}
