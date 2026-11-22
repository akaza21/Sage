//! Files rewind with the conversation only after a dry run lists them and
//! every listed file still holds the bytes the agent last wrote to it.
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    resource_mapper::{ConfiguredResourceRoot, ContentState, NativeResourceMapper},
    store::agent_writes::{self, LastWrites},
};
use crate::project_registry::ProjectRegistryStore;
use sage_acp_protocol::{
    contract::{SafeText, SageRootId, SageTurnId},
    intent::{RewindCapability, RewindTargets},
    scheduler::RuntimeOwner,
    session_control::{ConversationTarget, SessionControl},
};

/// Paths a preview shows; the counts cover the rest.
const PREVIEW_PATHS: usize = 200;
const NAME_BYTES: usize = 255;
/// Bytes one check may hash. Files past it are unverified, so they refuse.
const CHECK_HASH_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const RESTORED_NOT_CUT: &str =
    "Files were restored, but the conversation was not rewound.";
pub(crate) const RESTORED_RECOVERING: &str =
    "Files were restored; this chat is recovering and the rewind did not finish.";

/// What a file rewind would restore, as root-relative paths. Absolute paths
/// never leave the native side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileRewindPreview {
    pub files: Vec<String>,
    pub file_count: u32,
    /// Files changed since the agent last wrote them.
    pub conflicts: Vec<String>,
    pub conflict_count: u32,
    /// Files with no write Sage can check them against: never recorded, or
    /// recorded unread, from a cut turn, or unreadable now.
    pub unverified: Vec<String>,
    pub unverified_count: u32,
}

/// How one listed file stands against the agent's last write to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    Unchanged,
    Changed,
    Unverified,
}

impl FileRewindPreview {
    fn add(&mut self, path: String, standing: Standing) {
        fn push(list: &mut Vec<String>, count: &mut u32, path: String) {
            if list.len() < PREVIEW_PATHS {
                list.push(path);
            }
            *count = count.saturating_add(1);
        }
        match standing {
            Standing::Unchanged => {}
            Standing::Changed => push(&mut self.conflicts, &mut self.conflict_count, path.clone()),
            Standing::Unverified => push(
                &mut self.unverified,
                &mut self.unverified_count,
                path.clone(),
            ),
        }
        push(&mut self.files, &mut self.file_count, path);
    }

    /// Any changed or unverified file keeps every file as it is.
    pub(crate) fn refusal(&self) -> Option<SessionControlError> {
        if self.conflict_count > 0 {
            return Some(refusal(&format!(
                "{} of the files changed since the agent wrote them. Rewind the conversation only.",
                self.conflict_count
            )));
        }
        (self.unverified_count > 0).then(|| {
            refusal(&format!(
                "Sage can't verify {} of the files in this chat. Rewind the conversation only.",
                self.unverified_count
            ))
        })
    }
}

pub(crate) fn offers_file_rewind(rewind: RewindCapability) -> bool {
    rewind.effects.conversation
        && rewind.effects.files
        && rewind.effects.file_preview
        && rewind.targets != RewindTargets::LatestTurn
}

pub(crate) type Control<'a> =
    &'a mut (dyn FnMut(SessionControl) -> Result<NativeRpcReply, SessionControlError> + Send);

/// Why a files-and-conversation rewind stopped after its check.
#[derive(Debug)]
pub(crate) enum FileRewindFailure<S> {
    /// Nothing was staged or restored.
    Before(SessionControlError),
    /// Listed files changed or cannot be verified; none was restored.
    Conflicts(FileRewindPreview),
    /// The cut was staged; `restored` says whether files were restored first.
    Staged {
        staged: S,
        restored: bool,
        error: SessionControlError,
    },
}

/// What a failure after staging tells the user, once the staged cut was
/// rolled back (`Refused`/`Unavailable` kept) or left to recovery
/// (`RecoveryRequired`): with files restored, it says so either way.
pub(crate) fn staged_error(restored: bool, error: SessionControlError) -> SessionControlError {
    match (restored, error) {
        (false, error) => error,
        (true, SessionControlError::Refused(_) | SessionControlError::Unavailable) => {
            refusal(RESTORED_NOT_CUT)
        }
        (true, _) => refusal(RESTORED_RECOVERING),
    }
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

/// After a clean check: stage the cut, restore the planned files (skipped
/// when there are none), then cut the conversation, in that order. The
/// third value says whether files were restored.
pub(crate) fn restore_then_cut<S>(
    target: &ConversationTarget,
    planned: &[PathBuf],
    preview: FileRewindPreview,
    control: Control<'_>,
    stage: impl FnOnce() -> Result<(S, SessionControl), SessionControlError>,
) -> Result<(S, Option<SafeText<512>>, bool), FileRewindFailure<S>> {
    if preview.refusal().is_some() {
        return Err(FileRewindFailure::Conflicts(preview));
    }
    let (staged, cut) = stage().map_err(FileRewindFailure::Before)?;
    let restored = !planned.is_empty();
    let summary = if restored {
        let restore = control(SessionControl::RewindFiles {
            target: target.clone(),
            dry_run: false,
        });
        match restore {
            Ok(NativeRpcReply::FileRewindPlan(files)) => restored_summary(planned, &files),
            Ok(_) => {
                return Err(FileRewindFailure::Staged {
                    staged,
                    restored: false,
                    error: SessionControlError::RecoveryRequired,
                });
            }
            Err(error) => {
                return Err(FileRewindFailure::Staged {
                    staged,
                    restored: false,
                    error,
                });
            }
        }
    } else {
        SafeText::new("controlSummary", "No files needed restoring").ok()
    };
    match control(cut) {
        Ok(NativeRpcReply::Controlled(_)) => Ok((staged, summary, restored)),
        Ok(_) => Err(FileRewindFailure::Staged {
            staged,
            restored,
            error: SessionControlError::RecoveryRequired,
        }),
        Err(error) => Err(FileRewindFailure::Staged {
            staged,
            restored,
            error,
        }),
    }
}

/// The restore names what it restored; any file the preview did not list
/// is said, never hidden.
fn restored_summary(planned: &[PathBuf], restored: &[PathBuf]) -> Option<SafeText<512>> {
    let unlisted = restored
        .iter()
        .filter(|file| !planned.contains(file))
        .count();
    let text = match (restored.len(), unlisted) {
        (1, 0) => "Restored 1 file".to_owned(),
        (count, 0) => format!("Restored {count} files"),
        (count, unlisted) => {
            format!("Restored {count} files, {unlisted} of them not in the preview")
        }
    };
    SafeText::new("controlSummary", text).ok()
}

pub(crate) fn refusal(text: &str) -> SessionControlError {
    SessionControlError::Refused(SafeText::new("controlSummary", text).ok())
}

/// One dry-run file: how it shows, its record key, and its bytes now.
pub(crate) struct Probed {
    shown: String,
    key: Option<(String, String)>,
    state: ContentState,
}

/// Maps and hashes the files within one byte budget. It reads only files:
/// no database connection is held while it runs.
pub(crate) fn probe(mapper: &NativeResourceMapper, files: &[PathBuf]) -> Vec<Probed> {
    use crate::acp::bindings::adapter::ResourceMapper;
    let mut budget = CHECK_HASH_BYTES;
    files
        .iter()
        .map(|file| {
            let resource = file.to_str().and_then(|path| {
                mapper
                    .map_existing(path)
                    .ok()
                    .or_else(|| mapper.map_missing_display(path))
            });
            match resource {
                Some(resource) => Probed {
                    state: mapper
                        .probe(&resource)
                        .map_or(ContentState::Unreadable, |probe| probe.state(&mut budget)),
                    shown: resource.relative_path.as_str().to_owned(),
                    key: Some((
                        resource.root_id.as_str().to_owned(),
                        resource.relative_path.as_str().to_owned(),
                    )),
                },
                None => Probed {
                    shown: file_name(file),
                    key: None,
                    state: ContentState::Unreadable,
                },
            }
        })
        .collect()
}

/// A file is unchanged while its bytes are the agent's recorded last write,
/// changed once they differ or it is gone, and unverified without a write to
/// compare or bytes to read.
pub(crate) fn compare(probed: Vec<Probed>, writes: &LastWrites) -> FileRewindPreview {
    let mut preview = FileRewindPreview::default();
    for file in probed {
        let recorded = file
            .key
            .as_ref()
            .and_then(|key| writes.get(key).copied().flatten());
        let standing = match (recorded, file.state) {
            (Some(recorded), ContentState::Present(current)) if recorded == current => {
                Standing::Unchanged
            }
            (Some(_), ContentState::Present(_) | ContentState::Missing) => Standing::Changed,
            _ => Standing::Unverified,
        };
        preview.add(file.shown, standing);
    }
    preview
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
    fn project_mapper(&self, project: &str) -> Result<NativeResourceMapper, SessionControlError> {
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

    /// Hashes on the blocking pool with no connection held, then reads every
    /// recorded write in one query.
    pub(crate) async fn check_files(
        &self,
        owner: &RuntimeOwner,
        files: Vec<PathBuf>,
    ) -> Result<FileRewindPreview, SessionControlError> {
        let mapper = self.project_mapper(owner.project_id.as_str())?;
        let probed = tauri::async_runtime::spawn_blocking(move || probe(&mapper, &files))
            .await
            .map_err(|_| SessionControlError::Unavailable)?;
        let writes =
            agent_writes::last_writes(&self.database.connection(), owner.session_id.as_str())
                .map_err(|_| SessionControlError::RecoveryRequired)?;
        Ok(compare(probed, &writes))
    }

    /// Dry run, check, then stage, restore and cut, each control through
    /// `control`. A failure after staging rolls the cut back or leaves it to
    /// recovery, and says whether files were restored.
    pub(super) async fn rewind_files_then_cut(
        &self,
        owner: &RuntimeOwner,
        target: &ConversationTarget,
        control: Control<'_>,
        stage: impl FnOnce() -> Result<
            (crate::acp::store::TurnUndoneCommand, SessionControl),
            SessionControlError,
        >,
    ) -> Result<
        (
            crate::acp::store::TurnUndoneCommand,
            Option<SafeText<512>>,
            bool,
        ),
        SessionControlError,
    > {
        let planned = plan(target, control)?;
        let preview = self.check_files(owner, planned.clone()).await?;
        match restore_then_cut(target, &planned, preview, control, stage) {
            Ok(done) => Ok(done),
            Err(FileRewindFailure::Before(error)) => Err(error),
            Err(FileRewindFailure::Conflicts(preview)) => Err(preview
                .refusal()
                .unwrap_or(SessionControlError::Unavailable)),
            Err(FileRewindFailure::Staged {
                staged,
                restored,
                error,
            }) => Err(staged_error(
                restored,
                self.abandon_staged(owner, &staged, error),
            )),
        }
    }

    /// The preview the confirm shows, or `None` when the session cannot
    /// rewind files with the conversation.
    pub(crate) async fn preview_file_rewind(
        &self,
        owner: &RuntimeOwner,
        before: &SageTurnId,
    ) -> Result<Option<FileRewindPreview>, SessionControlError> {
        if !offers_file_rewind(self.session_rewind(owner)?) {
            return Ok(None);
        }
        let target = self.rewind_target(owner, before)?;
        let files = plan(&target, &mut |control| {
            self.session_control_reply(owner.clone(), control)
        })?;
        self.check_files(owner, files).await.map(Some)
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
        turn: &SageTurnId,
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
