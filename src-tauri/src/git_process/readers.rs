use std::time::Duration;

use tokio::{io::AsyncReadExt, process::ChildStdout, task::JoinHandle, time::timeout};

use super::{BoundedBytes, read_bounded};

pub(super) type ReaderTask = JoinHandle<std::io::Result<BoundedBytes>>;

/// How Git's stdout is retained.
#[derive(Clone, Copy)]
pub(super) enum StdoutCapture {
    /// The first bytes, up to this many.
    Bytes(usize),
    /// Only the NUL-terminated records a filter keeps, read as they arrive.
    Records(RecordFilter),
}

/// Streams NUL-terminated records, retaining only those `keep` accepts, so
/// memory stays bounded by the kept records however long the output is.
/// Past any limit the output is dropped and marked truncated, and the rest
/// is drained unread, so Git still exits normally.
#[derive(Clone, Copy)]
pub(super) struct RecordFilter {
    pub(super) keep: fn(&[u8]) -> bool,
    pub(super) max_kept: usize,
    pub(super) max_records: usize,
    pub(super) max_record_bytes: usize,
}

pub(super) fn spawn_stdout(stdout: ChildStdout, capture: StdoutCapture) -> ReaderTask {
    match capture {
        StdoutCapture::Bytes(limit) => tokio::spawn(read_bounded(stdout, limit)),
        StdoutCapture::Records(filter) => tokio::spawn(read_records(stdout, filter)),
    }
}

async fn read_records(
    mut stdout: ChildStdout,
    filter: RecordFilter,
) -> std::io::Result<BoundedBytes> {
    let mut records = RecordReader::new(filter);
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = stdout.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        records.push(&buffer[..read]);
    }
    Ok(records.finish())
}

struct RecordReader {
    filter: RecordFilter,
    partial: Vec<u8>,
    kept: Vec<u8>,
    kept_records: usize,
    records: usize,
    truncated: bool,
}

impl RecordReader {
    fn new(filter: RecordFilter) -> Self {
        Self {
            filter,
            partial: Vec::new(),
            kept: Vec::new(),
            kept_records: 0,
            records: 0,
            truncated: false,
        }
    }

    fn push(&mut self, mut chunk: &[u8]) {
        while !self.truncated && !chunk.is_empty() {
            let end = chunk.iter().position(|byte| *byte == 0);
            let piece = &chunk[..end.unwrap_or(chunk.len())];
            if self.partial.len() + piece.len() > self.filter.max_record_bytes {
                self.fail();
                break;
            }
            self.partial.extend_from_slice(piece);
            let Some(end) = end else { break };
            chunk = &chunk[end + 1..];
            self.records += 1;
            if self.records > self.filter.max_records {
                self.fail();
            } else if (self.filter.keep)(&self.partial) {
                self.kept_records += 1;
                if self.kept_records > self.filter.max_kept {
                    self.fail();
                } else {
                    self.kept.extend_from_slice(&self.partial);
                    self.kept.push(0);
                }
            }
            self.partial.clear();
        }
    }

    /// Drops everything held; the caller fails closed on truncation.
    fn fail(&mut self) {
        self.truncated = true;
        self.partial = Vec::new();
        self.kept = Vec::new();
    }

    fn finish(mut self) -> BoundedBytes {
        // A trailing record without its NUL is incomplete output.
        if !self.partial.is_empty() {
            self.fail();
        }
        BoundedBytes {
            bytes: self.kept,
            truncated: self.truncated,
        }
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;

    fn read(chunks: &[&[u8]]) -> BoundedBytes {
        let mut reader = RecordReader::new(RecordFilter {
            keep: |record| record.starts_with(b"k"),
            max_kept: 2,
            max_records: 4,
            max_record_bytes: 8,
        });
        for chunk in chunks {
            reader.push(chunk);
        }
        reader.finish()
    }

    #[test]
    fn records_split_across_chunks_keep_only_matches() {
        let output = read(&[b"k1\0d", b"rop\0k", b"2\0"]);
        assert_eq!(output.bytes, b"k1\0k2\0");
        assert!(!output.truncated);
    }

    #[test]
    fn every_limit_fails_closed_without_retaining_output() {
        for chunks in [
            &[&b"k1\0k2\0k3\0"[..]][..],
            &[&b"a\0b\0c\0d\0e\0"[..]][..],
            &[&b"k12345678"[..], &b"9\0"[..]][..],
            &[&b"k1\0k2"[..]][..],
        ] {
            let output = read(chunks);
            assert!(output.truncated, "{chunks:?}");
            assert!(output.bytes.is_empty(), "{chunks:?}");
        }
    }
}

/// A Git proxy can exit while its backend still owns the inherited pipes.
/// Retain bounded readers through cleanup until their inherited pipes reach
/// EOF. Redirected or detached children require separate process ownership.
#[derive(Default)]
pub(super) struct ReaderTasks {
    pub(super) stdout: Option<ReaderTask>,
    pub(super) stderr: Option<ReaderTask>,
    pub(super) failed: bool,
}

impl ReaderTasks {
    pub(super) async fn wait_for_eof(&mut self, budget: Duration) -> bool {
        let completed = timeout(budget, async {
            for slot in [&mut self.stdout, &mut self.stderr] {
                if let Some(task) = slot.as_mut() {
                    let result = task.await;
                    slot.take();
                    self.failed |= !matches!(result, Ok(Ok(_)));
                }
            }
        })
        .await
        .is_ok();
        completed && !self.failed
    }
}

impl Drop for ReaderTasks {
    fn drop(&mut self) {
        for task in [&self.stdout, &self.stderr].into_iter().flatten() {
            task.abort();
        }
    }
}
