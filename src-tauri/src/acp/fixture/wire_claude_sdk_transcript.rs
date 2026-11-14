//! Claude Code keeps each session as a JSON Lines transcript in its profile,
//! `<config>/projects/<project>/<session>.jsonl`, where `<project>` is the
//! working directory with every character but ASCII letters and digits made
//! `-`. Its launch flags pick the session: `--session-id S` starts S,
//! `--resume S` continues it, and `--resume S --resume-session-at A
//! --fork-session --session-id F` copies S through message A as the new
//! session F. A session's file is written with its first message, so a fork
//! that ends sooner leaves no copy. A case launched with these flags keeps
//! its transcript the same way, so Sage forks from a real file, and a reply
//! can quote what the session held before its prompt (`$TRANSCRIPT_ECHO`).

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

use super::super::FixtureError;

const MAX_TRANSCRIPT_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const ECHOED_MESSAGES: usize = 8;
const ECHOED_TEXT_BYTES: usize = 160;

pub(super) struct Transcript {
    project: PathBuf,
    cwd: String,
    session: String,
    opened: String,
    entries: Vec<Value>,
    bytes: u64,
    /// Whether the session's file holds `entries`: not until a new session
    /// or a fork has its first message.
    saved: bool,
}

impl Transcript {
    /// The session `args` open, as Claude Code reads them from `config` in
    /// `cwd`. A model named after them is Claude Code's to apply, not the
    /// transcript's. An id Claude Code would refuse fails the launch: a new
    /// session whose id is taken, or a resume or fork of a missing one.
    pub(super) fn launched(
        args: &[String],
        config: &Path,
        cwd: &Path,
    ) -> Result<Self, FixtureError> {
        let mut args: Vec<&str> = args.iter().map(String::as_str).collect();
        if let [.., "--model", model] = args[..]
            && !model.is_empty()
        {
            args.truncate(args.len() - 2);
        }
        let cwd_text = cwd.to_str().ok_or(FixtureError::InvalidArguments)?;
        let project = config.join("projects").join(project_name(cwd_text));
        let path = |id: &str| project.join(format!("{id}.jsonl"));
        let (session, opened, entries, bytes) = match args[..] {
            ["--session-id", id] if is_uuid(id) && !path(id).exists() => {
                (id, "new".to_owned(), Vec::new(), None)
            }
            ["--resume", id] if is_uuid(id) => {
                let (entries, bytes) = load(&path(id))?;
                let opened = format!("resumed with {} entries", entries.len());
                (id, opened, entries, Some(bytes))
            }
            [
                "--resume",
                source,
                "--resume-session-at",
                at,
                "--fork-session",
                "--session-id",
                id,
            ] if [source, at, id].into_iter().all(is_uuid) && !path(id).exists() => {
                let (mut entries, _) = load(&path(source))?;
                // Claude Code cuts a fork at a main-thread reply.
                let through = entries
                    .iter()
                    .position(|entry| {
                        entry["uuid"] == at
                            && entry["type"] == "assistant"
                            && entry["isSidechain"] != true
                    })
                    .ok_or(FixtureError::InvalidArguments)?;
                entries.truncate(through + 1);
                for entry in &mut entries {
                    entry["sessionId"] = json!(id);
                }
                let opened = format!(
                    "forked from {source} through message {at}, with {} entries",
                    entries.len()
                );
                (id, opened, entries, None)
            }
            _ => return Err(FixtureError::InvalidArguments),
        };
        Ok(Self {
            project,
            cwd: cwd_text.to_owned(),
            session: session.to_owned(),
            opened,
            entries,
            bytes: bytes.unwrap_or(0),
            saved: bytes.is_some(),
        })
    }

    pub(super) fn session(&self) -> &str {
        &self.session
    }

    pub(super) fn echo(&self) -> String {
        let messages: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry["isSidechain"] != true)
            .filter_map(message_line)
            .collect();
        let mut lines = vec![format!(
            "Claude Code session {}, {}.",
            self.session, self.opened
        )];
        let skipped = messages.len().saturating_sub(ECHOED_MESSAGES);
        if skipped > 0 {
            lines.push(format!("{skipped} earlier messages."));
        }
        lines.extend(messages.into_iter().skip(skipped));
        lines.join("\n")
    }

    /// Records the client's message, under the `uuid` it gave when it gave
    /// a valid one, as Claude Code does.
    pub(super) fn sent(&mut self, frame: &Value) -> Result<(), FixtureError> {
        let uuid = frame["uuid"]
            .as_str()
            .filter(|uuid| is_uuid(uuid))
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
        self.append("user", &frame["message"], uuid)
    }

    /// Gives a replayed frame this session's id and records a main-thread
    /// message under the `uuid` the frame carries, as Claude Code streams
    /// each message it saves. A message without a valid one gets one.
    pub(super) fn replayed(&mut self, frame: &mut Value) -> Result<(), FixtureError> {
        if frame.get("session_id").is_some() {
            frame["session_id"] = json!(self.session);
        }
        let kind = match frame["type"].as_str() {
            Some(kind @ ("user" | "assistant")) if frame["parent_tool_use_id"].is_null() => {
                kind.to_owned()
            }
            _ => return Ok(()),
        };
        let uuid = match frame["uuid"].as_str().filter(|uuid| is_uuid(uuid)) {
            Some(uuid) => uuid.to_owned(),
            None => {
                let uuid = uuid::Uuid::new_v4().to_string();
                frame["uuid"] = json!(uuid);
                uuid
            }
        };
        self.append(&kind, &frame["message"], uuid)
    }

    fn append(&mut self, kind: &str, message: &Value, uuid: String) -> Result<(), FixtureError> {
        if self.entries.len() >= MAX_ENTRIES || !message.is_object() {
            return Err(FixtureError::ExpandedLimit);
        }
        let parent = self
            .entries
            .iter()
            .rev()
            .find_map(|entry| entry["uuid"].as_str())
            .map(str::to_owned);
        let entry = json!({
            "parentUuid": parent, "isSidechain": false, "userType": "external",
            "cwd": self.cwd, "sessionId": self.session, "type": kind,
            "message": message, "uuid": uuid,
        });
        let unsaved = if self.saved { self.entries.len() } else { 0 };
        let mut lines = String::new();
        for written in self.entries[unsaved..].iter().chain([&entry]) {
            lines.push_str(&format!("{written}\n"));
        }
        // A session is never saved larger than a launch reads back.
        let bytes = self.bytes + lines.len() as u64;
        if bytes > MAX_TRANSCRIPT_BYTES {
            return Err(FixtureError::ExpandedLimit);
        }
        fs::create_dir_all(&self.project).map_err(|_| FixtureError::Io)?;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.project.join(format!("{}.jsonl", self.session)))
            .and_then(|mut file| file.write_all(lines.as_bytes()))
            .map_err(|_| FixtureError::Io)?;
        self.saved = true;
        self.bytes = bytes;
        self.entries.push(entry);
        Ok(())
    }
}

fn project_name(cwd: &str) -> String {
    cwd.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

fn load(path: &Path) -> Result<(Vec<Value>, u64), FixtureError> {
    let file = fs::File::open(path).map_err(|_| FixtureError::InvalidArguments)?;
    let mut text = String::new();
    file.take(MAX_TRANSCRIPT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| FixtureError::Io)?;
    if text.len() as u64 > MAX_TRANSCRIPT_BYTES {
        return Err(FixtureError::ExpandedLimit);
    }
    let entries = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|_| FixtureError::InvalidArguments))
        .collect::<Result<Vec<Value>, _>>()?;
    if entries.len() > MAX_ENTRIES {
        return Err(FixtureError::ExpandedLimit);
    }
    Ok((entries, text.len() as u64))
}

fn message_line(entry: &Value) -> Option<String> {
    let who = match entry["type"].as_str()? {
        "user" => "User",
        "assistant" => "Claude",
        _ => return None,
    };
    let content = &entry["message"]["content"];
    let text = match content.as_str() {
        Some(text) => text.to_owned(),
        None => content
            .as_array()?
            .iter()
            .map(|block| match block["type"].as_str() {
                Some("text") => block["text"].as_str().unwrap_or_default().to_owned(),
                Some(kind) => format!("[{kind}]"),
                None => "[block]".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    let mut cut = text.len().min(ECHOED_TEXT_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let more = if cut < text.len() { "..." } else { "" };
    Some(format!("{who}: {}{more}", text[..cut].replace('\n', " ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "1c8e7d9f-4d2b-4a8f-8b62-3a7e9d2f5ca1";
    const FORK: &str = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    struct Profile(PathBuf);

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile() -> (Profile, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("sage-transcript-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("project")).unwrap();
        let (config, cwd) = (root.join("config"), root.join("project"));
        (Profile(root), config, cwd)
    }

    fn assistant(text: &str) -> Value {
        json!({"type": "assistant", "parent_tool_use_id": null, "session_id": "fixture-session",
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})
    }

    fn user(text: &str) -> Value {
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
    }

    #[test]
    fn a_session_is_saved_with_its_first_message_and_forks_through_a_reply() {
        let (_profile, config, cwd) = profile();
        let mut source =
            Transcript::launched(&args(&["--session-id", SOURCE]), &config, &cwd).unwrap();
        let file = config
            .join("projects")
            .join(project_name(cwd.to_str().unwrap()))
            .join(format!("{SOURCE}.jsonl"));
        assert!(!file.exists(), "nothing is saved before the first message");
        source.sent(&user("First question")).unwrap();
        let mut reply = assistant("First answer");
        source.replayed(&mut reply).unwrap();
        assert_eq!(reply["session_id"], SOURCE);
        let at = reply["uuid"].as_str().unwrap().to_owned();
        source.sent(&user("Second question")).unwrap();
        source.replayed(&mut assistant("Second answer")).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap().lines().count(), 4);
        assert!(Transcript::launched(&args(&["--session-id", SOURCE]), &config, &cwd).is_err());

        let fork_args = args(&[
            "--resume",
            SOURCE,
            "--resume-session-at",
            &at,
            "--fork-session",
            "--session-id",
            FORK,
            "--model",
            "opus",
        ]);
        let mut fork = Transcript::launched(&fork_args, &config, &cwd).unwrap();
        let echo = fork.echo();
        assert!(echo.contains(&format!(
            "forked from {SOURCE} through message {at}, with 2 entries"
        )));
        assert!(
            echo.ends_with("User: First question\nClaude: First answer"),
            "{echo}"
        );
        let copy = file.with_file_name(format!("{FORK}.jsonl"));
        assert!(!copy.exists(), "a fork is copied with its first message");
        fork.sent(&user("Fork question")).unwrap();
        let saved: Vec<Value> = fs::read_to_string(&copy)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(saved.len(), 3);
        assert!(saved.iter().all(|entry| entry["sessionId"] == FORK));
        assert_eq!(saved[2]["parentUuid"], at.as_str());

        let resumed = Transcript::launched(&args(&["--resume", FORK]), &config, &cwd).unwrap();
        assert!(resumed.echo().contains("resumed with 3 entries"));
    }

    #[test]
    fn launches_claude_code_would_refuse_fail() {
        let (_profile, config, cwd) = profile();
        let mut source =
            Transcript::launched(&args(&["--session-id", SOURCE]), &config, &cwd).unwrap();
        source.sent(&user("Question")).unwrap();
        source.replayed(&mut assistant("Answer")).unwrap();
        let asked = source.entries[0]["uuid"].as_str().unwrap().to_owned();
        let fork_at = |at: &str| {
            args(&[
                "--resume",
                SOURCE,
                "--resume-session-at",
                at,
                "--fork-session",
                "--session-id",
                FORK,
            ])
        };
        for refused in [
            fork_at(FORK),
            fork_at(&asked),
            args(&["--session-id", SOURCE]),
            args(&["--resume", FORK]),
            args(&["--session-id", "not-a-uuid"]),
            args(&["--session-id", FORK, "--verbose"]),
        ] {
            assert!(
                Transcript::launched(&refused, &config, &cwd).is_err(),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_session_is_never_saved_larger_than_a_launch_reads_back() {
        let (_profile, config, cwd) = profile();
        let mut session =
            Transcript::launched(&args(&["--session-id", SOURCE]), &config, &cwd).unwrap();
        let large = "x".repeat(400 * 1024);
        session.sent(&user(&large)).unwrap();
        session.replayed(&mut assistant(&large)).unwrap();
        assert_eq!(
            session.sent(&user(&large)).err(),
            Some(FixtureError::ExpandedLimit)
        );
        let resumed = Transcript::launched(&args(&["--resume", SOURCE]), &config, &cwd).unwrap();
        assert!(resumed.echo().contains("resumed with 2 entries"));
    }

    #[test]
    fn an_echo_shows_the_latest_messages_cut_short() {
        let (_profile, config, cwd) = profile();
        let mut session =
            Transcript::launched(&args(&["--session-id", SOURCE]), &config, &cwd).unwrap();
        assert_eq!(
            session.echo(),
            format!("Claude Code session {SOURCE}, new.")
        );
        for turn in 0..5 {
            session.sent(&user(&format!("Question {turn}"))).unwrap();
            session.replayed(&mut assistant(&"é".repeat(100))).unwrap();
        }
        let echo = session.echo();
        assert!(echo.contains("2 earlier messages."));
        assert!(!echo.contains("Question 0"));
        assert!(echo.contains(&format!("Claude: {}...", "é".repeat(80))));
    }
}
