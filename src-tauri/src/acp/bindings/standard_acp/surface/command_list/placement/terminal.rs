//! A binding's curated terminal handoff: the commands only its own terminal
//! runs, and the command that reopens the session there. Harness specifics
//! live in metadata only; native owns the classification.
use super::KnownCommandTable;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const MAX_METADATA_BYTES: usize = 2048;
const MAX_TOKEN_BYTES: usize = 64;
const MAX_RESUME_ARGS: usize = 4;
/// A Pi session file path, the longest target a curated harness resumes.
const MAX_SESSION_BYTES: usize = 3072;

/// Curated installation data, never a command supplied by a harness process.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Curated {
    label: String,
    command: String,
    resume_args: Vec<String>,
}

/// Loaded once per curated binding, together with its command table.
pub(crate) struct TerminalMetadata {
    curated: Curated,
    /// Every terminal-only name and alias, flattened once.
    names: Vec<String>,
}

/// The projected handoff. The names are shared; only the resume command is
/// per session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalHandoff {
    label: &'static str,
    command: String,
    terminal_only: &'static [String],
    #[serde(skip)]
    session: Option<Box<str>>,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// The command is copied for the user, never run. Double quotes keep spaces in
/// cmd, PowerShell and POSIX shells; anything a shell would expand, a leading
/// `-` the harness would parse as an option, or a trailing `\` that would
/// escape the closing quote fails closed to the bare harness command.
fn resumable(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= MAX_SESSION_BYTES
        && !session.starts_with('-')
        && !session.ends_with('\\')
        && session
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.:/\\ ".contains(c))
}

impl TerminalMetadata {
    pub(crate) fn load(
        cache: &'static OnceLock<Self>,
        bytes: &[u8],
        table: &KnownCommandTable,
    ) -> &'static Self {
        cache.get_or_init(|| Self::parse(bytes, table).expect("invalid curated terminal metadata"))
    }

    fn parse(bytes: &[u8], table: &KnownCommandTable) -> Result<Self, &'static str> {
        if bytes.len() > MAX_METADATA_BYTES {
            return Err("terminal metadata exceeds byte cap");
        }
        let curated: Curated =
            serde_json::from_slice(bytes).map_err(|_| "malformed terminal metadata")?;
        if curated.label.is_empty()
            || curated.label.len() > 128
            || curated.label.chars().any(char::is_control)
            || !token(&curated.command)
            || curated.resume_args.len() > MAX_RESUME_ARGS
            || !curated.resume_args.iter().all(|arg| token(arg))
        {
            return Err("invalid terminal metadata bounds");
        }
        // The table's own caps bound this list (128 rows, 8 aliases each). Rows
        // with a Sage control wait for the native-control step, so they never
        // get a terminal notice.
        let names = table
            .0
            .iter()
            .filter(|row| row.terminal_only == Some(true) && row.control.is_none())
            .flat_map(|row| std::iter::once(&row.name).chain(&row.aliases))
            .cloned()
            .collect();
        Ok(Self { curated, names })
    }

    /// Called on every command-list read, so an unchanged projection allocates
    /// nothing. Returns whether the slot changed.
    pub(crate) fn update(
        &'static self,
        slot: &mut Option<TerminalHandoff>,
        session: Option<&str>,
    ) -> bool {
        let session = session.filter(|session| resumable(session));
        if slot
            .as_ref()
            .is_some_and(|handoff| handoff.session.as_deref() == session)
        {
            return false;
        }
        let Curated {
            label,
            command,
            resume_args,
        } = &self.curated;
        *slot = Some(TerminalHandoff {
            label,
            command: session.map_or_else(
                || command.clone(),
                |session| format!("{command} {} \"{session}\"", resume_args.join(" ")),
            ),
            terminal_only: &self.names,
            session: session.map(Box::from),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_handoff_projects_flattened_terminal_names_and_a_safe_resume() {
        static METADATA: OnceLock<TerminalMetadata> = OnceLock::new();
        let table = KnownCommandTable::parse(
            br#"[
          {"name":"hooks","section":"addOns","terminalOnly":true},
          {"name":"permissions","aliases":["allowed-tools"],"section":"modelAndAccess","terminalOnly":true,"control":"accessMenu"},
          {"name":"reported","section":"session","control":"compact"}
        ]"#,
        )
        .unwrap();
        let metadata = TerminalMetadata::load(
            &METADATA,
            br#"{"label":"Example","command":"agent","resumeArgs":["--resume"]}"#,
            &table,
        );
        let mut slot = None;
        assert!(metadata.update(&mut slot, Some("native session")));
        assert_eq!(
            serde_json::to_value(&slot).unwrap(),
            serde_json::json!({
              "label":"Example","command":"agent --resume \"native session\"",
              "terminalOnly":["hooks"]
            })
        );
        // Unchanged inputs keep the projection without allocating.
        assert!(!metadata.update(&mut slot, Some("native session")));
        for target in [
            None,
            Some("bad\ninput"),
            Some("$(execute)"),
            Some("%EXPANSION%"),
            Some("a\"b"),
            Some("--dangerously-skip-permissions"),
            Some("-x"),
            Some("C:\\Users\\me\\"),
            Some("x".repeat(MAX_SESSION_BYTES + 1).as_str()),
        ] {
            metadata.update(&mut slot, target);
            assert_eq!(slot.as_ref().unwrap().command, "agent");
        }
        for invalid in [
            &[b' '; MAX_METADATA_BYTES + 1][..],
            br#"{"label":"X","command":"a b","resumeArgs":[]}"#,
            br#"{"label":"X","command":"a","resumeArgs":["$(x)"]}"#,
        ] {
            assert!(TerminalMetadata::parse(invalid, &table).is_err());
        }
    }
}
