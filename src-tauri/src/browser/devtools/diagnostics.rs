use std::collections::VecDeque;

use crate::acp::{
    browser::{BrowserFailure, ConsoleEntry, NetworkEntry, redact_console, redact_url},
    limits::BROWSER_LIMITS,
};

#[derive(Debug, Default)]
pub(super) struct DiagnosticRings {
    console: VecDeque<ConsoleEntry>,
    network: VecDeque<NetworkEntry>,
    next_console: u64,
    next_network: u64,
}

impl DiagnosticRings {
    pub(super) fn push_console(&mut self, level: &str, text: &str) {
        let Some(cursor) = self.next_console.checked_add(1) else {
            self.console.clear();
            return;
        };
        self.next_console = cursor;
        self.console.push_back(ConsoleEntry {
            cursor,
            level: bounded(
                &redact_console(level),
                BROWSER_LIMITS.diagnostic_level_bytes,
            ),
            text: bounded(&redact_console(text), BROWSER_LIMITS.diagnostic_text_bytes),
        });
        trim(&mut self.console);
    }

    pub(super) fn push_network(&mut self, method: &str, url: &str, status: u16) {
        let Some(cursor) = self.next_network.checked_add(1) else {
            self.network.clear();
            return;
        };
        self.next_network = cursor;
        self.network.push_back(NetworkEntry {
            cursor,
            method: bounded(&redact_console(method), BROWSER_LIMITS.key_bytes),
            url: bounded(&redact_url(url), BROWSER_LIMITS.url_bytes),
            status,
        });
        trim(&mut self.network);
    }

    pub(super) fn console(
        &self,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<(u64, Vec<ConsoleEntry>), BrowserFailure> {
        page(&self.console, cursor, limit, self.next_console, |entry| {
            entry.cursor
        })
    }

    pub(super) fn network(
        &self,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<(u64, Vec<NetworkEntry>), BrowserFailure> {
        page(&self.network, cursor, limit, self.next_network, |entry| {
            entry.cursor
        })
    }
}

fn bounded(value: &str, cap: usize) -> String {
    let mut end = value.len().min(cap);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn trim<T>(ring: &mut VecDeque<T>) {
    while ring.len() > BROWSER_LIMITS.diagnostic_entries {
        ring.pop_front();
    }
}

fn page<T: Clone>(
    ring: &VecDeque<T>,
    cursor: Option<u64>,
    limit: usize,
    newest: u64,
    identity: impl Fn(&T) -> u64,
) -> Result<(u64, Vec<T>), BrowserFailure> {
    if limit == 0 {
        return Err(BrowserFailure::InvalidRequest);
    }
    if limit > BROWSER_LIMITS.diagnostic_entries {
        return Err(BrowserFailure::Capacity);
    }
    let after = cursor.unwrap_or_else(|| ring.front().map_or(newest, |first| identity(first) - 1));
    if after > newest {
        return Err(BrowserFailure::InvalidRequest);
    }
    if ring
        .front()
        .is_some_and(|first| after < identity(first) - 1)
    {
        return Err(BrowserFailure::Capacity);
    }
    let entries = ring
        .iter()
        .filter(|entry| identity(entry) > after)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let next = entries.last().map_or(after, &identity);
    Ok((next, entries))
}
