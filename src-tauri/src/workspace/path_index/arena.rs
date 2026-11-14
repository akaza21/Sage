//! An arena is filled by one walk and never changes after; a refresh builds
//! a new one.

use std::ops::Range;

/// Paths longer than this are left out of the index; Quick Open can't show
/// them usefully and the offsets stay small.
pub(super) const MAX_INDEXED_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy)]
struct Entry {
    start: u32,
    len: u16,
    base: u16,
    ascii: bool,
}

pub(super) struct Stored<'a> {
    pub(super) path: &'a str,
    pub(super) base: usize,
    pub(super) ascii: bool,
}

#[derive(Default)]
pub(super) struct PathArena {
    text: String,
    entries: Vec<Entry>,
}

impl PathArena {
    /// False when `path` is too long or the text would outgrow 32-bit offsets.
    pub(super) fn push(&mut self, path: &str) -> bool {
        if path.is_empty()
            || path.len() > MAX_INDEXED_PATH_BYTES
            || self.text.len() + path.len() > u32::MAX as usize
        {
            return false;
        }
        let base = path.rfind('/').map_or(0, |slash| slash + 1);
        self.entries.push(Entry {
            start: self.text.len() as u32,
            len: path.len() as u16,
            base: base as u16,
            ascii: path.is_ascii(),
        });
        self.text.push_str(path);
        true
    }

    pub(super) fn with_capacity(paths: usize, bytes: usize) -> Self {
        Self {
            text: String::with_capacity(bytes),
            entries: Vec::with_capacity(paths),
        }
    }

    pub(super) fn trim_slack(&mut self) {
        if self.text.capacity() > self.text.len() + self.text.len() / 8 {
            self.text.shrink_to_fit();
        }
        if self.entries.capacity() > self.entries.len() + self.entries.len() / 8 {
            self.entries.shrink_to_fit();
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn text_len(&self) -> usize {
        self.text.len()
    }

    pub(super) fn range(&self, slots: Range<usize>) -> impl Iterator<Item = Stored<'_>> {
        self.entries[slots].iter().map(|entry| {
            let start = entry.start as usize;
            Stored {
                path: &self.text[start..start + entry.len as usize],
                base: entry.base as usize,
                ascii: entry.ascii,
            }
        })
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.text.capacity() + self.entries.capacity() * std::mem::size_of::<Entry>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_paths_with_basename_offsets_and_trims_slack() {
        let mut whole = PathArena::with_capacity(64, 4096);
        assert!(whole.push("src/app/main.ts"));
        assert!(!whole.push(""));
        assert!(!whole.push(&"a".repeat(MAX_INDEXED_PATH_BYTES + 1)));
        assert!(whole.push("README.md"));
        assert!(whole.push("docs/étude.md"));
        whole.trim_slack();
        let stored = whole
            .range(0..whole.len())
            .map(|stored| (stored.path, stored.base, stored.ascii))
            .collect::<Vec<_>>();
        assert_eq!(
            stored,
            [
                ("src/app/main.ts", 8, true),
                ("README.md", 0, true),
                ("docs/étude.md", 5, false),
            ]
        );
        let text_bytes = "src/app/main.tsREADME.mddocs/étude.md".len();
        assert_eq!(
            whole.heap_bytes(),
            text_bytes + 3 * std::mem::size_of::<Entry>()
        );
    }
}
