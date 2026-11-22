//! A binding's curated terminal handoff: which known commands only its
//! terminal runs, which of them a native control replaces, and the command
//! that reopens the session there. Harness specifics live in metadata only.
use super::{KnownCommandTable, NativeControl, ReleaseVersion};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalOnlyCommand {
    name: String,
    aliases: Vec<String>,
    /// The curated control; it applies only while `controls` lists it.
    #[serde(skip_serializing_if = "Option::is_none")]
    control: Option<NativeControl>,
}

/// Loaded once per curated binding, together with its command table.
pub(crate) struct TerminalMetadata {
    curated: Curated,
    rows: Vec<TerminalOnlyCommand>,
    /// Version gates per curated control, in row order.
    gates: Vec<(NativeControl, Option<ReleaseVersion>)>,
}

/// The projected handoff. Static rows are shared; only the resume command and
/// the supported controls are per session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalHandoff {
    label: &'static str,
    command: String,
    terminal_only: &'static [TerminalOnlyCommand],
    controls: Vec<NativeControl>,
    #[serde(skip)]
    session: Option<Box<str>>,
    #[serde(skip)]
    mask: u16,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// The command is prefilled for the user, never run. Double quotes keep spaces
/// in cmd, PowerShell and POSIX shells; anything a shell would expand fails
/// closed to the bare harness command.
fn resumable(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= MAX_SESSION_BYTES
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
        let terminal = || table.0.iter().filter(|row| row.terminal_only == Some(true));
        Ok(Self {
            curated,
            rows: terminal()
                .map(|row| TerminalOnlyCommand {
                    name: row.name.clone(),
                    aliases: row.aliases.clone(),
                    control: row.control,
                })
                .collect(),
            gates: terminal()
                .filter_map(|row| {
                    let since = row.since.as_deref().and_then(ReleaseVersion::parse);
                    row.control.map(|control| (control, since))
                })
                .collect(),
        })
    }

    fn supported(
        &self,
        running: Option<&ReleaseVersion>,
        supports: &dyn Fn(NativeControl) -> bool,
    ) -> u16 {
        self.gates.iter().fold(0, |mask, (control, since)| {
            let version = since
                .as_ref()
                .is_none_or(|minimum| running.is_some_and(|running| running >= minimum));
            if version && supports(*control) {
                mask | 1 << *control as u16
            } else {
                mask
            }
        })
    }

    /// Called on every command-list read, so an unchanged projection allocates
    /// nothing. Returns whether the slot changed.
    pub(crate) fn update(
        &'static self,
        slot: &mut Option<TerminalHandoff>,
        running: Option<&ReleaseVersion>,
        supports: &dyn Fn(NativeControl) -> bool,
        session: Option<&str>,
    ) -> bool {
        let session = session.filter(|session| resumable(session));
        let mask = self.supported(running, supports);
        if slot
            .as_ref()
            .is_some_and(|handoff| handoff.mask == mask && handoff.session.as_deref() == session)
        {
            return false;
        }
        let Curated {
            label,
            command,
            resume_args,
        } = &self.curated;
        let mut controls = Vec::new();
        for (control, _) in &self.gates {
            if mask & 1 << *control as u16 != 0 && !controls.contains(control) {
                controls.push(*control);
            }
        }
        *slot = Some(TerminalHandoff {
            label,
            command: session.map_or_else(
                || command.clone(),
                |session| format!("{command} {} \"{session}\"", resume_args.join(" ")),
            ),
            terminal_only: &self.rows,
            controls,
            session: session.map(Box::from),
            mask,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_handoff_projects_terminal_commands_supported_controls_and_a_safe_resume() {
        static METADATA: OnceLock<TerminalMetadata> = OnceLock::new();
        let table = KnownCommandTable::parse(
            br#"[
          {"name":"hooks","section":"addOns","terminalOnly":true},
          {"name":"permissions","section":"modelAndAccess","terminalOnly":true,"control":"accessMenu"},
          {"name":"model","section":"modelAndAccess","terminalOnly":true,"control":"modelPicker","since":"3.0.0"},
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
        let running = ReleaseVersion::parse("2.1.322");
        let all = |_| true;
        assert!(metadata.update(&mut slot, running.as_ref(), &all, Some("native session")));
        assert_eq!(
            serde_json::to_value(&slot).unwrap(),
            serde_json::json!({
              "label":"Example","command":"agent --resume \"native session\"",
              "terminalOnly":[
                {"name":"hooks","aliases":[]},
                {"name":"permissions","aliases":[],"control":"accessMenu"},
                {"name":"model","aliases":[],"control":"modelPicker"}
              ],
              "controls":["accessMenu"]
            })
        );
        // Unchanged inputs keep the projection, so the list keeps its revision.
        assert!(!metadata.update(&mut slot, running.as_ref(), &all, Some("native session")));
        assert!(metadata.update(
            &mut slot,
            running.as_ref(),
            &|_| false,
            Some("native session")
        ));
        assert_eq!(slot.as_ref().unwrap().controls, []);
        for target in [
            None,
            Some("bad\ninput"),
            Some("$(execute)"),
            Some("%EXPANSION%"),
            Some("a\"b"),
            Some("x".repeat(MAX_SESSION_BYTES + 1).as_str()),
        ] {
            metadata.update(&mut slot, None, &all, target);
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
