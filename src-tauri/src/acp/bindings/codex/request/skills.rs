use serde_json::{Value, json};

use super::{OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
    /// `skills/list` for the session's working directory.
    pub(crate) fn skills_list(id: u64) -> Result<Self, RequestError> {
        Self::build(id, RequestKind::SkillsList, json!({}))
    }

    /// Attaches a skill ahead of a `turn/start`'s text, as Codex runs one.
    pub(crate) fn with_skill(self, name: &str, path: &str) -> Result<Self, RequestError> {
        let mut value: Value =
            serde_json::from_slice(&self.bytes).map_err(|_| RequestError::Encoding)?;
        let input = value["params"]["input"]
            .as_array_mut()
            .ok_or(RequestError::Encoding)?;
        input.insert(0, json!({"type": "skill", "name": name, "path": path}));
        Self::build(self.id, self.kind, value["params"].take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{contract::RequiredText, intent::Prompt};

    #[test]
    fn a_skill_runs_ahead_of_the_typed_text() {
        let thread = RequiredText::new("thread", "thread-1").unwrap();
        let message = RequiredText::new("message", "message-1").unwrap();
        let prompt = Prompt::new("prompt", "/pdf summarise report.pdf").unwrap();
        let request = OutgoingRequest::turn_start(4, &thread, &prompt, &message)
            .unwrap()
            .with_skill("pdf", "/skills/pdf/SKILL.md")
            .unwrap();
        let value: Value = serde_json::from_slice(&request.bytes).unwrap();
        assert_eq!(value["method"], "turn/start");
        assert_eq!(
            value["params"]["input"],
            json!([
                {"type": "skill", "name": "pdf", "path": "/skills/pdf/SKILL.md"},
                {"type": "text", "text": "/pdf summarise report.pdf"}
            ])
        );
        let list: Value =
            serde_json::from_slice(&OutgoingRequest::skills_list(5).unwrap().bytes).unwrap();
        assert_eq!(
            list,
            json!({"id": 5, "method": "skills/list", "params": {}})
        );
    }
}
