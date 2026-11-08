//! Ephemeral native prompt resources. No bytes or paths are persisted.
use crate::acp::intent::StagedSelection;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

mod attachment;
pub(crate) mod commands;
mod read;
mod secrets;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;

pub(crate) const FILE_BYTES: usize = 64 * 1024;
pub(crate) const TOTAL_BYTES: usize = 96 * 1024;
pub(crate) const FILE_COUNT: usize = 16;
/// Prompt text, attached context and staged files together, per dispatch.
pub(crate) const DISPATCH_BYTES: usize = 120 * 1024;
/// The name a pasted-text resource carries; its URI still holds its own id.
pub(crate) const PASTED_TEXT_NAME: &str = "pasted-text.txt";
const OWNER_COUNT: usize = 16;
const STORE_BYTES: usize = 1024 * 1024;
pub(crate) use attachment::{attachment, release_detached};

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
/// What a chat's latest dispatched prompt carried, kept so Retry can resend
/// it. `None` marks files that were released to stay within the caps.
struct Sent {
    attached_at: i64,
    resources: Option<Resources>,
}
#[derive(Default)]
pub(crate) struct FileStages {
    batches: BTreeMap<(String, String), Batch>,
    sent: BTreeMap<(String, String), Sent>,
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
    /// Adds pasted composer text to the chat's batch as one more text
    /// resource. The batch caps (count and total bytes) apply to the whole
    /// batch, so a paste is admitted only while the batch still fits.
    pub(crate) fn append_text(
        &mut self,
        project: &str,
        chat: &str,
        attached_at: i64,
        expected: Option<&str>,
        text: String,
    ) -> Result<StagedFiles, &'static str> {
        if text.is_empty() {
            return Err("Pasted text is empty.");
        }
        if text.len() > TOTAL_BYTES {
            return Err("Pasted text exceeds the 96 KiB attachment limit.");
        }
        let current = self
            .batches
            .get(&(project.to_owned(), chat.to_owned()))
            .filter(|b| b.attached_at == attached_at)
            .map(|b| b.resources.0.as_slice())
            .unwrap_or_default();
        if current.len() >= FILE_COUNT {
            return Err("Attachments exceed the 16-file limit.");
        }
        if current.iter().map(|r| r.bytes.len()).sum::<usize>() + text.len() > TOTAL_BYTES {
            return Err("Attachments exceed 96 KiB total.");
        }
        let mut next = Vec::with_capacity(current.len() + 1);
        next.extend_from_slice(current);
        next.push(Resource {
            id: uuid::Uuid::new_v4().to_string(),
            name: PASTED_TEXT_NAME.to_owned(),
            bytes: text.into_bytes(),
        });
        self.stage(
            project,
            chat,
            attached_at,
            expected,
            Resources(Arc::new(next)),
        )
    }
    /// The files a send carries. Naming no selection carries none, so a
    /// queued message or a slash command never takes a batch staged for the
    /// message the person is writing.
    pub(crate) fn selected(
        &self,
        project: &str,
        chat: &str,
        selection: Option<&StagedSelection>,
    ) -> Result<Resources, &'static str> {
        let key = (project.to_owned(), chat.to_owned());
        match selection {
            None => Ok(Resources::default()),
            Some(StagedSelection::Revision(revision)) => self
                .batches
                .get(&key)
                .filter(|b| &b.revision == revision)
                .map(|b| b.resources.clone())
                .ok_or("Attachments changed; review them and send again."),
            Some(StagedSelection::LastSent) => match self.sent.get(&key) {
                None => Ok(Resources::default()),
                Some(Sent {
                    resources: Some(resources),
                    ..
                }) => Ok(resources.clone()),
                Some(Sent {
                    resources: None, ..
                }) => Err(
                    "The attachments sent with that message were released; attach them again and send a new message.",
                ),
            },
        }
    }
    pub(crate) fn get(&self, project: &str, chat: &str) -> Resources {
        self.batches
            .get(&(project.to_owned(), chat.to_owned()))
            .map(|b| b.resources.clone())
            .unwrap_or_default()
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
    /// Records a dispatched prompt. A sent batch leaves the stage and is kept
    /// as the chat's last sent files; a send without files clears them, so
    /// Retry of the latest message resends exactly what it carried.
    pub(crate) fn consume(&mut self, project: &str, chat: &str, sent: &Resources) {
        let key = (project.to_owned(), chat.to_owned());
        if sent.0.is_empty() {
            self.sent.remove(&key);
            return;
        }
        if self
            .batches
            .get(&key)
            .is_some_and(|b| Arc::ptr_eq(&b.resources.0, &sent.0))
        {
            let batch = self.batches.remove(&key).expect("checked above");
            self.sent.insert(
                key.clone(),
                Sent {
                    attached_at: batch.attached_at,
                    resources: Some(batch.resources),
                },
            );
            self.bound_sent(&key);
        }
    }
    /// Kept sent files share the owner and memory caps with staged batches'
    /// own limits; the oldest-keyed other entries are released first.
    fn bound_sent(&mut self, keep: &(String, String)) {
        loop {
            let held = self.sent.values().filter_map(|s| s.resources.as_ref());
            let count = held.clone().count();
            let bytes = held.map(Resources::retained_bytes).sum::<usize>();
            if count <= OWNER_COUNT && bytes <= STORE_BYTES {
                return;
            }
            let Some(other) = self
                .sent
                .iter_mut()
                .find(|(k, s)| *k != keep && s.resources.is_some())
            else {
                return;
            };
            other.1.resources = None;
        }
    }
    pub(crate) fn clear_chat(&mut self, project: &str, chat: &str) {
        let key = (project.to_owned(), chat.to_owned());
        self.batches.remove(&key);
        self.sent.remove(&key);
    }
    pub(crate) fn clear(&mut self) {
        self.batches.clear();
        self.sent.clear();
    }
    fn owners(&self) -> Vec<(String, String, i64)> {
        let sent = self.sent.iter().map(|(k, s)| (k, s.attached_at));
        self.batches
            .iter()
            .map(|(k, batch)| (k, batch.attached_at))
            .chain(sent)
            .map(|((project, chat), attached_at)| (project.clone(), chat.clone(), attached_at))
            .collect()
    }
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
