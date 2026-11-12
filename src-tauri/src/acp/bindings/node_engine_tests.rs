use super::*;

fn check(range: &str, node: &str) -> Option<bool> {
    satisfies(range, parse_node_version(node).unwrap())
}

#[test]
fn the_ranges_harness_packages_declare_are_evaluated() {
    for (range, inside, outside) in [
        (">=22", "22.0.0", "21.99.99"),
        (">= 24.1", "24.1.0", "24.0.9"),
        (">=20.19.0 || >=22.12.0", "20.19.0", "20.18.9"),
        ("^20.19.0 || >=22.12.0", "22.12.0", "21.5.0"),
        ("^20.19.0 || >=22.12.0", "20.20.1", "22.11.0"),
        ("^22", "22.23.5", "23.0.0"),
        ("^0.4.2", "0.4.9", "0.5.0"),
        ("^0.0.3", "0.0.3", "0.0.4"),
        ("~22.1", "22.1.7", "22.2.0"),
        ("~22", "22.9.0", "23.0.0"),
        ("22.x", "22.18.0", "24.0.0"),
        ("22", "22.18.0", "21.0.0"),
        ("=22.18.0", "22.18.0", "22.18.1"),
        ("v22.18.0", "22.18.0", "22.17.0"),
        (">20", "21.0.0", "20.99.0"),
        (">20.5", "20.6.0", "20.5.9"),
        ("<23", "22.99.0", "23.0.0"),
        ("<=22", "22.99.0", "23.0.0"),
        ("<=22.1.3", "22.1.3", "22.1.4"),
        (">=20 <25", "24.9.0", "25.0.0"),
        ("20 - 22", "22.18.0", "23.0.0"),
        ("20.6 - 22.1.0", "20.6.0", "22.1.1"),
    ] {
        assert_eq!(check(range, inside), Some(true), "{inside} in {range}");
        assert_eq!(check(range, outside), Some(false), "{outside} in {range}");
    }
}

#[test]
fn a_range_sage_cannot_read_is_never_a_pass() {
    let long = format!(">={}", "1".repeat(RANGE_BYTES_MAX));
    for range in [
        "",
        "*",
        "x || *",
        ">=",
        ">=a",
        ">= ",
        "^20.19.0-rc.1",
        ">=22.0.0+build",
        "22.x.1",
        "1.2.3.4",
        "-22",
        "22 -",
        ">=22 ||",
        "|| >=22",
        "latest",
        long.as_str(),
    ] {
        assert_eq!(check(range, "24.0.0"), None, "{range:?}");
    }
}

#[test]
fn empty_comparators_admit_nothing() {
    assert_eq!(check("<0.0.0", "0.0.0"), Some(false));
    assert_eq!(check(">*", "24.0.0"), Some(false));
    assert_eq!(check("<*", "24.0.0"), Some(false));
}

#[test]
fn a_node_version_is_read_from_its_bare_or_v_form() {
    assert_eq!(parse_node_version("22.18.0"), Some((22, 18, 0)));
    assert_eq!(parse_node_version("v22.23.5\n"), Some((22, 23, 5)));
    for refused in ["22", "22.18", "22.18.0.1", "22.18.0-nightly", "", "v"] {
        assert_eq!(parse_node_version(refused), None, "{refused}");
    }
}
