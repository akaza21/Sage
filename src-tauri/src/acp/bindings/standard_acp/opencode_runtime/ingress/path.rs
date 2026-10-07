use std::path::{Component, Path};

use serde_json::Value;

pub(super) fn workspace_edit_target(request: &Value, root: &Path) -> bool {
    let Some(tool) = request.pointer("/params/toolCall") else {
        return false;
    };
    if tool.get("kind").and_then(Value::as_str) != Some("edit") {
        return false;
    }
    let Some(input) = tool.get("rawInput").and_then(Value::as_object) else {
        return false;
    };
    // Only the observed single-file edit shape is auto-approved. Any extra input
    // that might name another target stays behind a user permission card.
    if input
        .keys()
        .any(|key| !matches!(key.as_str(), "filepath" | "diff"))
    {
        return false;
    }
    let Some(primary) = input.get("filepath").and_then(Value::as_str) else {
        return false;
    };
    if !path_inside(root, primary) {
        return false;
    }
    let Some(locations) = tool.get("locations").and_then(Value::as_array) else {
        return false;
    };
    if locations.is_empty()
        || locations.iter().any(|location| {
            location.as_object().is_none_or(|object| object.len() != 1)
                || location
                    .get("path")
                    .and_then(Value::as_str)
                    .is_none_or(|path| path != primary || !path_inside(root, path))
        })
    {
        return false;
    }
    if let Some(content) = tool.get("content") {
        let Some(content) = content.as_array() else {
            return false;
        };
        if content.iter().any(|part| {
            part.get("path")
                .and_then(Value::as_str)
                .is_none_or(|path| path != primary || !path_inside(root, path))
        }) {
            return false;
        }
    }
    if let Some(diff) = input.get("diff") {
        let Some(diff) = diff.as_str() else {
            return false;
        };
        // Admit only OpenCode's observed single-file patch envelope; any
        // additional or unrecognised path-bearing syntax needs a user card.
        let mut lines = diff.lines();
        if lines.next() != Some(format!("Index: {primary}").as_str())
            || !lines.next().is_some_and(|line| line.starts_with("===="))
            || !lines.next().is_some_and(|line| {
                line.strip_prefix("--- ")
                    .is_some_and(|path| path == primary || path == "/dev/null")
            })
            || !lines.next().is_some_and(|line| {
                line.strip_prefix("+++ ")
                    .is_some_and(|path| path == primary || path == "/dev/null")
            })
            || lines.any(|line| line.starts_with("Index: ") || line.starts_with("diff --git "))
        {
            return false;
        }
    }
    true
}

pub(super) fn workspace_edit_path(request: &Value, root: &Path) -> Option<std::path::PathBuf> {
    workspace_edit_target(request, root).then(|| {
        std::path::PathBuf::from(
            request["params"]["toolCall"]["rawInput"]["filepath"]
                .as_str()
                .expect("validated filepath"),
        )
    })
}

pub(super) fn path_inside(root: &Path, candidate: &str) -> bool {
    if candidate.starts_with("\\\\") || candidate.starts_with("//") {
        return false;
    }
    let path = Path::new(candidate);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return false;
    }
    let Some(Component::Normal(filename)) = path.components().next_back() else {
        return false;
    };
    let Some(filename) = filename.to_str() else {
        return false;
    };
    if !admissible_filename(filename) {
        return false;
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_reparse_point(&metadata) => return false,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
    }
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() || !has_single_hard_link(path, &metadata) => {
            return false;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return false,
    }
    let canonical = if path.exists() {
        path.canonicalize()
    } else {
        path.parent()
            .filter(|parent| std::fs::metadata(parent).is_ok_and(|metadata| metadata.is_dir()))
            .and_then(|parent| parent.canonicalize().ok())
            .map(|parent| parent.join(filename))
            .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))
    };
    canonical.is_ok_and(|target| target.starts_with(root) && target != root)
}

fn admissible_filename(filename: &str) -> bool {
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.ends_with(' ')
        || filename.ends_with('.')
        || filename.chars().any(|character| {
            matches!(
                character,
                '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
            ) || character.is_control()
        })
    {
        return false;
    }
    !windows_reserved_device_name(filename)
}

fn windows_reserved_device_name(filename: &str) -> bool {
    let stem = filename.split('.').next().unwrap_or(filename).trim_end();
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let suffix = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"));
    suffix.is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn has_single_hard_link(path: &Path, _metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return false;
    }
    information.nNumberOfLinks == 1
}

#[cfg(unix)]
fn has_single_hard_link(_path: &Path, metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() == 1
}
