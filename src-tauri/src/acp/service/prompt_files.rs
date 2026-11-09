//! Ephemeral native prompt resources. No bytes or paths are persisted.
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) mod commands;
pub(crate) mod images;
mod read;
pub(crate) use read::file_is_excluded;
pub(crate) mod secrets;
#[cfg(test)]
mod tests;

pub(crate) const FILE_BYTES: usize = 64 * 1024;
pub(crate) const TOTAL_BYTES: usize = 96 * 1024;
pub(crate) const FILE_COUNT: usize = 16;
/// Prompt text, attached context and staged files together, per dispatch.
pub(crate) const DISPATCH_BYTES: usize = 120 * 1024;
const OWNER_COUNT: usize = 16;
const STORE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resource {
    pub id: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resources(pub Arc<Vec<Resource>>);
impl Default for Resources {
    fn default() -> Self {
        static EMPTY: std::sync::OnceLock<Arc<Vec<Resource>>> = std::sync::OnceLock::new();
        Self(Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new()))))
    }
}
impl Resources {
    pub(crate) fn same_selection(&self, other: &Self) -> bool {
        (self.0.is_empty() && other.0.is_empty()) || Arc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<Resource>()
            + self
                .0
                .iter()
                .map(|r| r.bytes.capacity() + r.name.capacity() + r.id.capacity())
                .sum::<usize>()
    }
    pub(crate) fn wire(
        &self,
        text: &str,
        supported: bool,
    ) -> Result<Vec<serde_json::Value>, &'static str> {
        if !self.0.is_empty() && !supported {
            return Err(
                "This harness does not advertise embedded-resource prompt support. Remove the file attachments or choose another harness.",
            );
        }
        use base64::Engine;
        let mut blocks = Vec::with_capacity(1 + self.0.len());
        blocks.push(serde_json::json!({"type":"text","text":text}));
        for r in self.0.iter() {
            // Opaque, non-file URI: the only granted content is this immutable snapshot.
            let mut uri = url::Url::parse("sage-attachment:///").expect("static URI");
            uri.set_path(&format!("{}/{}", r.id, r.name));
            let resource = match std::str::from_utf8(&r.bytes) {
                Ok(text)
                    if serde_json::to_string(text)
                        .is_ok_and(|s| s.len() <= r.bytes.len() * 4 / 3 + 4) =>
                {
                    serde_json::json!({"uri":uri,"mimeType":"text/plain","text":text})
                }
                _ => {
                    serde_json::json!({"uri":uri,"mimeType":"application/octet-stream","blob":base64::engine::general_purpose::STANDARD.encode(&r.bytes)})
                }
            };
            blocks.push(serde_json::json!({"type":"resource","resource":resource}));
        }
        Ok(blocks)
    }
}

/// The standard ACP `session/prompt` content: text, then images, then files,
/// then `@` mentions. Each kind is sent only when the session's negotiated
/// capabilities admit it; a mention without embedded context is a link.
pub(crate) fn prompt_blocks(
    prompt: &crate::acp::intent::Prompt,
    capabilities: Option<&crate::acp::intent::CapabilitySnapshotV1>,
) -> Result<Vec<serde_json::Value>, String> {
    let embedded = capabilities.is_some_and(|value| value.prompt_resources);
    let mut blocks = prompt
        .resources
        .wire(prompt.as_str(), embedded)
        .map_err(str::to_owned)?;
    let images = prompt
        .images
        .wire(capabilities)
        .map_err(images::ImageRefusal::message)?;
    blocks.splice(1..1, images);
    blocks.extend(prompt.mentions.blocks(embedded));
    Ok(blocks)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedFiles {
    pub revision: String,
    pub files: Vec<FileSummary>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileSummary {
    pub id: String,
    pub name: String,
    pub bytes: usize,
}
struct Batch {
    revision: String,
    /// The `chat_tabs.attached_at` of the tab the batch was staged in. A batch
    /// belongs to that one attachment: closing the tab ends it, and reopening
    /// the chat later starts a new attachment that never inherits it.
    attached_at: i64,
    resources: Resources,
}
#[derive(Default)]
pub(crate) struct FileStages {
    batches: BTreeMap<(String, String), Batch>,
}
impl FileStages {
    pub(crate) fn stage(
        &mut self,
        project: &str,
        chat: &str,
        attached_at: i64,
        expected: Option<&str>,
        resources: Resources,
    ) -> Result<StagedFiles, &'static str> {
        let key = (project.to_owned(), chat.to_owned());
        if self
            .batches
            .get(&key)
            .is_some_and(|b| b.attached_at != attached_at)
        {
            self.batches.remove(&key);
        }
        let current = self.batches.get(&key);
        if current.map(|b| b.revision.as_str()) != expected {
            return Err("Attachments changed; reload the staged attachments before retrying.");
        }
        if current.is_none() && self.batches.len() >= OWNER_COUNT {
            return Err("Too many chats have staged attachments.");
        }
        let retained = self
            .batches
            .iter()
            .filter(|(k, _)| *k != &key)
            .map(|(_, b)| b.resources.retained_bytes())
            .sum::<usize>();
        if retained + resources.retained_bytes() > STORE_BYTES {
            return Err("Staged attachment memory limit reached; remove other attachments first.");
        }
        let batch = Batch {
            revision: uuid::Uuid::new_v4().to_string(),
            attached_at,
            resources,
        };
        let summary = summary(&batch);
        self.batches.insert(key, batch);
        Ok(summary)
    }
    pub(crate) fn get(&self, project: &str, chat: &str) -> Resources {
        self.batches
            .get(&(project.to_owned(), chat.to_owned()))
            .map(|b| b.resources.clone())
            .unwrap_or_default()
    }
    /// What a prompt sends from the chat's staged files. A session action
    /// sends none, so the staged files stay for the next prompt.
    pub(crate) fn for_prompt(
        &self,
        project: &str,
        chat: &str,
        prompt: &crate::acp::intent::Prompt,
    ) -> Resources {
        if prompt.action.is_some() {
            Resources::default()
        } else {
            self.get(project, chat)
        }
    }
    pub(crate) fn list(&self, project: &str, chat: &str) -> Option<StagedFiles> {
        self.batches
            .get(&(project.to_owned(), chat.to_owned()))
            .map(summary)
    }
    pub(crate) fn remove(
        &mut self,
        project: &str,
        chat: &str,
        revision: &str,
        id: &str,
    ) -> Result<Option<StagedFiles>, &'static str> {
        let key = (project.to_owned(), chat.to_owned());
        let batch = self
            .batches
            .get_mut(&key)
            .ok_or("No attachments are staged for this chat.")?;
        if batch.revision != revision {
            return Err("Attachments changed; reload before removing.");
        }
        if !batch.resources.0.iter().any(|r| r.id == id) {
            return Err("Attachment does not belong to this chat.");
        }
        Arc::make_mut(&mut batch.resources.0).retain(|r| r.id != id);
        if batch.resources.0.is_empty() {
            self.batches.remove(&key);
            return Ok(None);
        }
        batch.revision = uuid::Uuid::new_v4().to_string();
        Ok(Some(summary(batch)))
    }
    pub(crate) fn consume(&mut self, project: &str, chat: &str, sent: &Resources) {
        let key = (project.to_owned(), chat.to_owned());
        if self
            .batches
            .get(&key)
            .is_some_and(|b| Arc::ptr_eq(&b.resources.0, &sent.0))
        {
            self.batches.remove(&key);
        }
    }
    pub(crate) fn clear_chat(&mut self, project: &str, chat: &str) {
        self.batches.remove(&(project.to_owned(), chat.to_owned()));
    }
    pub(crate) fn clear(&mut self) {
        self.batches.clear();
    }
    fn owners(&self) -> Vec<(String, String, i64)> {
        self.batches
            .iter()
            .map(|((project, chat), batch)| (project.clone(), chat.clone(), batch.attached_at))
            .collect()
    }
}

/// The one ownership rule for staged batches: a batch lives only while the
/// chat tab attachment it was staged in still exists. Any path that closes a
/// tab, deletes, moves or archives a chat, or saves a layout without the tab
/// ends that attachment; this drops the batch so it no longer counts against
/// the owner and memory caps and is never sent from a reopened tab. Attachment
/// commands and sends run it before they read stages, and layout persistence
/// runs it after a commit so closed tabs release their memory promptly.
pub(crate) fn release_detached(
    stages: &std::sync::Mutex<FileStages>,
    database: &crate::persistence::Database,
) {
    let owners = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .owners();
    if owners.is_empty() {
        return;
    }
    let ended: Vec<_> = owners
        .into_iter()
        .filter(|(project, chat, attached_at)| {
            attachment(database, project, chat).is_ok_and(|live| live != Some(*attached_at))
        })
        .collect();
    let mut stages = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (project, chat, attached_at) in ended {
        let key = (project, chat);
        if stages
            .batches
            .get(&key)
            .is_some_and(|b| b.attached_at == attached_at)
        {
            stages.batches.remove(&key);
        }
    }
}

/// The `attached_at` of the chat's current tab, or `None` when it has none.
pub(crate) fn attachment(
    database: &crate::persistence::Database,
    project: &str,
    chat: &str,
) -> rusqlite::Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    database
        .connection()
        .query_row(
            "SELECT attached_at FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![project, chat],
            |row| row.get(0),
        )
        .optional()
}
fn summary(batch: &Batch) -> StagedFiles {
    StagedFiles {
        revision: batch.revision.clone(),
        files: batch
            .resources
            .0
            .iter()
            .map(|r| FileSummary {
                id: r.id.clone(),
                name: r.name.clone(),
                bytes: r.bytes.len(),
            })
            .collect(),
    }
}
