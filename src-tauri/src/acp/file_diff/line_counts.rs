//! Line change counts for one old/new text pair, as a reader of the diff sees
//! them: a line both sides keep in order is not counted, a line only the old
//! side has is a deletion and a line only the new side has is an addition.
//! Shared by every binding that receives whole-file sides instead of a patch.

/// Above this many line comparisons the differing middle of the pair is
/// counted as wholly replaced instead of searched for kept lines, so an
/// oversized side costs bounded work.
const MAX_COMPARISONS: usize = 1_000_000;

/// Returns `(additions, deletions)`. A changed trailing newline changes the
/// last line, as it does in a unified diff.
pub(crate) fn line_change_counts(old_text: &str, new_text: &str) -> (u64, u64) {
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
    let kept = kept_lines(old, new);
    ((new.len() - kept) as u64, (old.len() - kept) as u64)
}

/// Longest common subsequence length over two rolling rows.
fn kept_lines(old: &[&str], new: &[&str]) -> usize {
    if old.is_empty() || new.is_empty() || old.len().saturating_mul(new.len()) > MAX_COMPARISONS {
        return 0;
    }
    let mut previous = vec![0_usize; new.len() + 1];
    let mut current = vec![0_usize; new.len() + 1];
    for line in old {
        for (index, other) in new.iter().enumerate() {
            current[index + 1] = if line == other {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[new.len()]
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
            (1, 1)
        );
    }

    #[test]
    fn creation_and_deletion_count_every_line_on_their_side() {
        assert_eq!(line_change_counts("", "a\nb\n"), (2, 0));
        assert_eq!(line_change_counts("a\nb\nc", ""), (0, 3));
        assert_eq!(line_change_counts("", ""), (0, 0));
    }

    #[test]
    fn kept_lines_between_changes_are_not_counted() {
        assert_eq!(
            line_change_counts("a\nb\nc\nd\ne\n", "a\nx\nc\nd\ny\nz\ne\n"),
            (3, 1)
        );
        assert_eq!(line_change_counts("a\nb\n", "b\na\n"), (1, 1));
    }

    #[test]
    fn a_dropped_trailing_newline_changes_the_last_line() {
        assert_eq!(line_change_counts("a\nb\n", "a\nb"), (1, 1));
    }

    #[test]
    fn an_oversized_middle_counts_as_replaced_instead_of_searching() {
        let old: String = (0..1_001).map(|line| format!("old {line}\n")).collect();
        let new: String = (0..1_001)
            .map(|line| format!("{} {line}\n", if line % 2 == 0 { "old" } else { "new" }))
            .collect();
        let prefixed_old = format!("x\n{old}");
        let prefixed_new = format!("y\n{new}");
        assert_eq!(
            line_change_counts(&prefixed_old, &prefixed_new),
            (1_001, 1_001)
        );
        assert_eq!(line_change_counts(&old, &new), (500, 500));
    }
}
