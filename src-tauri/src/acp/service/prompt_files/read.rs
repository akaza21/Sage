use super::{Resource, Resources, picked_images::PickedImage};
use crate::acp::prompt_files::secrets;
use sage_acp_protocol::intent::ImageMediaType;
use sage_core::limits::prompt_files::{FILE_BYTES, FILE_COUNT, TOTAL_BYTES};
use sage_core::limits::{IMAGE_UPLOAD_BYTES, PROMPT_IMAGE_COUNT};
use sage_workspace::ignore_rules::{FolderChain, IgnoreRules, RuleFiles, global_excludes_path};
use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

const DEPTH: usize = 64;
const ENTRIES: usize = 200_000;
/// Fails closed when the shared matcher skipped any rule source it resolved
/// (the global excludes file, `.ignore`, `.gitignore`, or the `info/exclude`
/// a `.git` file and `commondir` lead to): an entry that source would have
/// excluded must never be attached.
fn honoured(rules: &IgnoreRules) -> Result<(), &'static str> {
    if rules.every_source_honoured() {
        Ok(())
    } else {
        Err("Attachment ignore rules could not be applied in full.")
    }
}
struct Reader {
    files: Vec<Resource>,
    /// Top-level PNG and JPEG picks, kept as open files to read one at a
    /// time later; `None` when the harness takes no images, so they stay files.
    images: Option<Vec<PickedImage>>,
    seen: BTreeSet<PathBuf>,
    bytes: usize,
    visited: usize,
}
#[cfg(test)]
pub(super) fn read_paths(paths: &[String]) -> Result<Resources, &'static str> {
    read_picked(paths, false).map(|picked| picked.files.unwrap_or_default())
}
/// What one native pick holds: files to stage and, when images were asked
/// for, the picked top-level images.
pub(super) struct Picked {
    pub files: Option<Resources>,
    pub images: Vec<PickedImage>,
}
impl From<Resources> for Picked {
    fn from(files: Resources) -> Self {
        Self {
            files: Some(files),
            images: Vec::new(),
        }
    }
}
pub(super) fn read_picked(paths: &[String], images: bool) -> Result<Picked, &'static str> {
    if paths.is_empty() || paths.len() > FILE_COUNT || paths.iter().any(|p| p.len() > 4096) {
        return Err("Pick between 1 and 16 paths, each at most 4096 bytes.");
    }
    let mut reader = Reader {
        files: Vec::new(),
        images: images.then(Vec::new),
        seen: BTreeSet::new(),
        bytes: 0,
        visited: 0,
    };
    for path in paths {
        let selected = Path::new(path);
        if !selected.is_absolute() || selected.components().count() > DEPTH {
            return Err("Attachment paths must be absolute and at most 64 components deep.");
        }
        if secrets::path_is_secret(selected, selected.is_dir()) {
            return Err("Credential files and folders cannot be attached.");
        }
        // Reject symbolic-link ancestors too, so a selection cannot escape via
        // a linked directory. The final file is also opened no-follow on Unix.
        for ancestor in selected.ancestors() {
            let meta = ancestor
                .symlink_metadata()
                .map_err(|_| "A selected path is not readable.")?;
            if meta.file_type().is_symlink() {
                return Err("Symbolic links cannot be attached.");
            }
        }
        let parent = selected
            .parent()
            .ok_or("Cannot attach a filesystem root.")?;
        let (root, rules, chain, ignored) = scope(parent)?;
        if ignored || !reader.visit(selected, root, &rules, &chain, 0)? {
            return Err("The selected path is excluded by ignore rules.");
        }
    }
    let images = reader.images.unwrap_or_default();
    if reader.files.is_empty() && images.is_empty() {
        return Err("No readable, non-ignored files were selected.");
    }
    Ok(Picked {
        files: (!reader.files.is_empty()).then(|| Resources(Arc::new(reader.files))),
        images,
    })
}
/// The ignore rules over the entries of `parent`, rooted at its nearest
/// enclosing repository (or `parent` itself), and whether `parent` is itself
/// ignored. Fails closed when any rule source could not be honoured.
fn scope(parent: &Path) -> Result<(&Path, IgnoreRules, FolderChain, bool), &'static str> {
    let root = parent
        .ancestors()
        .find(|p| p.join(".git").exists())
        .unwrap_or(parent);
    let rules = IgnoreRules::load(root, true, global_excludes_path().as_deref());
    let relative_parent = relative(root, parent)?;
    let (chain, ignored) = rules.chain_for(&relative_parent, RuleFiles::probe(parent));
    honoured(&rules)?;
    Ok((root, rules, chain, ignored))
}
/// Errors when the rules cannot be applied in full.
pub(crate) fn file_is_excluded(path: &Path) -> Result<bool, &'static str> {
    let parent = path.parent().ok_or("Cannot attach a filesystem root.")?;
    let (root, rules, chain, ignored) = scope(parent)?;
    Ok(ignored || always_skipped(path) || rules.is_ignored(&chain, &relative(root, path)?, false))
}
fn always_skipped(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c.as_os_str().to_str(), Some(".git" | "node_modules")))
}
fn relative(root: &Path, path: &Path) -> Result<String, &'static str> {
    let value = path
        .strip_prefix(root)
        .map_err(|_| "Attachment left its selected root.")?
        .to_str()
        .ok_or("Attachment path is not UTF-8.")?;
    #[cfg(windows)]
    let value = value.replace('\\', "/");
    #[cfg(not(windows))]
    let value = value.to_owned();
    if value.len() > 4096 {
        return Err("Attachment path exceeds 4096 bytes.");
    }
    Ok(value)
}
impl Reader {
    fn visit(
        &mut self,
        path: &Path,
        root: &Path,
        rules: &IgnoreRules,
        chain: &FolderChain,
        depth: usize,
    ) -> Result<bool, &'static str> {
        self.visited += 1;
        if self.visited > ENTRIES || depth > DEPTH {
            return Err("Attachment walk exceeds 200000 entries or depth 64.");
        }
        let meta = path
            .symlink_metadata()
            .map_err(|_| "A selected path is not readable.")?;
        let rel = relative(root, path)?;
        if always_skipped(path)
            || meta.file_type().is_symlink()
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| secrets::is_secret(name, meta.is_dir()))
            || rules.is_ignored(chain, &rel, meta.is_dir())
        {
            return Ok(false);
        }
        if meta.is_dir() {
            let chain = rules.enter(chain, &rel, RuleFiles::probe(path));
            honoured(rules)?;
            // Depth-first streaming holds at most 65 directory iterators, never
            // collects a directory listing or an unbounded work queue.
            for entry in path
                .read_dir()
                .map_err(|_| "Cannot read attachment directory.")?
            {
                let entry = entry.map_err(|_| "Cannot read attachment directory entry.")?;
                self.visit(&entry.path(), root, rules, &chain, depth + 1)?;
            }
            return Ok(true);
        }
        if !meta.is_file() {
            return Err("Only regular files can be attached.");
        }
        if self.files.len() == FILE_COUNT {
            return Err("Attachments exceed the 16-file limit.");
        }
        if !self.seen.insert(
            path.canonicalize()
                .map_err(|_| "File is no longer readable.")?,
        ) {
            return Err("Selections contain the same file more than once.");
        }
        let identity = sage_workspace::root_identity_path(root)
            .map_err(|_| "Attachment root is unavailable.")?;
        let file = sage_workspace::open_workspace_text_file(root, identity, &rel, None)
            .map_err(|_| "Attachment file is unreadable or changed during selection.")?;
        let meta = file.metadata().map_err(|_| "Cannot inspect attachment.")?;
        if !meta.is_file() {
            return Err("Only regular files can be attached.");
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid attachment name.")?;
        if name.len() > 255
            || name
                .chars()
                .any(sage_acp_protocol::contract::has_unsafe_display_character)
        {
            return Err("Attachment name contains unsupported characters.");
        }
        // Only a picked file, never one inside a picked folder, is an image.
        if depth == 0
            && let Some(images) = self.images.as_mut()
            && let Some(media_type) = image_type(&file)?
        {
            if images.len() == PROMPT_IMAGE_COUNT {
                return Err(super::limits::IMAGE_OVER_COUNT.as_str());
            }
            if meta.len() > IMAGE_UPLOAD_BYTES as u64 {
                return Err(super::limits::IMAGE_OVER.as_str());
            }
            images.push(PickedImage::new(file, media_type, name));
            return Ok(true);
        }
        if meta.len() > FILE_BYTES as u64 {
            return Err(super::limits::FILE_OVER.as_str());
        }
        let mut data = Vec::new();
        file.take(FILE_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|_| "Cannot read attachment.")?;
        self.bytes += data.len();
        if data.len() > FILE_BYTES || self.bytes > TOTAL_BYTES {
            return Err(super::limits::READ_OVER.as_str());
        }
        data.shrink_to_fit();
        self.files.push(Resource {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_owned(),
            bytes: data,
        });
        Ok(true)
    }
}
/// PNG or JPEG by magic bytes; the file is rewound for its reader.
fn image_type(file: &std::fs::File) -> Result<Option<ImageMediaType>, &'static str> {
    let mut head = Vec::with_capacity(8);
    let mut reader = file;
    reader
        .take(8)
        .read_to_end(&mut head)
        .and_then(|_| reader.seek(SeekFrom::Start(0)))
        .map_err(|_| "Cannot read attachment.")?;
    Ok(if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageMediaType::Png)
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some(ImageMediaType::Jpeg)
    } else {
        None
    })
}
