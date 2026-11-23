//! The new-chat draft's staging slot. A draft has no chat tab yet, so its
//! batch is the same `Batch` a chat stages, keyed by a draft token instead
//! of a chat. Its first send hands the batch whole to the created chat.
use super::{Batch, FileStages, StagedFiles, summary};
use serde::Deserialize;

/// No chat owner may name an empty project, so draft keys never collide
/// with a chat's and a chat command can never read a draft's batch.
const DRAFT_PROJECT: &str = "";
/// A draft batch belongs to no chat tab attachment.
const DRAFT_ATTACHMENT: i64 = 0;

/// The new-chat draft that owns a batch, named by a token the WebView makes
/// for it. Only a well-formed UUID is accepted, which bounds the key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftOwner {
    draft_token: String,
}

impl DraftOwner {
    pub(super) fn key(&self) -> Result<(String, String, i64), String> {
        uuid::Uuid::parse_str(&self.draft_token)
            .map(|token| {
                (
                    DRAFT_PROJECT.to_owned(),
                    token.hyphenated().to_string(),
                    DRAFT_ATTACHMENT,
                )
            })
            .map_err(|_| "Attachment draft is invalid.".to_owned())
    }
}

pub(super) fn is_draft(project: &str) -> bool {
    project == DRAFT_PROJECT
}

impl FileStages {
    /// One draft holds files at a time. Staging for a draft drops any other
    /// draft's batch, which also sweeps one a reloaded or crashed WebView
    /// left behind: its token died with it, so nothing could hand it off.
    pub(super) fn release_other_drafts(&mut self, keep: &str) {
        self.batches
            .retain(|(project, token), _| !is_draft(project) || token == keep);
    }

    pub(crate) fn release_draft(&mut self, project: &str, token: &str) {
        if is_draft(project) {
            self.batches.remove(&(project.to_owned(), token.to_owned()));
        }
    }

    /// Chats end with a project switch; the draft is not owned by a project
    /// and keeps its batch for the chat its send creates.
    pub(crate) fn clear_chats(&mut self) {
        self.batches.retain(|(project, _), _| is_draft(project));
        self.sent.clear();
    }

    /// Moves the batch at `from` whole to the empty owner `to`, or nothing.
    /// Exactly one side is a draft, so no batch passes from one chat to
    /// another. The revision is the one the person last saw at `from`; the
    /// moved batch gets a fresh revision at `to`, and `from` is left empty.
    pub(crate) fn hand_off(
        &mut self,
        from: (&str, &str),
        to: (&str, &str, i64),
        revision: &str,
    ) -> Result<StagedFiles, &'static str> {
        if is_draft(from.0) == is_draft(to.0) {
            return Err("Attachments move only between a draft and its chat.");
        }
        let source = (from.0.to_owned(), from.1.to_owned());
        let target = (to.0.to_owned(), to.1.to_owned());
        if self
            .batches
            .get(&source)
            .is_none_or(|batch| batch.revision != revision)
        {
            return Err("Attachments changed; review them and send again.");
        }
        if self.batches.contains_key(&target) {
            return Err("That chat already has staged attachments.");
        }
        let resources = self
            .batches
            .remove(&source)
            .expect("checked above")
            .resources;
        if is_draft(to.0) {
            self.release_other_drafts(to.1);
        }
        let batch = Batch {
            revision: uuid::Uuid::new_v4().to_string(),
            attached_at: to.2,
            resources,
        };
        let moved = summary(&batch);
        self.batches.insert(target, batch);
        Ok(moved)
    }
}
