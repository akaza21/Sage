use super::*;

/// The path a gitfile in `repo` names, as walked from there.
fn target(bytes: &[u8]) -> Option<PathBuf> {
    written_prefixes(Path::new("repo"), gitfile_path(bytes)?)?.pop()
}

#[test]
fn only_git_line_endings_are_stripped() {
    let expected = Some(PathBuf::from("repo/../data.git"));
    for bytes in [
        &b"gitdir: ../data.git"[..],
        b"gitdir: ../data.git\n",
        b"gitdir: ../data.git\r\n",
    ] {
        assert_eq!(target(bytes), expected, "{bytes:?}");
    }
}

#[test]
fn paths_git_could_read_differently_are_refused() {
    for bytes in [
        &b"gitdir: ../data.git \n"[..],
        b"gitdir: ../ data.git\n",
        b"gitdir: ../data.git\t\n",
        b"gitdir: ../data.git\0x\n",
        b"gitdir: ../data.git\n\n",
        b"gitdir: ../d\xffta.git\n",
        b"gitdir:../data.git\n",
        b"gitdir:  ../data.git\n",
    ] {
        assert_eq!(target(bytes), None, "{bytes:?}");
    }
}

#[test]
fn commondir_files_drop_every_trailing_line_ending() {
    assert_eq!(commondir_path(b"../..\r\n\n"), Some("../.."));
    assert_eq!(commondir_path(b"../.."), Some("../.."));
}

#[test]
fn a_backslash_is_a_separator_only_on_windows() {
    let separated = target(b"gitdir: ..\\data.git\n");
    let named = target(b"gitdir: ../foo\\bar\n");
    if cfg!(windows) {
        assert_eq!(separated, Some(PathBuf::from("repo/../data.git")));
        assert_eq!(named, Some(PathBuf::from("repo/../foo/bar")));
    } else {
        assert_eq!(separated, None);
        assert_eq!(named, None);
    }
}

#[test]
fn prefixes_are_bounded_by_component_count_and_length() {
    let within = vec!["d"; MAX_PATH_COMPONENTS].join("/");
    assert_eq!(
        written_prefixes(Path::new("repo"), &within).map(|prefixes| prefixes.len()),
        Some(MAX_PATH_COMPONENTS)
    );
    let deeper = vec!["d"; MAX_PATH_COMPONENTS + 1].join("/");
    assert_eq!(written_prefixes(Path::new("repo"), &deeper), None);
    let longer = "x".repeat(MAX_PATH_BYTES + 1);
    assert_eq!(written_prefixes(Path::new("repo"), &longer), None);
}

#[cfg(unix)]
#[test]
fn an_absolute_path_starts_at_the_root() {
    assert_eq!(
        written_prefixes(Path::new("repo"), "/srv/data.git").unwrap(),
        [PathBuf::from("/srv"), PathBuf::from("/srv/data.git")]
    );
}

#[cfg(windows)]
#[test]
fn drive_and_unc_paths_start_where_git_starts_them() {
    assert_eq!(
        written_prefixes(Path::new("repo"), "C:/work/data.git").unwrap(),
        [
            PathBuf::from(r"C:\work"),
            PathBuf::from(r"C:\work\data.git")
        ]
    );
    for unc in [r"\\server\share\data.git", "//server/share/data.git"] {
        assert_eq!(
            written_prefixes(Path::new("repo"), unc).unwrap(),
            [PathBuf::from(r"\\server\share\data.git")],
            "{unc}"
        );
    }
}

#[cfg(windows)]
#[test]
fn starts_git_and_win32_could_read_differently_are_refused() {
    for target in [
        // The current drive's root, and a drive's current directory.
        "/work/data.git",
        "C:work/data.git",
        // Verbatim and device namespaces, and incomplete or odd shares.
        r"\\?\C:\work\data.git",
        r"\\.\pipe\data.git",
        r"\\server",
        r"\\server\share.\data.git",
        r"\\server \share\data.git",
    ] {
        assert_eq!(
            written_prefixes(Path::new("repo"), target),
            None,
            "{target}"
        );
    }
}

#[cfg(windows)]
#[test]
fn a_component_win32_would_strip_is_refused() {
    assert_eq!(target(b"gitdir: ../data.git.\n"), None);
}

#[test]
fn verbatim_windows_paths_become_plain_for_the_os() {
    if cfg!(windows) {
        assert_eq!(
            os_path(Path::new(r"\\?\C:\work")),
            PathBuf::from(r"C:\work")
        );
        assert_eq!(
            os_path(Path::new(r"\\?\UNC\server\share\work")),
            PathBuf::from(r"\\server\share\work")
        );
    } else {
        assert_eq!(os_path(Path::new("/work")), PathBuf::from("/work"));
    }
}
