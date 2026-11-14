use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::{
    acp::resource_mapper::{ConfiguredResourceRoot, NativeResourceMapper},
    project_registry::RootRecord,
    state::AppState,
};
use sage_acp_protocol::contract::SageRootId;
use sage_domain::resource::ResourceRef;

const MAX_REFERENCE_BYTES: usize = 4096;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveChatFileReferenceRequest {
    project_id: String,
    destination: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedChatFileReference {
    resource: ResourceRef,
    line_number: Option<u32>,
}

#[tauri::command]
pub async fn resolve_chat_file_reference(
    state: State<'_, AppState>,
    request: ResolveChatFileReferenceRequest,
) -> Result<Option<ResolvedChatFileReference>, String> {
    if request.project_id.is_empty() || request.project_id.len() > 512 {
        return Ok(None);
    }
    let _transition = state.project_transitions.lock().await;
    if state
        .database
        .active_project_id()
        .map_err(|error| error.to_string())?
        .as_deref()
        != Some(request.project_id.as_str())
    {
        return Ok(None);
    }
    let roots = state
        .database
        .root_records(&request.project_id)
        .map_err(|error| error.to_string())?;
    Ok(resolve_with_roots(&roots, &request.destination))
}

fn resolve_with_roots(
    roots: &[RootRecord],
    destination: &str,
) -> Option<ResolvedChatFileReference> {
    if destination.is_empty()
        || destination.len() > MAX_REFERENCE_BYTES
        || destination.chars().any(char::is_control)
        || destination.starts_with("\\\\")
        || destination.starts_with("//")
    {
        return None;
    }
    let (path, line_number) = split_line(destination)?;
    let supplied = Path::new(path);
    if !supplied.is_absolute()
        && (roots.len() != 1
            || path.contains('\\')
            || path.contains(':')
            || !supplied
                .components()
                .all(|component| matches!(component, Component::Normal(_))))
    {
        return None;
    }
    let configured = roots
        .iter()
        .map(|root| {
            Some(ConfiguredResourceRoot {
                root_id: SageRootId::parse(root.id.clone()).ok()?,
                canonical_native_root: root.absolute_path.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let mapper = NativeResourceMapper::new(configured).ok()?;
    let absolute = if supplied.is_absolute() {
        supplied.to_path_buf()
    } else {
        roots.first()?.absolute_path.join(supplied)
    };
    let resource = mapper.try_map_existing(absolute.to_str()?).ok()?;
    Some(ResolvedChatFileReference {
        resource: ResourceRef {
            root_id: resource.root_id.as_str().to_owned(),
            relative_path: resource.relative_path.as_str().to_owned(),
        },
        line_number,
    })
}

fn split_line(destination: &str) -> Option<(&str, Option<u32>)> {
    let Some((path, suffix)) = destination.rsplit_once(':') else {
        return Some((destination, None));
    };
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some((destination, None));
    }
    let line = suffix.parse::<u32>().ok().filter(|line| *line > 0)?;
    Some((path, Some(line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTree(std::path::PathBuf);

    impl TempTree {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("sage-chat-file-reference-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn relative_and_absolute_file_references_are_contained() {
        let temp = TempTree::new();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("README.md"), "hello").unwrap();
        let root = root.canonicalize().unwrap();
        let roots = [RootRecord {
            id: "root-a".into(),
            project_id: "project-a".into(),
            absolute_path: root.clone(),
            ordinal: 0,
        }];
        for destination in [
            "README.md".to_owned(),
            root.join("README.md")
                .to_str()
                .unwrap()
                .strip_prefix("\\\\?\\")
                .unwrap_or(root.join("README.md").to_str().unwrap())
                .to_owned(),
        ] {
            let resolved = resolve_with_roots(&roots, &format!("{destination}:12")).unwrap();
            assert_eq!(resolved.resource.relative_path, "README.md");
            assert_eq!(resolved.line_number, Some(12));
        }
        for destination in [
            "../README.md".to_owned(),
            temp.path()
                .join("outside.md")
                .to_string_lossy()
                .into_owned(),
            "\\\\server\\share\\file.md".to_owned(),
            "\\\\?\\C:\\file.md".to_owned(),
            "file:///README.md".to_owned(),
            "https://example.com/README.md".to_owned(),
            "x".repeat(MAX_REFERENCE_BYTES + 1),
        ] {
            assert!(
                resolve_with_roots(&roots, &destination).is_none(),
                "{destination}"
            );
        }
    }
}
