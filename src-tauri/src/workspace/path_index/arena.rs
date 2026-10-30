//! One root's relative File paths, packed into a single string. Each entry
//! is an offset, a length, where its basename starts and whether it is all
//! ASCII, so 200k paths cost one allocation for the text plus 12 bytes each.

use std::ops::Range;

/// Paths longer than this are left out of the index; Quick Open can't show
/// them usefully and the offsets stay small.
pub(super) const MAX_INDEXED_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy)]
struct Entry {
    start: u32,
    len: u16,
    base: u16,
    live: bool,
    ascii: bool,
}

/// One stored path.
pub(super) struct Stored<'a> {
    pub(super) path: &'a str,
    /// Byte offset where the basename starts.
    pub(super) base: usize,
    pub(super) ascii: bool,
}

#[derive(Default)]
pub(super) struct PathArena {
    text: String,
    entries: Vec<Entry>,
    live: usize,
}

impl PathArena {
    pub(super) fn with_capacity(paths: usize, bytes: usize) -> Self {
        Self {
            text: String::with_capacity(bytes),
            entries: Vec::with_capacity(paths),
            live: 0,
        }
    }

    /// Adds `path`, a relative path with `/` separators. Returns false when
    /// it is too long or the text would outgrow 32-bit offsets.
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
            live: true,
            ascii: path.is_ascii(),
        });
        self.text.push_str(path);
        self.live += 1;
        true
    }

    /// Makes room for `paths` more paths of `bytes` in total, exactly, so a
    /// full build doesn't leave growth slack behind.
    pub(super) fn reserve(&mut self, paths: usize, bytes: usize) {
        self.text.reserve_exact(bytes);
        self.entries.reserve_exact(paths);
    }

    pub(super) fn live(&self) -> usize {
        self.live
    }

    /// Slots, live or removed; `range` takes a share of them.
    pub(super) fn slots(&self) -> usize {
        self.entries.len()
    }

    /// The live paths among `slots`.
    pub(super) fn range(&self, slots: Range<usize>) -> impl Iterator<Item = Stored<'_>> {
        self.entries[slots]
            .iter()
            .filter(|entry| entry.live)
            .map(|entry| {
                let start = entry.start as usize;
                Stored {
                    path: &self.text[start..start + entry.len as usize],
                    base: entry.base as usize,
                    ascii: entry.ascii,
                }
            })
    }

    /// Live paths with their basename offsets.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, usize)> {
        self.range(0..self.slots())
            .map(|stored| (stored.path, stored.base))
    }

    #[cfg(test)]
    pub(super) fn contains(&self, path: &str) -> bool {
        self.iter().any(|(candidate, _)| candidate == path)
    }

    /// Removes `path` and everything below it, in one pass however many
    /// prefixes are given. Returns how many entries were removed.
    pub(super) fn remove_under(&mut self, prefixes: &[&str]) -> usize {
        let mut removed = 0;
        for entry in &mut self.entries {
            if !entry.live {
                continue;
            }
            let start = entry.start as usize;
            let path = &self.text[start..start + entry.len as usize];
            if prefixes.iter().any(|prefix| covers(prefix, path)) {
                entry.live = false;
                removed += 1;
            }
        }
        self.live -= removed;
        if self.entries.len() > 1024 && self.live < self.entries.len() / 2 {
            self.compact();
        }
        removed
    }

    /// Bytes held, for the memory budget.
    pub(super) fn heap_bytes(&self) -> usize {
        self.text.capacity() + self.entries.capacity() * std::mem::size_of::<Entry>()
    }

    fn compact(&mut self) {
        let mut next = Self::with_capacity(self.live, self.text.len());
        for (path, _) in self.iter() {
            next.push(path);
        }
        *self = next;
    }
}

/// Whether `prefix` names `path` itself or a folder above it.
fn covers(prefix: &str, path: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_paths_with_basename_offsets() {
        let mut arena = PathArena::default();
        assert!(arena.push("src/app/main.ts"));
        assert!(arena.push("README.md"));
        assert!(!arena.push(""));
        assert!(!arena.push(&"a".repeat(MAX_INDEXED_PATH_BYTES + 1)));
        let entries = arena.iter().collect::<Vec<_>>();
        assert_eq!(entries, [("src/app/main.ts", 8), ("README.md", 0)]);
        assert!(arena.contains("README.md"));
        assert!(!arena.contains("src/app"));
    }

    #[test]
    fn removes_a_path_and_everything_below_a_folder() {
        let mut arena = PathArena::default();
        for path in ["src/a.ts", "src/ab.ts", "src/lib/b.ts", "srcx/c.ts", "d.ts"] {
            arena.push(path);
        }
        assert_eq!(arena.remove_under(&["src/lib", "d.ts", "src/a"]), 2);
        let left = arena.iter().map(|(path, _)| path).collect::<Vec<_>>();
        assert_eq!(left, ["src/a.ts", "src/ab.ts", "srcx/c.ts"]);
        assert_eq!(arena.live(), 3);
    }

    #[test]
    fn compacts_once_most_entries_are_removed() {
        let mut arena = PathArena::default();
        for index in 0..2000 {
            arena.push(&format!("dir{}/file{index}.rs", index % 2));
        }
        assert_eq!(arena.remove_under(&["dir0"]), 1000);
        assert_eq!(arena.remove_under(&["dir1/file1.rs"]), 1);
        assert_eq!(arena.entries.len(), 999);
        assert_eq!(arena.live(), 999);
        assert!(arena.contains("dir1/file3.rs"));
    }
}
