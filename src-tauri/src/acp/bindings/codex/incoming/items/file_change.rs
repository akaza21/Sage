use serde_json::{Map, Value};

use crate::acp::{
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    file_diff::{PairAdmission, admit_pair},
    intent::{
        FileChangeFacts, NormalizedPayload, SafeResourceLocation, ToolCategory, ToolExecution,
    },
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

use super::{
    IngressError, ItemNormalization, ResourceMapper, ToolPresentationInput, content,
    lenient_tool_state, terminal_lifecycle, text, tool_with_summary_status, unsupported,
};

mod patch;

pub(in crate::acp::bindings::codex::incoming) struct FilePair {
    pub old_text: String,
    pub new_text: String,
}

type FileChangeNormalization = (ItemNormalization, Option<FilePair>);
/// The one change object and its facts before any line counts.
type DescribedChange<'a> = (&'a Map<String, Value>, FileChangeFacts);

/// The Edit tool row for an applied change, as every harness shows a file
/// edit; `None` when the change was declined or failed (nothing was edited)
/// or spans several resources.
pub(in crate::acp::bindings::codex::incoming) fn edit_tool<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
) -> Result<Option<ItemNormalization>, IngressError> {
    if lenient_tool_state(item, ItemLifecycle::Completed) != ToolState::Completed {
        return Ok(None);
    }
    let Some((_, facts)) = describe(item, mapper)? else {
        return Ok(None);
    };
    tool_with_summary_status(ToolPresentationInput {
        category: ToolCategory::Edit,
        label: facts.summary.as_str(),
        tool_state: ToolState::Completed,
        input_summary: None,
        output_summary: None,
        locations: BoundedVec::new("editLocations", vec![facts.resource]).map_err(content)?,
        command_activity: CommandActivity::Ordinary,
        execution: ToolExecution::default(),
    })
    .map(Some)
}

pub(in crate::acp::bindings::codex::incoming) fn normalize<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
    retain_pair: bool,
) -> Result<FileChangeNormalization, IngressError> {
    let Some((change, mut facts)) = describe(item, mapper)? else {
        return Ok((
            (
                unsupported("fileChange", "File change spans multiple resources")?,
                None,
                Some(ItemLifecycle::Unsupported),
            ),
            None,
        ));
    };
    let decoded = change
        .get("diff")
        .and_then(Value::as_str)
        .map(|diff| match facts.operation {
            crate::acp::intent::FileChangeOperation::Created => patch::raw_content(diff, true),
            crate::acp::intent::FileChangeOperation::Deleted => patch::raw_content(diff, false),
            crate::acp::intent::FileChangeOperation::Updated => patch::decode(diff),
        });
    let mut pair = None;
    let mut admitted = None;
    let mut truncated = matches!(decoded, Some(Err(patch::PatchError::Oversized)));
    if let Some(Ok(patch)) = decoded {
        facts.additions = Some(patch.additions);
        facts.deletions = Some(patch.deletions);
        if retain_pair
            && item.get("status").and_then(Value::as_str) == Some("completed")
            && let SafeResourceLocation::Resource { resource } = &facts.resource
        {
            let context = patch
                .needs_context()
                .then(|| mapper.read_diff_context(resource))
                .flatten();
            match patch.saved_sides(context.as_deref()) {
                Ok((old_text, new_text)) => {
                    if let PairAdmission::Admitted {
                        old_bytes,
                        new_bytes,
                        digest,
                    } = admit_pair(&old_text, &new_text)
                    {
                        admitted = Some((old_bytes as u64, new_bytes as u64, digest));
                        pair = Some(FilePair { old_text, new_text });
                    } else {
                        truncated = true;
                    }
                }
                Err(patch::PatchError::Oversized) => truncated = true,
                Err(patch::PatchError::Invalid) => {}
            }
        }
    }
    let payload = match admitted {
        Some((old_bytes, new_bytes, digest)) => {
            NormalizedPayload::file_admitted(facts, 0, (old_bytes, new_bytes), digest)
        }
        None => NormalizedPayload::file_summary_only(facts, truncated),
    };
    Ok(((payload, None, refused_lifecycle(item)), pair))
}

/// A declined or failed change settles its diff in that state; any other
/// status leaves the notification's own lifecycle.
fn refused_lifecycle(item: &Map<String, Value>) -> Option<ItemLifecycle> {
    match lenient_tool_state(item, ItemLifecycle::Started) {
        state @ (ToolState::Declined | ToolState::Failed) => terminal_lifecycle(state),
        _ => None,
    }
}

/// The single change's resource and `Edited <path>` summary, before any line
/// counts; `None` when the item spans several resources.
fn describe<'a, M: ResourceMapper>(
    item: &'a Map<String, Value>,
    mapper: &M,
) -> Result<Option<DescribedChange<'a>>, IngressError> {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if changes.len() != 1 {
        return Ok(None);
    }
    let change = changes[0].as_object().ok_or(IngressError::Malformed)?;
    let absolute = text(
        change,
        "path",
        LIMITS.wire.absolute_path_bytes_native_only as usize,
    )?;
    let operation = match change
        .get("kind")
        .and_then(Value::as_object)
        .and_then(|kind| kind.get("type"))
        .and_then(Value::as_str)
    {
        Some("add") => crate::acp::intent::FileChangeOperation::Created,
        Some("delete") => crate::acp::intent::FileChangeOperation::Deleted,
        _ => crate::acp::intent::FileChangeOperation::Updated,
    };
    let resource = match mapper.map_existing(absolute) {
        Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
        Err(crate::acp::intent::UnavailableResourceReason::Missing)
            if operation == crate::acp::intent::FileChangeOperation::Deleted =>
        {
            match mapper.map_missing_display(absolute) {
                Some(display) => {
                    crate::acp::intent::SafeResourceLocation::unavailable_with_display(
                        crate::acp::intent::UnavailableResourceReason::Missing,
                        display,
                    )
                }
                None => crate::acp::intent::SafeResourceLocation::unavailable(
                    crate::acp::intent::UnavailableResourceReason::Missing,
                ),
            }
        }
        Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
    };
    let verb = match operation {
        crate::acp::intent::FileChangeOperation::Created => "Created",
        crate::acp::intent::FileChangeOperation::Updated => "Edited",
        crate::acp::intent::FileChangeOperation::Deleted => "Deleted",
    };
    let display = match &resource {
        crate::acp::intent::SafeResourceLocation::Resource { resource }
            if resource.relative_path.as_str().len() <= 480
                && !resource
                    .relative_path
                    .as_str()
                    .chars()
                    .any(char::is_control) =>
        {
            resource.relative_path.as_str()
        }
        _ => "file",
    };
    let summary = SafeText::new("fileSummary", format!("{verb} {display}")).map_err(content)?;
    Ok(Some((
        change,
        FileChangeFacts {
            resource,
            summary,
            additions: None,
            deletions: None,
            operation,
        },
    )))
}
