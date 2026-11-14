//! Line change counts for one old/new text pair, as a reader of the diff sees
//! them: a line both sides keep in order is not counted, a line only the old
//! side has is a deletion and a line only the new side has is an addition.
//! Shared by every binding that receives whole-file sides instead of a patch.

/// Work bound for the shortest-edit search (Myers, O((N+M)·D)). A small edit
/// to a large file stays cheap; a pair too different to settle within it has
/// unknown counts rather than an estimate.
const MAX_SEARCH_STEPS: usize = 1_000_000;
/// Edit depth past which the step bound is always exceeded anyway; it sizes
/// the per-diagonal row.
const MAX_EDIT_DEPTH: usize = 2_048;

/// Returns `(additions, deletions)`, or `None` when the bound is reached
/// before the shortest edit is known. A changed trailing newline changes the
/// last line, as it does in a unified diff.
pub(crate) fn line_change_counts(old_text: &str, new_text: &str) -> Option<(u64, u64)> {
    let old: Vec<&str> = old_text.split_inclusive('\n').collect();
    let new: Vec<&str> = new_text.split_inclusive('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let (old, new) = (&old[prefix..], &new[prefix..]);
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old, new) = (&old[..old.len() - suffix], &new[..new.len() - suffix]);
    let distance = edit_distance(old, new)?;
    // distance = additions + deletions and additions - deletions = M - N.
    let additions = (distance + new.len() - old.len()) / 2;
    Some((additions as u64, (distance - additions) as u64))
}

/// Myers' greedy shortest edit script length, keeping only the furthest
/// reaching path per diagonal.
fn edit_distance(old: &[&str], new: &[&str]) -> Option<usize> {
    let (n, m) = (old.len(), new.len());
    if n == 0 || m == 0 {
        return Some(n + m);
    }
    let max = (n + m).min(MAX_EDIT_DEPTH);
    let offset = max + 1;
    let mut furthest = vec![0_usize; 2 * max + 3];
    let mut steps = 0_usize;
    for depth in 0..=max {
        for diagonal in (0..=2 * depth).step_by(2) {
            // k = diagonal - depth, stored at index k + offset.
            let index = offset + diagonal - depth;
            let down = diagonal == 0
                || (diagonal != 2 * depth && furthest[index - 1] < furthest[index + 1]);
            let mut x = if down {
                furthest[index + 1]
            } else {
                furthest[index - 1] + 1
            };
            // y = x - k, never negative on a reachable diagonal.
            let mut y = (x + depth).checked_sub(diagonal)?;
            while x < n && y < m && old[x] == new[y] {
                x += 1;
                y += 1;
                steps += 1;
            }
            steps += 1;
            if steps > MAX_SEARCH_STEPS {
                return None;
            }
            furthest[index] = x;
            if x >= n && y >= m {
                return Some(depth);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::line_change_counts;

    #[test]
    fn one_changed_line_in_a_whole_file_pair_counts_once_each_way() {
        assert_eq!(
            line_change_counts(
                "pub fn answer() -> u32 {\n    41\n}\n",
                "pub fn answer() -> u32 {\n    42\n}\n"
            ),
            Some((1, 1))
        );
    }

    #[test]
    fn creation_and_deletion_count_every_line_on_their_side() {
        assert_eq!(line_change_counts("", "a\nb\n"), Some((2, 0)));
        assert_eq!(line_change_counts("a\nb\nc", ""), Some((0, 3)));
        assert_eq!(line_change_counts("", ""), Some((0, 0)));
    }

    #[test]
    fn kept_lines_between_changes_are_not_counted() {
        assert_eq!(
            line_change_counts("a\nb\nc\nd\ne\n", "a\nx\nc\nd\ny\nz\ne\n"),
            Some((3, 1))
        );
        assert_eq!(line_change_counts("a\nb\n", "b\na\n"), Some((1, 1)));
        assert_eq!(line_change_counts("a\nb\nc\n", "c\nb\na\n"), Some((2, 2)));
    }

    #[test]
    fn a_dropped_trailing_newline_changes_the_last_line() {
        assert_eq!(line_change_counts("a\nb\n", "a\nb"), Some((1, 1)));
    }

    #[test]
    fn a_large_file_edited_at_both_ends_counts_only_the_edited_lines() {
        let body: String = (0..999).map(|line| format!("kept {line}\n")).collect();
        let old = format!("first\n{body}last\n");
        let new = format!("FIRST\n{body}LAST\n");
        assert_eq!(line_change_counts(&old, &new), Some((2, 2)));
        let alternating: String = (0..1_001)
            .map(|line| format!("{} {line}\n", if line % 2 == 0 { "old" } else { "new" }))
            .collect();
        let old_side: String = (0..1_001).map(|line| format!("old {line}\n")).collect();
        assert_eq!(
            line_change_counts(&old_side, &alternating),
            Some((500, 500))
        );
    }

    #[test]
    fn a_pair_too_different_to_settle_within_the_bound_has_unknown_counts() {
        let old: String = (0..2_000).map(|line| format!("old {line}\n")).collect();
        let new: String = (0..2_000).map(|line| format!("new {line}\n")).collect();
        assert_eq!(line_change_counts(&old, &new), None);
    }
}
