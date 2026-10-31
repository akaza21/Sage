use std::io::Read;

use crate::{domain::ResourceRef, workspace::Workspace};

use super::{
    contained_file::{ContainedFile, open_contained_file},
    engine::{RootCounters, SearchBudget, SearchEngine},
    literal_match::{file_may_match, literal_matches},
    types::{SearchLimitReason, WorkspaceSearchMatch, push_reason},
};

/// A NUL byte this early marks a File as binary, as in VS Code and Git.
const BINARY_SNIFF_BYTES: u64 = 8_192;

/// What one Search query asks of every File.
#[derive(Clone, Copy)]
pub(super) struct ScanQuery<'a> {
    pub(super) text: &'a str,
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
}

/// Reading and matching one File, which depends on nothing but the File, so
/// it can run ahead on another thread. `apply_scan` then charges it to the
/// budget in walk order, exactly as if it had just run.
pub(super) enum FileScan {
    Unopened,
    Opened { byte_len: u64, body: FileBody },
}

pub(super) enum FileBody {
    /// Over the per-file bound or the bytes still left, so never read.
    NotRead,
    Unreadable,
    Read {
        len: u64,
        text: TextScan,
    },
}

pub(super) enum TextScan {
    Binary,
    NotUtf8,
    Text(Vec<WorkspaceSearchMatch>),
}

impl FileBody {
    pub(super) fn match_count(&self) -> usize {
        match self {
            Self::Read {
                text: TextScan::Text(found),
                ..
            } => found.len(),
            _ => 0,
        }
    }
}

impl SearchEngine {
    /// `bytes_left` lets a scan that runs in walk order skip a read the
    /// budget would refuse; a scan run ahead passes `u64::MAX`.
    pub(super) fn scan_file(
        &self,
        workspace: &Workspace,
        relative_path: &str,
        query: ScanQuery<'_>,
        bytes_left: u64,
    ) -> FileScan {
        let resource = workspace.resource_unchecked(relative_path);
        let ContainedFile { file, byte_len } = match open_contained_file(workspace, &resource) {
            Ok(contained) => contained,
            Err(_) => return FileScan::Unopened,
        };
        if byte_len > self.limits.max_file_bytes || byte_len > bytes_left {
            return FileScan::Opened {
                byte_len,
                body: FileBody::NotRead,
            };
        }
        let mut bytes = Vec::with_capacity(byte_len as usize);
        let mut file = file.take(byte_len);
        // The head settles whether a File is binary, so a binary File is not
        // read further. It is still charged its whole length, as when it was.
        let head = (&mut file)
            .take(BINARY_SNIFF_BYTES.min(byte_len))
            .read_to_end(&mut bytes);
        if head.is_ok() && bytes.contains(&0) {
            return FileScan::Opened {
                byte_len,
                body: FileBody::Read {
                    len: byte_len,
                    text: TextScan::Binary,
                },
            };
        }
        if head.is_err() || file.read_to_end(&mut bytes).is_err() {
            return FileScan::Opened {
                byte_len,
                body: FileBody::Unreadable,
            };
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(content) => TextScan::Text(self.collect_matches(resource, content, query)),
            Err(_) => TextScan::NotUtf8,
        };
        FileScan::Opened {
            byte_len,
            body: FileBody::Read {
                len: bytes.len() as u64,
                text,
            },
        }
    }

    pub(super) fn apply_scan(
        &self,
        scan: FileScan,
        matches: &mut Vec<WorkspaceSearchMatch>,
        budget: &mut SearchBudget,
        counters: &mut RootCounters,
    ) {
        let (byte_len, body) = match scan {
            FileScan::Unopened => {
                counters.error_count += 1;
                counters.partial = true;
                return;
            }
            FileScan::Opened { byte_len, body } => (byte_len, body),
        };
        if byte_len > self.limits.max_file_bytes {
            counters.skipped_files += 1;
            counters.partial = true;
            push_reason(&mut budget.reasons, SearchLimitReason::FileSize);
            return;
        }
        if budget.bytes_scanned.saturating_add(byte_len) > self.limits.max_total_bytes {
            push_reason(&mut budget.reasons, SearchLimitReason::Bytes);
            counters.partial = true;
            return;
        }
        let (len, text) = match body {
            FileBody::Read { len, text } => (len, text),
            // `NotRead` cannot reach here: both of its causes returned above.
            FileBody::NotRead | FileBody::Unreadable => {
                counters.error_count += 1;
                counters.partial = true;
                return;
            }
        };
        budget.bytes_scanned += len;
        counters.bytes_scanned += len;
        counters.files_scanned += 1;
        match text {
            // Binary files are skipped like symlinks: counted, but the search
            // stays complete, as in VS Code. Text that is not UTF-8 was not
            // searched, so it still marks the root partial.
            TextScan::Binary => counters.skipped_files += 1,
            TextScan::NotUtf8 => {
                counters.skipped_files += 1;
                counters.partial = true;
            }
            TextScan::Text(found) => {
                for found in found {
                    matches.push(found);
                    if matches.len() == self.limits.max_results {
                        push_reason(&mut budget.reasons, SearchLimitReason::Results);
                        counters.partial = true;
                        return;
                    }
                }
            }
        }
    }

    /// One File's matches, at most as many as a page holds.
    fn collect_matches(
        &self,
        resource: ResourceRef,
        content: &str,
        query: ScanQuery<'_>,
    ) -> Vec<WorkspaceSearchMatch> {
        let mut matches = Vec::new();
        // Most Files hold no match: one pass over the bytes settles that
        // before any line is split out.
        if !file_may_match(content, query.text, query.case_sensitive) {
            return matches;
        }
        for (line_index, raw_line) in content.split('\n').enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            for (start, _) in
                literal_matches(line, query.text, query.case_sensitive, query.whole_word)
            {
                matches.push(WorkspaceSearchMatch {
                    resource: resource.clone(),
                    line_number: one_based(line_index),
                    column_number: column_number(line, start),
                    preview: bounded_preview(line, self.limits.max_preview_chars),
                });
                if matches.len() == self.limits.max_results {
                    return matches;
                }
            }
        }
        matches
    }
}

fn bounded_preview(line: &str, max_chars: usize) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.into();
    }
    let mut preview = trimmed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    preview.push('…');
    preview
}

fn one_based(zero_based: usize) -> u32 {
    u32::try_from(zero_based.saturating_add(1)).unwrap_or(u32::MAX)
}

fn column_number(line: &str, byte_offset: usize) -> u32 {
    let characters = line[..byte_offset].chars().count().saturating_add(1);
    u32::try_from(characters).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::{FileBody, FileScan, ScanQuery, TextScan};
    use crate::workspace::{
        Workspace,
        search::{engine::SearchEngine, types::SearchLimits},
    };

    fn scan(contents: &[u8]) -> FileScan {
        let root = std::env::temp_dir().join(format!("sage-search-scan-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("file.bin"), contents).unwrap();
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let query = ScanQuery {
            text: "needle",
            case_sensitive: false,
            whole_word: false,
        };
        let scan = SearchEngine::new(SearchLimits::default()).scan_file(
            &workspace,
            "file.bin",
            query,
            u64::MAX,
        );
        fs::remove_dir_all(root).unwrap();
        scan
    }

    #[test]
    fn a_nul_in_the_first_8_kib_marks_a_file_binary_and_charges_all_of_it() {
        let mut contents = vec![b'a'; 100_000];
        contents[8_191] = 0;
        let FileScan::Opened {
            byte_len: 100_000,
            body:
                FileBody::Read {
                    len: 100_000,
                    text: TextScan::Binary,
                },
        } = scan(&contents)
        else {
            panic!("expected a binary File charged its whole length");
        };
    }

    #[test]
    fn a_nul_after_the_first_8_kib_is_searched_as_text() {
        let mut contents = b"needle\n".repeat(2_000);
        contents[8_192] = 0;
        let FileScan::Opened {
            body:
                FileBody::Read {
                    text: TextScan::Text(found),
                    ..
                },
            ..
        } = scan(&contents)
        else {
            panic!("expected a text File");
        };
        assert_eq!(found.len(), 500);
    }
}
