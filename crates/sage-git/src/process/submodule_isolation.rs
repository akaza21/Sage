//! A submodule's own config (filters, hooks paths, `core.worktree`, helpers)
//! is not admitted, so Sage's Git must never start a Git process inside one,
//! recurse a checkout or fetch into one, or summarise one. Every call gets
//! [`CONFIG`], and the commands that take one get a command-line flag too,
//! because the flag also overrides `.gitmodules` `submodule.<name>.ignore`
//! and `fetchRecurseSubmodules`, which config cannot.
//!
//! The trade-off is intended: Sage shows a submodule only as a gitlink
//! change (a different commit recorded or checked out), never the modified
//! or untracked files inside it. Git still reads a submodule's checked-out
//! commit in-process when it refreshes the index, which reads its refs and
//! config but runs nothing there.

pub(super) const CONFIG: [&str; 6] = [
    "submodule.recurse=false",
    "fetch.recurseSubmodules=false",
    "push.recurseSubmodules=no",
    "diff.ignoreSubmodules=dirty",
    "diff.submodule=short",
    "status.submoduleSummary=false",
];

fn flag(subcommand: &str) -> Option<&'static str> {
    match subcommand {
        // Compares only the recorded and checked-out commits, so no Git runs
        // inside a submodule to inspect its files.
        "status" | "diff" | "diff-index" | "diff-files" | "diff-tree" => {
            Some("--ignore-submodules=dirty")
        }
        "fetch" | "pull" | "push" | "checkout" | "switch" | "reset" | "restore" | "read-tree" => {
            Some("--no-recurse-submodules")
        }
        _ => None,
    }
}

pub(super) fn arguments<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut arguments = Vec::with_capacity(args.len() + 1);
    let mut rest = args.iter().copied();
    if let Some(subcommand) = rest.next() {
        arguments.push(subcommand);
        arguments.extend(flag(subcommand));
    }
    arguments.extend(rest);
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_subcommand_that_could_reach_a_submodule_gets_its_flag() {
        assert_eq!(
            arguments(&["status", "--porcelain=v2"]),
            ["status", "--ignore-submodules=dirty", "--porcelain=v2"]
        );
        assert_eq!(
            arguments(&["diff", "--cached", "--", "a"]),
            ["diff", "--ignore-submodules=dirty", "--cached", "--", "a"]
        );
        for subcommand in [
            "fetch", "pull", "push", "checkout", "switch", "reset", "restore",
        ] {
            assert_eq!(
                arguments(&[subcommand, "x"]),
                [subcommand, "--no-recurse-submodules", "x"]
            );
        }
        assert_eq!(arguments(&["rev-parse", "HEAD"]), ["rev-parse", "HEAD"]);
        assert!(arguments(&[]).is_empty());
    }
}
