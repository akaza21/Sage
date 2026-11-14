use std::io::Read;

use super::*;
use crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX;

impl NativeResourceMapper {
    pub(super) fn bounded_diff_context(&self, resource: &SafeResourceRef) -> Option<String> {
        let root = self
            .roots
            .iter()
            .find(|root| root.root_id == resource.root_id)?;
        let file = open_workspace_text_file(
            &root.native_root,
            root.identity,
            resource.relative_path.as_str(),
            None,
        )
        .ok()?;
        let before = file.metadata().ok()?;
        if !before.is_file() || before.len() > DIFF_SIDE_BYTES_MAX as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        (&file)
            .take(DIFF_SIDE_BYTES_MAX as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let after = file.metadata().ok()?;
        if bytes.len() > DIFF_SIDE_BYTES_MAX
            || bytes.contains(&0)
            || before.len() != after.len()
            || before.modified().ok()? != after.modified().ok()?
        {
            return None;
        }
        String::from_utf8(bytes).ok()
    }
}
