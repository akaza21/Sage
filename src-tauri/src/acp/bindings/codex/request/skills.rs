use serde_json::{Value, json};

use super::{OutgoingRequest, RequestError, RequestKind};

impl OutgoingRequest {
    /// `skills/list` for the thread's root; `reload` skips Codex's cache.
    pub(crate) fn skills_list(id: u64, root: &str, reload: bool) -> Result<Self, RequestError> {
        let params = if reload {
            json!({"cwds": [root], "forceReload": true})
        } else {
            json!({"cwds": [root]})
        };
        Self::build(id, RequestKind::SkillsList, params)
    }

    /// Attaches a skill in place of the `/name` that picked it: the skill
    /// item comes first, then whatever was typed after the name, as Pi
    /// sends a command's input without its name.
    pub(crate) fn with_skill(self, name: &str, path: &str) -> Result<Self, RequestError> {
        let mut value: Value =
            serde_json::from_slice(&self.bytes).map_err(|_| RequestError::Encoding)?;
        let input = value["params"]["input"]
            .as_array_mut()
            .ok_or(RequestError::Encoding)?;
        let typed = input
            .iter_mut()
            .find(|item| item["type"] == "text")
            .ok_or(RequestError::Encoding)?;
        let rest = typed["text"]
            .as_str()
            .and_then(|text| text.strip_prefix(&format!("/{name}")))
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            .ok_or(RequestError::Encoding)?
            .trim_start()
            .to_owned();
        typed["text"] = Value::String(rest);
        input.retain(|item| item["type"] != "text" || item["text"] != "");
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
                {"type": "text", "text": "summarise report.pdf"}
            ])
        );
        // A skill picked with nothing typed after it is the whole input.
        let bare = Prompt::new("prompt", "/pdf").unwrap();
        let request = OutgoingRequest::turn_start(6, &thread, &bare, &message)
            .unwrap()
            .with_skill("pdf", "/skills/pdf/SKILL.md")
            .unwrap();
        let value: Value = serde_json::from_slice(&request.bytes).unwrap();
        assert_eq!(
            value["params"]["input"],
            json!([{"type": "skill", "name": "pdf", "path": "/skills/pdf/SKILL.md"}])
        );
        // Text that does not start with the skill's name is not rewritten.
        let other = Prompt::new("prompt", "/pdfx summarise").unwrap();
        assert!(
            OutgoingRequest::turn_start(7, &thread, &other, &message)
                .unwrap()
                .with_skill("pdf", "/skills/pdf/SKILL.md")
                .is_err()
        );
        for (reload, params) in [
            (false, json!({"cwds": ["/repo"]})),
            (true, json!({"cwds": ["/repo"], "forceReload": true})),
        ] {
            let list: Value = serde_json::from_slice(
                &OutgoingRequest::skills_list(5, "/repo", reload)
                    .unwrap()
                    .bytes,
            )
            .unwrap();
            assert_eq!(
                list,
                json!({"id": 5, "method": "skills/list", "params": params})
            );
        }
    }
}
