use std::io::Read;

use crate::{domain::ResourceRef, workspace::Workspace};

use super::{
    contained_file::{ContainedFile, open_contained_file},
    engine::{RootCounters, SearchBudget, SearchEngine},
    types::{SearchLimitReason, WorkspaceSearchMatch, push_reason},
};

impl SearchEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_file(
        &self,
        workspace: &Workspace,
        relative_path: String,
        query: &str,
        case_sensitive: bool,
        matches: &mut Vec<WorkspaceSearchMatch>,
        budget: &mut SearchBudget,
        counters: &mut RootCounters,
    ) {
        let resource = workspace.resource_unchecked(&relative_path);
        let ContainedFile { file, byte_len } = match open_contained_file(workspace, &resource) {
            Ok(contained) => contained,
            Err(_) => {
                counters.error_count += 1;
                counters.partial = true;
                return;
            }
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

        let mut bytes = Vec::with_capacity(byte_len as usize);
        let read_result = (|| {
            file.take(byte_len).read_to_end(&mut bytes)?;
            std::io::Result::Ok(())
        })();
        if read_result.is_err() {
            counters.error_count += 1;
            counters.partial = true;
            return;
        }
        budget.bytes_scanned += bytes.len() as u64;
        counters.bytes_scanned += bytes.len() as u64;
        counters.files_scanned += 1;
        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            counters.skipped_files += 1;
            counters.partial = true;
            return;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            counters.skipped_files += 1;
            counters.partial = true;
            return;
        };
        self.collect_matches(
            resource,
            content,
            query,
            case_sensitive,
            matches,
            budget,
            counters,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_matches(
        &self,
        resource: ResourceRef,
        content: &str,
        query: &str,
        case_sensitive: bool,
        matches: &mut Vec<WorkspaceSearchMatch>,
        budget: &mut SearchBudget,
        counters: &mut RootCounters,
    ) {
        for (line_index, raw_line) in content.split('\n').enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let mut remainder_start = 0;
            while remainder_start <= line.len() {
                let Some((start, end)) =
                    find_literal(&line[remainder_start..], query, case_sensitive)
                else {
                    break;
                };
                let start = remainder_start + start;
                let end = remainder_start + end;
                matches.push(WorkspaceSearchMatch {
                    resource: resource.clone(),
                    line_number: one_based(line_index),
                    column_number: column_number(line, start),
                    preview: bounded_preview(line, self.limits.max_preview_chars),
                });
                if matches.len() == self.limits.max_results {
                    push_reason(&mut budget.reasons, SearchLimitReason::Results);
                    counters.partial = true;
                    return;
                }
                remainder_start = end.max(start + 1);
            }
        }
    }
}

fn find_literal(line: &str, query: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if case_sensitive {
        return line.find(query).map(|start| (start, start + query.len()));
    }
    let needle = query.to_lowercase();
    let mut folded = String::with_capacity(line.len());
    let mut origin = Vec::with_capacity(line.len());
    for (start, character) in line.char_indices() {
        let end = start + character.len_utf8();
        for folded_character in character.to_lowercase() {
            let mut encoded = [0; 4];
            let encoded = folded_character.encode_utf8(&mut encoded);
            folded.push_str(encoded);
            origin.extend(std::iter::repeat_n((start, end), encoded.len()));
        }
    }
    let folded_start = folded.find(&needle)?;
    let folded_end = folded_start + needle.len();
    Some((
        origin.get(folded_start)?.0,
        origin.get(folded_end.checked_sub(1)?)?.1,
    ))
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
