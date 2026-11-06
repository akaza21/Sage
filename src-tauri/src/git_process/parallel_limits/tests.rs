use super::*;

#[test]
fn unset_keys_get_no_override() {
    assert!(overrides_for(b"", 64).is_empty());
    assert!(overrides_for(b"core.bare\nfalse\0", 64).is_empty());
}

#[test]
fn values_within_the_cap_are_left_alone() {
    assert!(overrides_for(b"checkout.workers\n4\0fetch.parallel\n8\0", 64).is_empty());
}

#[test]
fn larger_and_per_cpu_values_are_capped_never_raised() {
    let overrides = overrides_for(
        b"checkout.workers\n32\0fetch.parallel\n0\0submodule.fetchjobs\n-1\0",
        64,
    );
    assert_eq!(
        overrides,
        [
            format!("checkout.workers={MAX_PARALLEL_JOBS}"),
            format!("fetch.parallel={MAX_PARALLEL_JOBS}"),
            format!("submodule.fetchJobs={MAX_PARALLEL_JOBS}"),
        ]
    );
    // Per CPU on a two-CPU machine stays at two.
    assert_eq!(
        overrides_for(b"checkout.workers\n0\0", 2),
        ["checkout.workers=2"]
    );
    // Git's multipliers count, and the last value wins.
    assert_eq!(
        overrides_for(b"checkout.workers\n2\0Checkout.Workers\n1k\0", 64),
        [format!("checkout.workers={MAX_PARALLEL_JOBS}")]
    );
}

#[test]
fn unreadable_values_and_failed_probes_fail_safe_to_one() {
    assert_eq!(
        overrides_for(b"fetch.parallel\nmany\0submodule.fetchjobs\0", 64),
        ["fetch.parallel=1", "submodule.fetchJobs=1"]
    );
    assert_eq!(
        from_probe(None),
        [
            "checkout.workers=1",
            "fetch.parallel=1",
            "submodule.fetchJobs=1"
        ]
    );
}
