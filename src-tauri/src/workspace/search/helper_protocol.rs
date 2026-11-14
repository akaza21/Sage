use std::{
    collections::HashSet,
    io::{Read, Write},
    path::PathBuf,
};

use sage_core::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

use crate::workspace::Workspace;

use super::{
    engine::SearchEngine,
    types::{
        MAX_PROJECT_ID_BYTES, MAX_PROJECT_ROOTS, SearchLimits, SearchRoot, SearchRootSource,
        WorkspaceSearchRequest, WorkspaceSearchResponse, validate_search_request,
    },
};

pub(super) const HELPER_ARGUMENT: &str = "--sage-workspace-search-helper-v1";
pub(super) const MAX_HELPER_INPUT_BYTES: u64 = 256 * 1024;
pub(super) const MAX_HELPER_OUTPUT_BYTES: usize = 12 * 1024 * 1024;
const HELPER_VERSION: u8 = 1;
const MAX_ROOT_NAME_BYTES: usize = 512;
const MAX_ABSOLUTE_PATH_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HelperRequest {
    version: u8,
    pub(super) request: WorkspaceSearchRequest,
    pub(super) roots: Vec<HelperRoot>,
    /// Git's global excludes file, resolved by the app because the helper
    /// runs with an empty environment.
    #[serde(default)]
    pub(super) global_excludes: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HelperRoot {
    pub(super) root_id: String,
    pub(super) root_name: String,
    pub(super) project_id: String,
    pub(super) absolute_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperReply {
    response: Option<WorkspaceSearchResponse>,
    error: Option<String>,
}

impl HelperRequest {
    pub(super) fn new(
        request: WorkspaceSearchRequest,
        roots: Vec<HelperRoot>,
        global_excludes: Option<PathBuf>,
    ) -> Self {
        Self {
            version: HELPER_VERSION,
            request,
            roots,
            global_excludes,
        }
    }
}

pub(super) fn execute(request: HelperRequest) -> AppResult<WorkspaceSearchResponse> {
    validate_helper_request(&request)?;
    let roots = request
        .roots
        .into_iter()
        .map(|root| {
            let source = match Workspace::open(&root.project_id, &root.root_id, &root.absolute_path)
            {
                Ok(workspace) => SearchRootSource::Available(workspace),
                Err(error) => SearchRootSource::Unavailable(error.to_string()),
            };
            SearchRoot {
                root_id: root.root_id,
                root_name: root.root_name,
                source,
            }
        })
        .collect();
    SearchEngine::new(SearchLimits::default())
        .with_global_excludes(request.global_excludes)
        .search(request.request, roots)
}

fn validate_helper_request(helper: &HelperRequest) -> AppResult<()> {
    if helper.version != HELPER_VERSION {
        return Err(AppError::InvalidRequest(
            "workspace search helper protocol version is invalid".into(),
        ));
    }
    validate_search_request(&helper.request)?;
    if helper.roots.is_empty() || helper.roots.len() > MAX_PROJECT_ROOTS {
        return Err(AppError::ResourceLimit(
            "workspace search root snapshot exceeds its bound".into(),
        ));
    }
    if helper.global_excludes.as_ref().is_some_and(|path| {
        !path.is_absolute() || path.to_string_lossy().len() > MAX_ABSOLUTE_PATH_BYTES
    }) {
        return Err(AppError::InvalidRequest(
            "workspace search helper excludes path is invalid".into(),
        ));
    }
    let mut root_ids = HashSet::with_capacity(helper.roots.len());
    for root in &helper.roots {
        let path_bytes = root.absolute_path.to_string_lossy().len();
        if root.project_id != helper.request.project_id
            || root.project_id.len() > MAX_PROJECT_ID_BYTES
            || root.root_id.is_empty()
            || root.root_id.len() > MAX_PROJECT_ID_BYTES
            || root.root_id.contains('\0')
            || !root_ids.insert(&root.root_id)
            || root.root_name.is_empty()
            || root.root_name.len() > MAX_ROOT_NAME_BYTES
            || root.root_name.contains('\0')
            || path_bytes == 0
            || path_bytes > MAX_ABSOLUTE_PATH_BYTES
        {
            return Err(AppError::InvalidRequest(
                "workspace search helper root snapshot is invalid".into(),
            ));
        }
    }
    Ok(())
}

pub fn run_stdio_helper_if_requested() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 1 || arguments[0] != HELPER_ARGUMENT {
        return;
    }

    let result = (|| -> AppResult<WorkspaceSearchResponse> {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(MAX_HELPER_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_HELPER_INPUT_BYTES {
            return Err(AppError::ResourceLimit(
                "workspace search helper input exceeded its bound".into(),
            ));
        }
        let request = serde_json::from_slice::<HelperRequest>(&bytes).map_err(|_| {
            AppError::InvalidRequest("workspace search helper input is invalid".into())
        })?;
        execute(request)
    })();
    let reply = match result {
        Ok(response) => HelperReply {
            response: Some(response),
            error: None,
        },
        Err(error) => HelperReply {
            response: None,
            error: Some(error.to_string()),
        },
    };
    let output = serde_json::to_vec(&reply).unwrap_or_else(|_| {
        br#"{"response":null,"error":"workspace search helper output failed"}"#.to_vec()
    });
    if output.len() <= MAX_HELPER_OUTPUT_BYTES {
        let _ = std::io::stdout().write_all(&output);
    }
    std::process::exit(0);
}

pub(super) fn decode_reply(bytes: &[u8]) -> AppResult<WorkspaceSearchResponse> {
    let reply = serde_json::from_slice::<HelperReply>(bytes).map_err(|_| {
        AppError::InvalidRequest("workspace search helper returned invalid output".into())
    })?;
    match (reply.response, reply.error) {
        (Some(response), None) => Ok(response),
        (None, Some(error)) if !error.is_empty() && error.len() <= 8 * 1024 => {
            Err(AppError::InvalidRequest(error))
        }
        _ => Err(AppError::InvalidRequest(
            "workspace search helper returned a contradictory result".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::workspace::search::tests::request;

    #[test]
    fn missing_roots_stay_local_to_their_reports() {
        let missing = std::env::temp_dir().join(format!("sage-helper-missing-{}", Uuid::new_v4()));
        let available = std::env::temp_dir().join(format!("sage-helper-kept-{}", Uuid::new_v4()));
        fs::create_dir_all(&available).unwrap();
        fs::write(available.join("kept.txt"), "needle in available root\n").unwrap();
        let roots = vec![
            HelperRoot {
                root_id: "missing".into(),
                root_name: "Missing".into(),
                project_id: "project-test".into(),
                absolute_path: missing,
            },
            HelperRoot {
                root_id: "kept".into(),
                root_name: "Kept".into(),
                project_id: "project-test".into(),
                absolute_path: available.clone(),
            },
        ];
        let response = execute(HelperRequest::new(request("needle"), roots, None)).unwrap();
        assert_eq!(response.matches.len(), 1);
        assert_eq!(
            response.roots[0].state,
            super::super::types::SearchRootState::Unavailable
        );
        assert_eq!(
            response.roots[1].state,
            super::super::types::SearchRootState::Complete
        );
        fs::remove_dir_all(available).unwrap();
    }
}
