//! The rewind dialog's view of a file preview.
use serde::Serialize;

use super::FileRewindPreview;

const CHANGED_NOTE: &str = "Files changed since the agent last wrote them, so they stay:";
const UNVERIFIED_NOTE: &str =
    "Sage can't verify these files in this chat, so they stay as they are:";
const RESTORE_NOTE: &str = "These files go back to before this message:";
const CAVEAT: &str = "Rewind does not undo shell commands, commits, branch switches, or files \
                      changed outside checkpoints.";

/// One listed section of the dialog: files a rewind keeps, with why, or the
/// files it would restore.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Section<'a> {
    note: &'static str,
    paths: &'a [String],
    count: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    keeps: bool,
}

/// The dialog's view: the kept files with why, or else the files a rewind
/// would restore with what it does not undo. `caveat` is present exactly when
/// files can be restored.
impl Serialize for FileRewindPreview {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut sections: Vec<Section<'_>> = [
            (CHANGED_NOTE, &self.conflicts, self.conflict_count),
            (UNVERIFIED_NOTE, &self.unverified, self.unverified_count),
        ]
        .into_iter()
        .filter(|(_, _, count)| *count > 0)
        .map(|(note, paths, count)| Section {
            note,
            paths,
            count,
            keeps: true,
        })
        .collect();
        let restores = sections.is_empty() && self.file_count > 0;
        if restores {
            sections.push(Section {
                note: RESTORE_NOTE,
                paths: &self.files,
                count: self.file_count,
                keeps: false,
            });
        }
        let mut state = serializer.serialize_struct("FileRewindPreview", 3)?;
        state.serialize_field("fileCount", &self.file_count)?;
        state.serialize_field("sections", &sections)?;
        state.serialize_field("caveat", &restores.then_some(CAVEAT))?;
        state.end()
    }
}
