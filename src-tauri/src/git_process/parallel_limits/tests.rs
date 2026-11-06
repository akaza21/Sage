use super::*;

fn limits(listing: &[u8], cpus: u32) -> Vec<String> {
    ParallelLimits::from_listing(listing, cpus)
        .overrides()
        .collect()
}

#[test]
fn every_key_is_pinned_and_unset_keys_keep_gits_default() {
    assert_eq!(
        limits(b"", 64),
        [
            "checkout.workers=1",
            "fetch.parallel=1",
            "submodule.fetchJobs=1"
        ]
    );
    assert_eq!(limits(b"core.bare\nfalse\0", 64), limits(b"", 64));
}

#[test]
fn values_within_the_cap_are_kept() {
    assert_eq!(
        limits(b"checkout.workers\n4\0fetch.parallel\n8\0", 64),
        [
            "checkout.workers=4",
            "fetch.parallel=8",
            "submodule.fetchJobs=1"
        ]
    );
}

#[test]
fn larger_and_per_cpu_values_are_capped_never_raised() {
    assert_eq!(
        limits(
            b"checkout.workers\n32\0fetch.parallel\n0\0submodule.fetchjobs\n-1\0",
            64,
        ),
        [
            format!("checkout.workers={MAX_PARALLEL_JOBS}"),
            format!("fetch.parallel={MAX_PARALLEL_JOBS}"),
            format!("submodule.fetchJobs={MAX_PARALLEL_JOBS}"),
        ]
    );
    // Per CPU on a two-CPU machine stays at two.
    assert_eq!(limits(b"checkout.workers\n0\0", 2)[0], "checkout.workers=2");
    // Git's multipliers count, and the last value wins.
    assert_eq!(
        limits(b"checkout.workers\n2\0Checkout.Workers\n1k\0", 64)[0],
        format!("checkout.workers={MAX_PARALLEL_JOBS}")
    );
}

#[test]
fn unreadable_values_and_failed_probes_fail_safe_to_one() {
    assert_eq!(
        limits(b"fetch.parallel\nmany\0submodule.fetchjobs\0", 64),
        [
            "checkout.workers=1",
            "fetch.parallel=1",
            "submodule.fetchJobs=1"
        ]
    );
    assert_eq!(ParallelLimits::from_probe(None), ParallelLimits::default());
}

#[test]
fn repositories_with_submodules_never_fetch_in_parallel() {
    let limits = ParallelLimits::from_listing(
        b"checkout.workers\n6\0fetch.parallel\n32\0submodule.fetchjobs\n32\0",
        64,
    )
    .without_parallel_fetches();
    assert_eq!(
        limits.overrides().collect::<Vec<_>>(),
        [
            "checkout.workers=6",
            "fetch.parallel=1",
            "submodule.fetchJobs=1"
        ]
    );
}
