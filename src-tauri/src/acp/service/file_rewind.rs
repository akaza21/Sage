//! Files rewind with the conversation only after a dry run lists them and
//! every listed file still holds the bytes the agent last wrote to it.
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    resource_mapper::{ConfiguredResourceRoot, ContentState, NativeResourceMapper},
};
use crate::project_registry::ProjectRegistryStore;
use sage_acp_protocol::{
    contract::{SafeText, SageRootId},
    intent::{RewindCapability, RewindTargets},
    scheduler::RuntimeOwner,
    session_control::{ConversationTarget, SessionControl},
};

/// Paths a preview shows; the counts cover the rest.
const PREVIEW_PATHS: usize = 200;
const NAME_BYTES: usize = 255;

/// What a file rewind would restore, as root-relative paths. Absolute paths
/// never leave the native side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileRewindPreview {
    pub files: Vec<String>,
    pub file_count: u32,
    /// Files changed since the agent last wrote them, or that Sage cannot
    /// check. Any of them refuses the file rewind.
    pub conflicts: Vec<String>,
    pub conflict_count: u32,
}

impl FileRewindPreview {
    fn add(&mut self, path: String, conflict: bool) {
        if self.files.len() < PREVIEW_PATHS {
            self.files.push(path.clone());
        }
        self.file_count = self.file_count.saturating_add(1);
        if conflict {
            if self.conflicts.len() < PREVIEW_PATHS {
                self.conflicts.push(path);
            }
            self.conflict_count = self.conflict_count.saturating_add(1);
        }
    }
}

pub(crate) fn offers_file_rewind(rewind: RewindCapability) -> bool {
    rewind.effects.conversation
        && rewind.effects.files
        && rewind.effects.file_preview
        && rewind.targets != RewindTargets::LatestTurn
}

pub(crate) type Control<'a> =
    &'a mut dyn FnMut(SessionControl) -> Result<NativeRpcReply, SessionControlError>;

/// Why a files-and-conversation rewind stopped.
#[derive(Debug)]
pub(crate) enum FileRewindFailure<S> {
    /// Nothing was staged or restored.
    Before(SessionControlError),
    /// Listed files changed since the agent wrote them; none was restored.
    Conflicts(FileRewindPreview),
    /// The cut was staged; `restored` says whether files were restored first.
    Staged {
        staged: S,
        restored: bool,
        error: SessionControlError,
    },
}

/// The dry run: the native files a rewind to `target` would restore.
pub(crate) fn plan(
    target: &ConversationTarget,
    control: Control<'_>,
) -> Result<Vec<PathBuf>, SessionControlError> {
    match control(SessionControl::RewindFiles {
        target: target.clone(),
        dry_run: true,
    })? {
        NativeRpcReply::FileRewindPlan(files) => Ok(files),
        NativeRpcReply::TooLarge => Err(refusal("Too many files changed to check them all.")),
        _ => Err(SessionControlError::RecoveryRequired),
    }
}

/// Dry run, then the check, then (only with no conflict) stage the cut,
/// restore the files, and cut the conversation, in that order.
pub(crate) fn files_then_conversation<S>(
    target: &ConversationTarget,
    control: Control<'_>,
    check: impl FnOnce(&[PathBuf]) -> Result<FileRewindPreview, SessionControlError>,
    stage: impl FnOnce() -> Result<(S, SessionControl), SessionControlError>,
) -> Result<(S, Option<SafeText<512>>), FileRewindFailure<S>> {
    let files = plan(target, control).map_err(FileRewindFailure::Before)?;
    let preview = check(&files).map_err(FileRewindFailure::Before)?;
    if preview.conflict_count > 0 {
        return Err(FileRewindFailure::Conflicts(preview));
    }
    let (staged, cut) = stage().map_err(FileRewindFailure::Before)?;
    let restore = SessionControl::RewindFiles {
        target: target.clone(),
        dry_run: false,
    };
    let summary = match controlled(control(restore)) {
        Ok(summary) => summary,
        Err(error) => {
            return Err(FileRewindFailure::Staged {
                staged,
                restored: false,
                error,
            });
        }
    };
    match controlled(control(cut)) {
        Ok(_) => Ok((staged, summary)),
        Err(error) => Err(FileRewindFailure::Staged {
            staged,
            restored: true,
            error,
        }),
    }
}

fn controlled(
    reply: Result<NativeRpcReply, SessionControlError>,
) -> Result<Option<SafeText<512>>, SessionControlError> {
    match reply? {
        NativeRpcReply::Controlled(summary) => Ok(summary),
        _ => Err(SessionControlError::RecoveryRequired),
    }
}

pub(crate) fn refusal(text: &str) -> SessionControlError {
    SessionControlError::Refused(SafeText::new("controlSummary", text).ok())
}

/// Checks each file against the agent's last recorded write to it.
pub(crate) fn check(
    connection: &rusqlite::Connection,
    session: &str,
    mapper: &NativeResourceMapper,
    files: &[PathBuf],
) -> Result<FileRewindPreview, SessionControlError> {
    use crate::acp::bindings::adapter::ResourceMapper;
    let mut preview = FileRewindPreview::default();
    for file in files {
        let resource = file.to_str().and_then(|path| {
            mapper
                .map_existing(path)
                .ok()
                .or_else(|| mapper.map_missing_display(path))
        });
        let Some(resource) = resource else {
            preview.add(file_name(file), true);
            continue;
        };
        let recorded = crate::acp::store::agent_writes::last_write(connection, session, &resource)
            .map_err(|_| SessionControlError::RecoveryRequired)?;
        let conflict = match (recorded, mapper.content_state(&resource)) {
            (Some(recorded), ContentState::Present(current)) => recorded != current,
            _ => true,
        };
        preview.add(resource.relative_path.as_str().to_owned(), conflict);
    }
    Ok(preview)
}

fn file_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(|| "(unnamed)".into(), |name| name.to_string_lossy());
    let mut end = name.len().min(NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_owned()
}

impl AcpService {
    pub(super) fn project_mapper(
        &self,
        project: &str,
    ) -> Result<NativeResourceMapper, SessionControlError> {
        let roots = self
            .database
            .root_records(project)
            .map_err(|_| SessionControlError::Unavailable)?;
        NativeResourceMapper::new(
            roots
                .into_iter()
                .map(|root| {
                    Ok(ConfiguredResourceRoot {
                        root_id: SageRootId::parse(&root.id)
                            .map_err(|_| SessionControlError::Unavailable)?,
                        canonical_native_root: root.absolute_path,
                    })
                })
                .collect::<Result<Vec<_>, SessionControlError>>()?,
        )
        .map_err(|_| SessionControlError::Unavailable)
    }

    pub(crate) fn check_files(
        &self,
        owner: &RuntimeOwner,
        files: &[PathBuf],
    ) -> Result<FileRewindPreview, SessionControlError> {
        let mapper = self.project_mapper(owner.project_id.as_str())?;
        check(
            &self.database.connection(),
            owner.session_id.as_str(),
            &mapper,
            files,
        )
    }

    /// The preview the confirm shows, or `None` when the session cannot
    /// rewind files with the conversation.
    pub(crate) fn preview_file_rewind(
        &self,
        owner: &RuntimeOwner,
        before: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<Option<FileRewindPreview>, SessionControlError> {
        if !offers_file_rewind(self.session_rewind(owner)?) {
            return Ok(None);
        }
        let target = self.rewind_target(owner, before)?;
        let files = plan(&target, &mut |control| {
            self.session_control_reply(owner.clone(), control)
        })?;
        self.check_files(owner, &files).map(Some)
    }

    pub(super) fn session_rewind(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<RewindCapability, SessionControlError> {
        let json: String = self
            .database
            .connection()
            .query_row(
                "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                [owner.session_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| SessionControlError::RecoveryRequired)?;
        serde_json::from_str::<sage_acp_protocol::intent::CapabilitySnapshotV1>(&json)
            .map(|capabilities| capabilities.rewind)
            .map_err(|_| SessionControlError::Unavailable)
    }

    pub(super) fn rewind_target(
        &self,
        owner: &RuntimeOwner,
        turn: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<ConversationTarget, SessionControlError> {
        crate::acp::store::rewind::target(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            turn.as_str(),
        )
        .map_err(|error| match error {
            StoreError::RewindUnavailable { reason } => refusal(reason),
            _ => SessionControlError::Unavailable,
        })
    }
}
