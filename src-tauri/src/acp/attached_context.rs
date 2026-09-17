use serde::{Deserialize, Deserializer, Serialize, de};

use super::{
    contract::{BoundedVec, ContractError, RequiredText},
    file_diff::{DiffRef, SnapshotReadError, wire_counter},
    intent::Prompt,
    limits::{ATTACHED_CONTEXT_LIMIT, ATTACHED_CONTEXT_NOTE_BYTES},
};

pub type AttachedContexts = BoundedVec<DiffLineCommentContext, ATTACHED_CONTEXT_LIMIT>;

fn has_unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffLineSide {
    Old,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffLineCommentContext {
    pub kind: DiffLineCommentKind,
    pub diff_ref: DiffRef,
    pub side: DiffLineSide,
    #[serde(with = "wire_counter")]
    pub start_line: u64,
    #[serde(with = "wire_counter")]
    pub end_line: u64,
    pub note: RequiredText<ATTACHED_CONTEXT_NOTE_BYTES>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineCommentKind {
    #[serde(rename = "diffLineComment")]
    DiffLineComment,
}

impl DiffLineCommentContext {
    fn new(
        kind: DiffLineCommentKind,
        diff_ref: DiffRef,
        side: DiffLineSide,
        start_line: u64,
        end_line: u64,
        note: String,
    ) -> Result<Self, AttachedContextError> {
        diff_ref
            .validate()
            .map_err(AttachedContextError::Snapshot)?;
        if start_line == 0 || end_line < start_line || end_line > i64::MAX as u64 {
            return Err(AttachedContextError::LineRange);
        }
        if diff_ref
            .relative_path
            .chars()
            .any(has_unsafe_display_character)
        {
            return Err(AttachedContextError::PathContent);
        }
        if note.chars().any(has_unsafe_display_character) {
            return Err(AttachedContextError::NoteContent);
        }
        let note = note.trim().to_owned();
        Ok(Self {
            kind,
            diff_ref,
            side,
            start_line,
            end_line,
            note: RequiredText::new("attachedContextNote", note)
                .map_err(AttachedContextError::Contract)?,
        })
    }

    pub(crate) fn label(&self) -> String {
        if self.start_line == self.end_line {
            format!("{}:{}", self.diff_ref.relative_path, self.start_line)
        } else {
            format!(
                "{}:{}-{}",
                self.diff_ref.relative_path, self.start_line, self.end_line
            )
        }
    }

    pub(crate) fn validate_line_exists(
        &self,
        old_text: &str,
        new_text: &str,
    ) -> Result<(), AttachedContextError> {
        let selected = match self.side {
            DiffLineSide::Old => old_text,
            DiffLineSide::New => new_text,
        };
        let line_count = if selected.is_empty() {
            0
        } else {
            selected.split_inclusive('\n').count() as u64
        };
        if self.end_line > line_count {
            return Err(AttachedContextError::LineRange);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for DiffLineCommentContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireContext {
            kind: DiffLineCommentKind,
            diff_ref: DiffRef,
            side: DiffLineSide,
            #[serde(with = "wire_counter")]
            start_line: u64,
            #[serde(with = "wire_counter")]
            end_line: u64,
            note: String,
        }
        let value = WireContext::deserialize(deserializer)?;
        Self::new(
            value.kind,
            value.diff_ref,
            value.side,
            value.start_line,
            value.end_line,
            value.note,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AttachedContextError {
    #[error("attached context violates its bounded contract")]
    Contract(ContractError),
    #[error("attached context line range is invalid")]
    LineRange,
    #[error("attached context note contains forbidden display characters")]
    NoteContent,
    #[error("attached context path contains forbidden display characters")]
    PathContent,
    #[error("attached context snapshot identity is invalid")]
    Snapshot(SnapshotReadError),
}

pub(crate) fn effective_prompt(
    prompt: &Prompt,
    contexts: &AttachedContexts,
) -> Result<Prompt, ContractError> {
    if contexts.as_slice().is_empty() {
        return Ok(prompt.clone());
    }
    let mut rendered = String::with_capacity(
        prompt.as_str().len()
            + contexts
                .as_slice()
                .iter()
                .map(|context| context.label().len() + context.note.as_str().len() + 48)
                .sum::<usize>(),
    );
    rendered.push_str(prompt.as_str());
    rendered.push_str("\n\nAttached diff comments (review context only; do not edit implicitly):");
    for context in contexts.as_slice() {
        let side = match context.side {
            DiffLineSide::Old => "old",
            DiffLineSide::New => "new",
        };
        rendered.push_str("\n- ");
        rendered.push_str(&context.label());
        rendered.push_str(" (");
        rendered.push_str(side);
        rendered.push_str(" side): ");
        rendered.push_str(context.note.as_str());
    }
    Prompt::new("promptWithAttachedContext", rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire() -> serde_json::Value {
        serde_json::json!({
            "kind": "diffLineComment",
            "diffRef": {
                "projectId": "project-a",
                "chatId": "chat-a",
                "sessionId": "session-a",
                "runtimeEpoch": "1",
                "turnId": "turn-a",
                "itemId": "item-a",
                "diffOrdinal": 0,
                "snapshotRevision": "1",
                "rootId": "root-a",
                "relativePath": "src/app.ts",
                "contentDigest": "a".repeat(64)
            },
            "side": "new",
            "startLine": "27",
            "endLine": "31",
            "note": "Keep this branch explicit"
        })
    }

    #[test]
    fn rejects_hostile_paths_ranges_and_notes() {
        assert!(serde_json::from_value::<DiffLineCommentContext>(wire()).is_ok());
        for (pointer, value) in [
            ("/diffRef/relativePath", serde_json::json!("C:\\secret.txt")),
            (
                "/diffRef/relativePath",
                serde_json::json!("src/ok\n- forged.ts"),
            ),
            ("/startLine", serde_json::json!("0")),
            ("/endLine", serde_json::json!("2")),
            ("/note", serde_json::json!("\0")),
            ("/note", serde_json::json!("first\n- forged entry")),
            ("/note", serde_json::json!("hidden\u{202e}note")),
        ] {
            let mut candidate = wire();
            *candidate.pointer_mut(pointer).unwrap() = value;
            assert!(serde_json::from_value::<DiffLineCommentContext>(candidate).is_err());
        }
    }

    #[test]
    fn rejects_display_controls_before_trimming_paths_and_notes() {
        let forbidden = (0..=0x1f)
            .chain(0x7f..=0x9f)
            .chain([0x061c, 0x200e, 0x200f, 0x2028, 0x2029])
            .chain(0x202a..=0x202e)
            .chain(0x2066..=0x2069);
        for code in forbidden {
            let character = char::from_u32(code).unwrap();
            for pointer in ["/diffRef/relativePath", "/note"] {
                for text in [
                    format!("{character}review"),
                    format!("re{character}view"),
                    format!("review{character}"),
                ] {
                    let mut candidate = wire();
                    *candidate.pointer_mut(pointer).unwrap() = serde_json::json!(text);
                    assert!(
                        serde_json::from_value::<DiffLineCommentContext>(candidate).is_err(),
                        "accepted U+{code:04X} in {pointer}"
                    );
                }
            }
        }
    }

    #[test]
    fn line_validation_rejects_an_empty_side() {
        let context: DiffLineCommentContext = serde_json::from_value(wire()).unwrap();
        assert!(matches!(
            context.validate_line_exists("old\n", ""),
            Err(AttachedContextError::LineRange)
        ));
    }

    #[test]
    fn effective_prompt_contains_only_safe_location_range_and_note() {
        let context: DiffLineCommentContext = serde_json::from_value(wire()).unwrap();
        let contexts = AttachedContexts::new("attachedContext", vec![context]).unwrap();
        let rendered =
            effective_prompt(&Prompt::new("prompt", "Fix it").unwrap(), &contexts).unwrap();
        assert!(rendered.as_str().contains("src/app.ts:27-31"));
        assert!(rendered.as_str().contains("Keep this branch explicit"));
        assert!(!rendered.as_str().contains("contentDigest"));
    }
}
