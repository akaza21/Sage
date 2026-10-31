//! Applies queued visits and titles on the worker thread. It remembers the
//! latest visit row of each recent tab, so a title lands on the visit its
//! own tab made, and a burst of same-document URL changes (a page calling
//! `replaceState` as it scrolls, say) folds into one row instead of
//! flooding the store.

use std::collections::VecDeque;

use rusqlite::Connection;

use super::store;

/// Recent tabs whose latest visit is remembered. Browser tabs are capped
/// at eight; the rest covers tabs that closed recently.
const MAX_TABS_TRACKED: usize = 32;
/// A same-document change this soon after its tab's last visit replaces
/// that visit's URL rather than adding a row.
pub(super) const SAME_DOCUMENT_FOLD_MS: i64 = 5_000;
/// Inserted visits between prunes. The count prune walks the time index,
/// so it runs on the first write of a session and then periodically.
pub(super) const PRUNE_EVERY: usize = 256;

/// A write the worker applies in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Write {
    Visit {
        tab: String,
        url: String,
        /// The tab's title when the URL changed within its document;
        /// a new document reports its title later.
        title: Option<String>,
        visited_at: i64,
        same_document: bool,
    },
    Title {
        tab: String,
        title: String,
    },
}

struct LastVisit {
    tab: String,
    id: i64,
    visited_at: i64,
}

#[derive(Default)]
pub(super) struct Writer {
    last: VecDeque<LastVisit>,
    /// Visits inserted since the last prune; `None` before the first.
    since_prune: Option<usize>,
}

impl Writer {
    /// Writes a batch in one transaction and prunes when due. `now` is the
    /// current time, for the age bound.
    pub(super) fn apply(
        &mut self,
        connection: &mut Connection,
        batch: &[Write],
        now: i64,
    ) -> rusqlite::Result<()> {
        let transaction = connection.transaction()?;
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
                    let fold = self
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
                    self.remember(tab, id, *visited_at);
                }
                Write::Title { tab, title } => {
                    if let Some(last) = self.latest(tab) {
                        store::set_title(&transaction, last.id, title)?;
                    }
                }
            }
        }
        let since = self
            .since_prune
            .map_or(PRUNE_EVERY, |since| since + inserted);
        if since >= PRUNE_EVERY {
            store::prune(&transaction, now)?;
        }
        transaction.commit()?;
        self.since_prune = Some(if since >= PRUNE_EVERY { 0 } else { since });
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
