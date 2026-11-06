//! Ephemeral native prompt resources. No bytes or paths are persisted.
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) mod commands;
mod read;
#[cfg(test)]
mod tests;

pub(crate) const FILE_BYTES: usize = 64 * 1024;
pub(crate) const TOTAL_BYTES: usize = 96 * 1024;
pub(crate) const FILE_COUNT: usize = 16;
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
        expected: Option<&str>,
        resources: Resources,
    ) -> Result<StagedFiles, &'static str> {
        let key = (project.to_owned(), chat.to_owned());
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
