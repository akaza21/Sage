use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
};

use crate::domain::ResourceRef;

use super::{
    IndexInputs, ProjectIndex, Readied, Ticket, claim_generation, ready,
    score::{Query, positions, score},
    superseded,
};

/// A newer query is checked for after this many paths.
const CANCEL_CHECK_EVERY: usize = 8192;
/// Indexes smaller than this are ranked on the calling thread; larger ones
/// are split across up to four. Ranking allocates almost nothing, so the
/// extra threads leave no memory behind.
const PARALLEL_FROM: usize = 20_000;
/// The most recent File gets this much extra; each older one a step less.
const RECENT_BONUS: i32 = 200;
const RECENT_STEP: i32 = 4;
const RECENT_FILTER_BITS: usize = 8192;

/// Ranks a browsed File that is not recent below any recent File.
const BROWSED: i32 = i32::MAX / 2;

pub(crate) struct QuickOpenQuery<'a> {
    pub(crate) text: &'a str,
    pub(crate) limit: usize,
    pub(crate) generation: u64,
    pub(crate) session: u64,
    pub(crate) recent: &'a [ResourceRef],
    pub(crate) browse: bool,
}

#[derive(Debug)]
pub(crate) struct Ranked {
    pub(crate) root_id: String,
    pub(crate) relative_path: String,
    /// Matched characters, as UTF-16 offsets into `relative_path`.
    pub(crate) positions: Vec<u32>,
}

pub(crate) enum Outcome {
    Ready {
        results: Vec<Ranked>,
        files: usize,
        truncated: bool,
        heap_bytes: usize,
        readied: Readied,
    },
    /// A newer query arrived, or the Project closed.
    Superseded,
}

pub(crate) fn run(inputs: &IndexInputs, query: &QuickOpenQuery<'_>, ticket: Ticket) -> Outcome {
    if !claim_generation(query.generation) {
        return Outcome::Superseded;
    }
    let Some((index, readied)) = ready(inputs, query.session, query.generation, ticket) else {
        return Outcome::Superseded;
    };
    if superseded(query.generation) {
        return Outcome::Superseded;
    }
    let Some(results) = rank(&index, query) else {
        return Outcome::Superseded;
    };
    Outcome::Ready {
        results,
        files: index.files(),
        truncated: index.truncated(),
        heap_bytes: index.heap_bytes(),
        readied,
    }
}

struct Candidate<'a> {
    score: i32,
    root: usize,
    path: &'a str,
    base: usize,
}

impl Candidate<'_> {
    fn rank(&self, other: &Self) -> Ordering {
        other
            .score
            .cmp(&self.score)
            .then_with(|| self.path.len().cmp(&other.path.len()))
            .then_with(|| self.path.cmp(other.path))
            .then_with(|| self.root.cmp(&other.root))
    }
}

// The heap keeps the worst kept candidate on top, ready to be replaced.
impl Ord for Candidate<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank(other)
    }
}

impl PartialOrd for Candidate<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate<'_> {}

fn filter_slot(path: &str, base: usize) -> usize {
    let bytes = path.as_bytes();
    let at = |index: usize| bytes.get(index).map_or(0, |byte| *byte as usize);
    let hash = (bytes.len() * 0x9e37)
        ^ (at(base) * 0x85eb)
        ^ (at(base + 1) * 0xc2b3)
        ^ (at(bytes.len().wrapping_sub(1)) * 0x27d5);
    (hash ^ (hash >> 13)) % RECENT_FILTER_BITS
}

struct Ranking<'a> {
    parsed: Option<Query>,
    recent: HashMap<(&'a str, &'a str), usize>,
    recent_filter: Vec<u64>,
    limit: usize,
    generation: u64,
    browse: bool,
}

impl<'a> Ranking<'a> {
    fn new(query: &QuickOpenQuery<'a>) -> Self {
        let mut recent_filter = vec![0; RECENT_FILTER_BITS / 64];
        let mut recent = HashMap::with_capacity(query.recent.len());
        for (rank, resource) in query.recent.iter().enumerate() {
            let path = resource.relative_path.as_str();
            let slot = filter_slot(path, path.rfind('/').map_or(0, |slash| slash + 1));
            recent_filter[slot / 64] |= 1 << (slot % 64);
            recent
                .entry((resource.root_id.as_str(), path))
                .or_insert(rank);
        }
        Self {
            parsed: Query::new(query.text),
            recent,
            recent_filter,
            limit: query.limit,
            generation: query.generation,
            browse: query.browse,
        }
    }

    /// 0 for the most recent.
    fn recent_rank(&self, root_id: &str, path: &str, base: usize) -> Option<usize> {
        let slot = filter_slot(path, base);
        if self.recent_filter[slot / 64] & (1 << (slot % 64)) == 0 {
            return None;
        }
        self.recent.get(&(root_id, path)).copied()
    }

    fn recency(&self, root_id: &str, path: &str, base: usize) -> i32 {
        self.recent_rank(root_id, path, base)
            .map_or(0, |rank| RECENT_BONUS - RECENT_STEP * rank as i32)
    }

    /// `None` when a newer query arrived meanwhile.
    fn scan<'i>(
        &self,
        index: &'i ProjectIndex,
        share: usize,
        shares: usize,
    ) -> Option<Vec<Candidate<'i>>> {
        let mut kept = BinaryHeap::with_capacity(self.limit + 1);
        let mut seen = 0;
        for (position, root) in index.roots.iter().enumerate() {
            let slots = root.arena.len();
            let from = slots * share / shares;
            let to = slots * (share + 1) / shares;
            for stored in root.arena.range(from..to) {
                seen += 1;
                if seen % CANCEL_CHECK_EVERY == 0 && superseded(self.generation) {
                    return None;
                }
                let score = match &self.parsed {
                    Some(parsed) => match score(parsed, stored.path, stored.base, stored.ascii) {
                        Some(score) => {
                            score + self.recency(&root.root_id, stored.path, stored.base)
                        }
                        None => continue,
                    },
                    // An empty query lists recently opened Files only when not browsing, as
                    // VS Code does.
                    None => match self.recent_rank(&root.root_id, stored.path, stored.base) {
                        Some(rank) => -(rank as i32),
                        None if self.browse => -BROWSED,
                        None => continue,
                    },
                };
                let candidate = Candidate {
                    score,
                    root: position,
                    path: stored.path,
                    base: stored.base,
                };
                if kept.len() < self.limit {
                    kept.push(candidate);
                } else if kept
                    .peek()
                    .is_some_and(|worst| candidate.rank(worst) == Ordering::Less)
                {
                    kept.pop();
                    kept.push(candidate);
                }
            }
        }
        Some(kept.into_vec())
    }
}

/// `None` when a newer query arrived meanwhile.
fn rank(index: &ProjectIndex, query: &QuickOpenQuery<'_>) -> Option<Vec<Ranked>> {
    let shares = if index.files() >= PARALLEL_FROM {
        std::thread::available_parallelism().map_or(1, |cores| cores.get().min(4))
    } else {
        1
    };
    rank_in_shares(index, query, shares)
}

pub(super) fn rank_in_shares(
    index: &ProjectIndex,
    query: &QuickOpenQuery<'_>,
    shares: usize,
) -> Option<Vec<Ranked>> {
    let ranking = Ranking::new(query);
    let mut kept = Vec::with_capacity(query.limit * shares);
    if shares == 1 {
        kept = ranking.scan(index, 0, 1)?;
    } else {
        let found = std::thread::scope(|scope| {
            let ranking = &ranking;
            let workers = (0..shares)
                .map(|share| scope.spawn(move || ranking.scan(index, share, shares)))
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().ok().flatten())
                .collect::<Vec<_>>()
        });
        for share in found {
            kept.extend(share?);
        }
    }
    kept.sort_by(Candidate::rank);
    kept.truncate(query.limit);
    Some(
        kept.into_iter()
            .map(|candidate| Ranked {
                root_id: index.roots[candidate.root].root_id.clone(),
                relative_path: candidate.path.to_owned(),
                positions: ranking
                    .parsed
                    .as_ref()
                    .map(|parsed| positions(parsed, candidate.path, candidate.base))
                    .unwrap_or_default(),
            })
            .collect(),
    )
}
