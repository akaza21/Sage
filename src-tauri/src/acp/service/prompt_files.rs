//! Ephemeral native prompt resources. No bytes or paths are persisted.
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

mod attachment;
pub(crate) mod commands;
pub(crate) mod images;
pub(crate) mod limits;
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
pub(crate) use attachment::{attachment, release_detached, retry_origin};

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

/// The standard ACP `session/prompt` content: text, then images, then files.
/// Each kind is sent only when the session's negotiated capabilities admit it.
pub(crate) fn prompt_blocks(
    prompt: &crate::acp::intent::Prompt,
    capabilities: Option<&crate::acp::intent::CapabilitySnapshotV1>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut blocks = prompt
        .resources
        .wire(
            prompt.as_str(),
            capabilities.is_some_and(|value| value.prompt_resources),
        )
        .map_err(str::to_owned)?;
    let images = prompt
        .images
        .wire(capabilities)
        .map_err(images::ImageRefusal::message)?;
    blocks.splice(1..1, images);
    Ok(blocks)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedFiles {
    pub revision: String,
    pub files: Vec<FileSummary>,
    /// The resource a paste just added, so its preview attaches by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<String>,
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
/// What a chat's latest dispatched prompt carried, kept so Retry of that
/// one turn can resend it. A send without files is recorded as empty, and
/// `None` marks files released to stay within the shared caps.
struct Sent {
    attached_at: i64,
    client_intent_id: String,
    resources: Option<Resources>,
    /// Recency: the least recent record is released first.
    sequence: u64,
}
/// The turn a Retry repeats, as its durable record describes it.
pub(crate) struct RetryOrigin {
    pub client_intent_id: String,
    /// How many resources its prompt carried; persisted as a count only.
    /// `None` when its prompt record is missing or unreadable: such a turn
    /// may have carried files, so Retry without a kept record is refused.
    pub attachments: Option<usize>,
}
/// Which staged files a send carries, resolved against durable state.
pub(crate) enum Selection {
    /// None: a queued message or slash command never takes a batch.
    None,
    Revision(String),
    Retry(RetryOrigin),
}
#[derive(Default)]
pub(crate) struct FileStages {
    batches: BTreeMap<(String, String), Batch>,
    sent: BTreeMap<(String, String), Sent>,
    sequence: u64,
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
        // Files kept for Retry share the memory cap and give way first.
        while self.retained(Some(&key)) + resources.retained_bytes() > STORE_BYTES {
            if !self.release_oldest_sent(None) {
                return Err(
                    "Staged attachment memory limit reached; remove other attachments first.",
                );
            }
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
    /// Bytes held by staged batches (other than `except`'s) and kept sends.
    fn retained(&self, except: Option<&(String, String)>) -> usize {
        let staged = self.batches.iter().filter(|(k, _)| Some(*k) != except);
        let sent = self.sent.values().filter_map(|s| s.resources.as_ref());
        staged
            .map(|(_, b)| b.resources.retained_bytes())
            .sum::<usize>()
            + sent.map(Resources::retained_bytes).sum::<usize>()
    }
    /// Releases the least recently sent files held for Retry, other than
    /// `keep`'s. False when none are left to release.
    fn release_oldest_sent(&mut self, keep: Option<&(String, String)>) -> bool {
        let oldest = self
            .sent
            .iter_mut()
            .filter(|(k, s)| {
                Some(*k) != keep && s.resources.as_ref().is_some_and(|r| !r.0.is_empty())
            })
            .min_by_key(|(_, s)| s.sequence);
        oldest.map(|(_, s)| s.resources = None).is_some()
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
            return Err(limits::PASTE_OVER_TOTAL.as_str());
        }
        let current = self
            .batches
            .get(&(project.to_owned(), chat.to_owned()))
            .filter(|b| b.attached_at == attached_at)
            .map(|b| b.resources.0.as_slice())
            .unwrap_or_default();
        if current.len() >= FILE_COUNT {
            return Err(limits::OVER_COUNT.as_str());
        }
        if current.iter().map(|r| r.bytes.len()).sum::<usize>() + text.len() > TOTAL_BYTES {
            return Err(limits::OVER_TOTAL.as_str());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut next = Vec::with_capacity(current.len() + 1);
        next.extend_from_slice(current);
        next.push(Resource {
            id: id.clone(),
            name: PASTED_TEXT_NAME.to_owned(),
            bytes: text.into_bytes(),
        });
        let mut staged = self.stage(
            project,
            chat,
            attached_at,
            expected,
            Resources(Arc::new(next)),
        )?;
        staged.added = Some(id);
        Ok(staged)
    }
    /// The files a send carries. Naming no selection carries none, so a
    /// queued message or a slash command never takes a batch staged for the
    /// message the person is writing.
    pub(crate) fn selected(
        &self,
        project: &str,
        chat: &str,
        selection: &Selection,
    ) -> Result<Resources, &'static str> {
        let key = (project.to_owned(), chat.to_owned());
        match selection {
            Selection::None => Ok(Resources::default()),
            Selection::Revision(revision) => self
                .batches
                .get(&key)
                .filter(|b| b.revision == *revision)
                .map(|b| b.resources.clone())
                .ok_or("Attachments changed; review them and send again."),
            Selection::Retry(origin) => match self.sent.get(&key) {
                Some(sent) if sent.client_intent_id == origin.client_intent_id => sent
                    .resources
                    .clone()
                    .ok_or("The attachments sent with that message were released; attach them again and send a new message."),
                // No record for that turn (a restart, a reopened tab): resend
                // nothing only when the turn is known to have carried nothing.
                _ => match origin.attachments {
                    Some(0) => Ok(Resources::default()),
                    Some(_) => Err(
                        "The attachments sent with that message are no longer held. Attach them again and send a new message.",
                    ),
                    None => Err(
                        "That message's record does not show whether it carried attachments, so it cannot be retried. Send it again as a new message.",
                    ),
                },
            },
        }
    }
    #[cfg(test)]
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
    /// A dispatched batch leaves the stage, matched by pointer so a batch
    /// staged after it stays, even when the send cannot be recorded for
    /// Retry; otherwise the next message would carry the same files again.
    pub(crate) fn unstage_sent(&mut self, project: &str, chat: &str, sent: &Resources) {
        let key = (project.to_owned(), chat.to_owned());
        if self
            .batches
            .get(&key)
            .is_some_and(|b| !sent.0.is_empty() && Arc::ptr_eq(&b.resources.0, &sent.0))
        {
            self.batches.remove(&key);
        }
    }
    /// Records a dispatched prompt as the chat's latest send, under its
    /// client intent, so Retry of that turn resends exactly what it carried:
    /// a sent batch leaves the stage, and a send without files is recorded
    /// as carrying none. Only the latest send per chat is kept.
    pub(crate) fn consume(
        &mut self,
        project: &str,
        chat: &str,
        attached_at: i64,
        client_intent_id: &str,
        sent: &Resources,
    ) {
        self.unstage_sent(project, chat, sent);
        let key = (project.to_owned(), chat.to_owned());
        self.sequence += 1;
        self.sent.insert(
            key.clone(),
            Sent {
                attached_at,
                client_intent_id: client_intent_id.to_owned(),
                resources: Some(sent.clone()),
                sequence: self.sequence,
            },
        );
        self.bound_sent(&key);
    }
    /// Files kept for Retry share the owner and memory caps with staged
    /// batches; the least recently sent other entries are released first.
    fn bound_sent(&mut self, keep: &(String, String)) {
        loop {
            let held = self
                .sent
                .values()
                .filter_map(|s| s.resources.as_ref())
                .filter(|r| !r.0.is_empty());
            let count = held.clone().count();
            if count <= OWNER_COUNT && self.retained(None) <= STORE_BYTES {
                return;
            }
            if !self.release_oldest_sent(Some(keep)) {
                return;
            }
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
        added: None,
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
