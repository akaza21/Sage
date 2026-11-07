use serde::{Deserialize, Deserializer, Serialize, de};

use super::{
    attached_context::{AttachedContexts, has_unsafe_display_character},
    contract::{ContractError, RequiredText},
};

/// The text of a stored text item when the projection renders it as
/// assistant prose, decoded exactly as the projection decodes it: a native
/// `NormalizedPayload::Text` (no role, assistant by definition) or a legacy
/// row whose role is `assistant`.
pub(crate) fn assistant_text(payload_json: &str) -> Option<String> {
    use super::{intent::NormalizedPayload, projection::TextRole};
    let (payload, role) = match serde_json::from_str::<NormalizedPayload>(payload_json) {
        Ok(value) => (value, None),
        Err(_) => super::store::parse_legacy_text(payload_json).ok()?,
    };
    match (payload, role) {
        (NormalizedPayload::Text { text }, None | Some(TextRole::Assistant)) => {
            Some(text.as_str().to_owned())
        }
        _ => None,
    }
}

/// The quoted answer is missing, is not durable assistant prose in this
/// Project, or is shorter than the quoted range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuoteSourceMismatch;

impl From<QuoteSourceMismatch> for super::ipc::AcpIpcError {
    fn from(_: QuoteSourceMismatch) -> Self {
        Self::InvalidRequest {
            message: super::contract::SafeText::new(
                "message",
                "An annotation no longer matches its source answer.",
            )
            .expect("static ACP message is bounded"),
        }
    }
}

/// Every transcript quote in `contexts` must match its durable answer.
pub(crate) fn validate_quote_sources(
    connection: &rusqlite::Connection,
    project_id: &super::contract::SageProjectId,
    contexts: &AttachedContexts,
) -> Result<(), QuoteSourceMismatch> {
    contexts
        .as_slice()
        .iter()
        .try_for_each(|context| match context {
            super::attached_context::AttachedContext::TranscriptQuote(quote) => {
                validate_quote_source(connection, project_id.as_str(), quote)
            }
            _ => Ok(()),
        })
}

/// Checks a quote against the durable answer it names. Offsets count UTF-16
/// units of the rendered answer, which never exceeds its Markdown source, so
/// the source length bounds the range. Side chats quote their parent Chat, so
/// the item only has to belong to the same Project.
pub(crate) fn validate_quote_source(
    connection: &rusqlite::Connection,
    project_id: &str,
    quote: &TranscriptQuoteContext,
) -> Result<(), QuoteSourceMismatch> {
    use rusqlite::OptionalExtension;
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload_json FROM items
             WHERE project_id=?1 AND turn_id=?2 AND id=?3 AND kind='text'",
            rusqlite::params![project_id, quote.source_turn_id, quote.source_item_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| QuoteSourceMismatch)?;
    // Decode as the projection does: native text has no role field.
    let text = payload
        .as_deref()
        .and_then(assistant_text)
        .ok_or(QuoteSourceMismatch)?;
    let length = text.encode_utf16().count() as u64;
    if quote.end_offset > length {
        return Err(QuoteSourceMismatch);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteKind {
    #[serde(rename = "transcriptQuote")]
    TranscriptQuote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptQuoteContext {
    pub kind: QuoteKind,
    pub ordinal: u8,
    pub source_turn_id: String,
    pub source_item_id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub start_offset: u64,
    pub end_offset: u64,
}

impl<'de> Deserialize<'de> for TranscriptQuoteContext {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            kind: QuoteKind,
            ordinal: u8,
            source_turn_id: String,
            source_item_id: String,
            text: String,
            comment: Option<String>,
            start_offset: u64,
            end_offset: u64,
        }
        let value = Wire::deserialize(deserializer)?;
        // Quoted prose keeps its line structure; every other control or
        // bidirectional override would reshape the rendered prompt.
        let unsafe_display = |text: &str| {
            text.chars()
                .any(|ch| ch != '\n' && ch != '\t' && has_unsafe_display_character(ch))
        };
        if !(1..=8).contains(&value.ordinal)
            || unsafe_display(&value.text)
            || value.comment.as_deref().is_some_and(unsafe_display)
            || [&value.source_turn_id, &value.source_item_id]
                .iter()
                .any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
            || value.text.trim().is_empty()
            || value.text.len() > 4096
            || value.text.contains('\0')
            || value
                .comment
                .as_ref()
                .is_some_and(|text| text.len() > 4096 || text.contains('\0'))
            || value.end_offset <= value.start_offset
            || value.end_offset > 9_007_199_254_740_991
        {
            return Err(de::Error::custom("invalid bounded transcript quote"));
        }
        Ok(Self {
            kind: value.kind,
            ordinal: value.ordinal,
            source_turn_id: value.source_turn_id,
            source_item_id: value.source_item_id,
            text: value.text,
            comment: value.comment,
            start_offset: value.start_offset,
            end_offset: value.end_offset,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Prompt {
    text: RequiredText<100000>,
    #[serde(skip)]
    pub(crate) command_revision: Option<u64>,
    #[serde(skip)]
    context: Option<Box<(String, AttachedContexts)>>,
}

impl Prompt {
    pub fn new(field: &'static str, text: impl Into<String>) -> Result<Self, ContractError> {
        Ok(Self {
            text: RequiredText::new(field, text)?,
            context: None,
            command_revision: None,
        })
    }
    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.as_str().len()
            + self.context.as_ref().map_or(0, |context| {
                std::mem::size_of::<(String, AttachedContexts)>()
                    + context.0.len()
                    + std::mem::size_of_val(context.1.as_slice())
                    + serde_json::to_vec(&context.1).map_or(0, |value| value.len())
            })
    }
    pub(crate) fn with_context(mut self, original: &Prompt, context: &AttachedContexts) -> Self {
        self.context = Some(Box::new((original.as_str().to_owned(), context.clone())));
        self
    }
    pub(crate) fn user_payload(&self) -> serde_json::Value {
        match &self.context {
            Some(context) => {
                serde_json::json!({ "role": "user", "text": context.0, "attachedContext": context.1 })
            }
            None => serde_json::json!({ "role": "user", "text": self.as_str() }),
        }
    }
}

pub(crate) fn load_context(
    connection: &rusqlite::Connection,
    turn_id: &str,
) -> Result<Vec<super::attached_context::AttachedContext>, super::store::StoreError> {
    use super::store::StoreError;
    use rusqlite::OptionalExtension;
    let payload: Option<String> = connection.query_row(
        "SELECT CASE WHEN payload_bytes<=262144 THEN payload_json ELSE NULL END FROM items WHERE turn_id=?1 AND ordinal=0 AND kind='text' LIMIT 1",
        [turn_id], |row| row.get(0),
    ).optional()?;
    let Some(payload) = payload else {
        return Ok(Vec::new());
    };
    let value: serde_json::Value =
        serde_json::from_str(&payload).map_err(|_| StoreError::Corrupt("prompt context"))?;
    match value
        .get("payload")
        .and_then(|payload| payload.get("attachedContext"))
    {
        Some(context) => {
            let contexts: AttachedContexts = serde_json::from_value(context.clone())
                .map_err(|_| StoreError::Corrupt("prompt context bound"))?;
            Ok(contexts.as_slice().to_vec())
        }
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::attached_context::{AttachedContext, effective_prompt};

    #[test]
    fn quotes_are_escaped_numbered_and_retained_separately_from_the_prompt() {
        let first: AttachedContext = serde_json::from_value(serde_json::json!({"kind":"transcriptQuote","ordinal":1,"sourceTurnId":"t","sourceItemId":"i","text":"a \"quote\"\nnext","comment":"Why?","startOffset":0,"endOffset":14})).unwrap();
        let second: AttachedContext = serde_json::from_value(serde_json::json!({"kind":"transcriptQuote","ordinal":2,"sourceTurnId":"t","sourceItemId":"j","text":"other","startOffset":0,"endOffset":5})).unwrap();
        let contexts = AttachedContexts::new("attachedContext", vec![first, second]).unwrap();
        let prompt =
            effective_prompt(&Prompt::new("prompt", "Explain").unwrap(), &contexts).unwrap();
        assert_eq!(
            prompt.as_str(),
            "Explain\n\nAnnotations on earlier assistant output (quoted context; cite as \"Annotation N\"):\n- Annotation 1\n  Selected text (JSON-quoted): \"a \\\"quote\\\"\\nnext\"\n  User comment (JSON-quoted): \"Why?\"\n- Annotation 2\n  Selected text (JSON-quoted): \"other\"\n  User comment (JSON-quoted): (none)"
        );
        assert_eq!(prompt.user_payload()["text"], "Explain");
        assert_eq!(
            prompt.user_payload()["attachedContext"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(prompt.retained_bytes() > prompt.as_str().len() + "Explain".len());
        assert!(
            prompt.retained_bytes()
                >= prompt.as_str().len()
                    + "Explain".len()
                    + std::mem::size_of_val(contexts.as_slice())
                    + serde_json::to_vec(&contexts).unwrap().len()
        );
    }

    #[test]
    fn rejects_oversized_multibyte_quotes_and_bad_ranges() {
        let mut value = serde_json::json!({"kind":"transcriptQuote","ordinal":1,"sourceTurnId":"t","sourceItemId":"i","text":"x","startOffset":0,"endOffset":1});
        value["text"] = serde_json::json!("\u{6c34}".repeat(1366));
        assert!(serde_json::from_value::<TranscriptQuoteContext>(value.clone()).is_err());
        value["text"] = serde_json::json!("x");
        value["endOffset"] = serde_json::json!(0);
        assert!(serde_json::from_value::<TranscriptQuoteContext>(value).is_err());
    }

    #[test]
    fn original_context_survives_projection_storage_without_polluting_user_text() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE items(turn_id TEXT,ordinal INTEGER,kind TEXT,payload_json TEXT,payload_bytes INTEGER)").unwrap();
        let quote: AttachedContext = serde_json::from_value(serde_json::json!({"kind":"transcriptQuote","ordinal":1,"sourceTurnId":"earlier","sourceItemId":"answer","text":"source","comment":"why?","startOffset":0,"endOffset":6})).unwrap();
        let contexts = AttachedContexts::new("attachedContext", vec![quote.clone()]).unwrap();
        let effective =
            effective_prompt(&Prompt::new("prompt", "Explain").unwrap(), &contexts).unwrap();
        let payload =
            serde_json::json!({"kind":"text","payload":effective.user_payload()}).to_string();
        connection
            .execute(
                "INSERT INTO items VALUES ('turn',0,'text',?1,?2)",
                rusqlite::params![payload, payload.len() as i64],
            )
            .unwrap();
        assert_eq!(load_context(&connection, "turn").unwrap(), vec![quote]);
        assert_eq!(effective.user_payload()["text"], "Explain");
    }

    #[test]
    fn annotations_stay_under_one_header_when_other_context_interleaves() {
        let quote = |ordinal: u8, item: &str| -> AttachedContext {
            serde_json::from_value(serde_json::json!({"kind":"transcriptQuote","ordinal":ordinal,"sourceTurnId":"t","sourceItemId":item,"text":item,"startOffset":0,"endOffset":1})).unwrap()
        };
        let page: AttachedContext = serde_json::from_value(serde_json::json!({
            "kind": "browserPage", "tabId": "browser-1",
            "url": "https://example.com/", "title": "Example", "text": "page"
        }))
        .unwrap();
        let contexts =
            AttachedContexts::new("attachedContext", vec![quote(1, "a"), page, quote(2, "b")])
                .unwrap();
        let prompt = effective_prompt(&Prompt::new("prompt", "Explain").unwrap(), &contexts)
            .unwrap()
            .as_str()
            .to_owned();
        assert_eq!(
            prompt
                .matches("Annotations on earlier assistant output")
                .count(),
            1
        );
        let first = prompt.find("- Annotation 1").unwrap();
        let second = prompt.find("- Annotation 2").unwrap();
        let browser = prompt.find("Untrusted browser page context").unwrap();
        assert!(first < second && (browser < first || browser > second));
    }

    #[test]
    fn quote_text_and_comment_refuse_display_controls_but_keep_newlines_and_tabs() {
        let wire = |text: &str, comment: &str| serde_json::json!({"kind":"transcriptQuote","ordinal":1,"sourceTurnId":"t","sourceItemId":"i","text":text,"comment":comment,"startOffset":0,"endOffset":4});
        assert!(
            serde_json::from_value::<TranscriptQuoteContext>(wire("a\n\tb", "line\nnext\t."))
                .is_ok()
        );
        for (text, comment) in [
            ("a\u{202e}b", "ok"),
            ("ok", "a\u{2066}b"),
            ("a\u{0007}b", "ok"),
            ("ok", "a\u{001b}[31m"),
            ("ok", "a\rb"),
        ] {
            assert!(
                serde_json::from_value::<TranscriptQuoteContext>(wire(text, comment)).is_err(),
                "{text:?} / {comment:?}"
            );
        }
    }

    #[test]
    fn quote_sources_must_be_a_durable_assistant_answer_long_enough_for_the_range() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE items(id TEXT,project_id TEXT,turn_id TEXT,kind TEXT,payload_json TEXT)").unwrap();
        let text = "Durable \u{6c34} answer";
        for (id, payload) in [
            // Native ingress: NormalizedPayload::Text, no role.
            (
                "answer",
                serde_json::to_string(&crate::acp::intent::NormalizedPayload::Text {
                    text: crate::acp::contract::SafeText::new("text", text).unwrap(),
                })
                .unwrap(),
            ),
            // Legacy rows carry a role; only assistant prose is quotable.
            (
                "legacy-answer",
                serde_json::json!({"kind":"text","payload":{"role":"assistant","text":text}})
                    .to_string(),
            ),
            (
                "prompt",
                serde_json::json!({"kind":"text","payload":{"role":"user","text":text}})
                    .to_string(),
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO items VALUES (?1,'project','turn','text',?2)",
                    rusqlite::params![id, payload],
                )
                .unwrap();
        }
        let quote = |item: &str, turn: &str, end: u64| -> TranscriptQuoteContext {
            serde_json::from_value(serde_json::json!({"kind":"transcriptQuote","ordinal":1,"sourceTurnId":turn,"sourceItemId":item,"text":"answer","startOffset":0,"endOffset":end})).unwrap()
        };
        // Offsets count UTF-16 units of the rendered answer: 16 here.
        assert!(
            validate_quote_source(&connection, "project", &quote("answer", "turn", 16)).is_ok()
        );
        assert!(
            validate_quote_source(&connection, "project", &quote("legacy-answer", "turn", 16))
                .is_ok()
        );
        for (item, turn, end, project) in [
            ("answer", "turn", 17, "project"),
            ("prompt", "turn", 5, "project"),
            ("answer", "other-turn", 5, "project"),
            ("missing", "turn", 5, "project"),
            ("answer", "turn", 5, "other-project"),
        ] {
            assert!(
                validate_quote_source(&connection, project, &quote(item, turn, end)).is_err(),
                "{item} {turn} {end} {project}"
            );
        }
    }
}
