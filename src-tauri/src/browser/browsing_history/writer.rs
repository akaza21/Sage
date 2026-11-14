use std::collections::VecDeque;

use rusqlite::Connection;

use super::store;

/// Browser tabs are capped at eight; the rest covers tabs that closed
/// recently.
const MAX_TABS_TRACKED: usize = 32;
/// Folding keeps a page calling `replaceState` as it scrolls from flooding history.
pub(super) const SAME_DOCUMENT_FOLD_MS: i64 = 5_000;
/// The count prune walks the time index, so it runs on the first write of a
/// session and then periodically.
pub(super) const PRUNE_EVERY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Write {
    Visit {
        tab: String,
        url: String,
        /// `None` for a new document, which reports its title later.
        title: Option<String>,
        visited_at: i64,
        same_document: bool,
    },
    Title {
        tab: String,
        title: String,
    },
}

#[derive(Clone)]
struct LastVisit {
    tab: String,
    id: i64,
    visited_at: i64,
}

#[derive(Clone, Default)]
pub(super) struct Writer {
    last: VecDeque<LastVisit>,
    /// `None` before the first prune.
    since_prune: Option<usize>,
}

impl Writer {
    /// The worker discarded these visits. Later updates must not fall back
    /// to the tab's previous committed visit, even though rollback kept it.
    pub(super) fn invalidate_visits(&mut self, batch: &[Write]) {
        for write in batch {
            if let Write::Visit { tab, .. } = write {
                self.last.retain(|last| last.tab != tab.as_str());
            }
        }
    }

    pub(super) fn apply(
        &mut self,
        connection: &mut Connection,
        batch: &[Write],
        now: i64,
    ) -> rusqlite::Result<()> {
        let transaction = connection.transaction()?;
        // Row ids may be reused after rollback. Keep every cache update
        // local to this transaction until its rows are committed.
        let mut pending = self.clone();
        let mut inserted = 0;
        for write in batch {
            match write {
                Write::Visit {
                    tab,
                    url,
                    title,
                    visited_at,
                    same_document,
                } => {
                    if !store::admissible(url, *visited_at) {
                        continue;
                    }
                    let fold = pending
                        .latest(tab)
                        .filter(|last| {
                            *same_document && visited_at - last.visited_at < SAME_DOCUMENT_FOLD_MS
                        })
                        .map(|last| last.id);
                    if let Some(id) = fold
                        && store::replace_url(&transaction, id, url, title.as_deref())?
                    {
                        continue;
                    }
                    let id = store::insert(&transaction, url, title.as_deref(), *visited_at)?;
                    inserted += 1;
                    pending.remember(tab, id, *visited_at);
                }
                Write::Title { tab, title } => {
                    if let Some(last) = pending.latest(tab) {
                        store::set_title(&transaction, last.id, title)?;
                    }
                }
            }
        }
        let since = pending
            .since_prune
            .map_or(PRUNE_EVERY, |since| since + inserted);
        if since >= PRUNE_EVERY {
            store::prune(&transaction, now)?;
        }
        transaction.commit()?;
        pending.since_prune = Some(if since >= PRUNE_EVERY { 0 } else { since });
        *self = pending;
        Ok(())
    }

    fn latest(&self, tab: &str) -> Option<&LastVisit> {
        self.last.iter().find(|last| last.tab == tab)
    }

    fn remember(&mut self, tab: &str, id: i64, visited_at: i64) {
        self.last.retain(|last| last.tab != tab);
        if self.last.len() == MAX_TABS_TRACKED {
            self.last.pop_front();
        }
        self.last.push_back(LastVisit {
            tab: tab.to_owned(),
            id,
            visited_at,
        });
    }
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
