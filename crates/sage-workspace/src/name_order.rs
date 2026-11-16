use std::cmp::Ordering;

/// The exact name breaks ties, so names that differ only in case or leading
/// zeros still sort the same every time.
pub(crate) fn explorer_name_cmp(left: &str, right: &str) -> Ordering {
    natural_cmp(left, right).then_with(|| left.cmp(right))
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (left_bytes, right_bytes) = (left.as_bytes(), right.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if left_bytes[i].is_ascii_digit() && right_bytes[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < left.len() && left_bytes[i].is_ascii_digit() {
                i += 1;
            }
            while j < right.len() && right_bytes[j].is_ascii_digit() {
                j += 1;
            }
            let (left_run, right_run) = (&left[start_i..i], &right[start_j..j]);
            let (left_value, right_value) = (
                left_run.trim_start_matches('0'),
                right_run.trim_start_matches('0'),
            );
            // Leading zeros don't count here, as in VS Code: `01` and `1` tie
            // and the exact-name tie-break orders them at the end.
            let order = left_value
                .len()
                .cmp(&right_value.len())
                .then_with(|| left_value.cmp(right_value));
            if order.is_ne() {
                return order;
            }
        } else {
            let (Some(x), Some(y)) = (left[i..].chars().next(), right[j..].chars().next()) else {
                break;
            };
            i += x.len_utf8();
            j += y.len_utf8();
            let order = x.to_lowercase().cmp(y.to_lowercase());
            if order.is_ne() {
                return order;
            }
        }
    }
    (i < left.len()).cmp(&(j < right.len()))
}

#[cfg(test)]
mod tests {
    use super::explorer_name_cmp;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        names.sort_unstable_by(|left, right| explorer_name_cmp(left, right));
        names
    }

    #[test]
    fn compares_digit_runs_by_value_and_letters_without_case() {
        assert_eq!(
            sorted(&[
                "file10.ts",
                "File2.ts",
                "file1.ts",
                "file02.ts",
                "file2.ts",
                "a",
                "B",
                "_x",
                "é",
                "z"
            ]),
            [
                "_x",
                "a",
                "B",
                "file1.ts",
                "File2.ts",
                "file02.ts",
                "file2.ts",
                "file10.ts",
                "z",
                "é"
            ]
        );
    }

    #[test]
    fn leading_zeros_do_not_decide_before_later_text() {
        assert_eq!(
            sorted(&["img1_z.png", "img01_a.png"]),
            ["img01_a.png", "img1_z.png"]
        );
    }

    #[test]
    fn orders_prefixes_and_huge_numbers() {
        assert_eq!(
            sorted(&[
                "v1.10",
                "v1.9",
                "v1",
                "v1.9.1",
                "n99999999999999999999999",
                "n100000000000000000000000"
            ]),
            [
                "n99999999999999999999999",
                "n100000000000000000000000",
                "v1",
                "v1.9",
                "v1.9.1",
                "v1.10"
            ]
        );
    }
}
