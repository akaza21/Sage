use std::{
    ffi::OsStr,
    fs::Metadata,
    path::{Component, Path},
};

use super::{HarnessExecutable, PATH_BYTES_MAX};

pub(super) fn expected_basenames(harness: HarnessExecutable) -> &'static [&'static str] {
    #[cfg(windows)]
    {
        match harness {
            HarnessExecutable::Codex => &["codex.exe", "codex.cmd"],
            HarnessExecutable::Claude => &["claude.exe", "claude.cmd"],
            HarnessExecutable::Pi => &["pi.exe", "pi.cmd"],
            HarnessExecutable::Gemini => &["gemini.exe", "gemini.cmd"],
            HarnessExecutable::OpenCode => &["opencode.exe", "opencode.cmd"],
            HarnessExecutable::CursorAgent => &["cursor-agent.exe", "cursor-agent.cmd"],
            HarnessExecutable::OhMyPi => &["omp.exe", "omp.cmd"],
        }
    }
    #[cfg(not(windows))]
    {
        match harness {
            HarnessExecutable::Codex => &["codex"],
            HarnessExecutable::Claude => &["claude"],
            HarnessExecutable::Pi => &["pi"],
            HarnessExecutable::Gemini => &["gemini"],
            HarnessExecutable::OpenCode => &["opencode"],
            HarnessExecutable::CursorAgent => &["cursor-agent"],
            HarnessExecutable::OhMyPi => &["omp"],
        }
    }
}

pub(super) fn authentic_harness_file(path: &Path, expected_basename: &str) -> bool {
    path.file_name() == Some(OsStr::new(expected_basename)) && authentic_file_with_parents(path)
}

pub(super) fn authentic_regular_file(path: &Path) -> bool {
    authentic_file_with_parents(path)
}

/// Validates a package file relative to a bounded, plain npm-global root. Every
/// path segment is inspected so a package, `node_modules`, or adapter entry can
/// never cross a symlink/reparse-point boundary after root discovery.
pub(super) fn authentic_descendant_file(root: &Path, relative: impl AsRef<Path>) -> bool {
    let relative = relative.as_ref();
    if !is_local_probe_root(root)
        || relative.as_os_str().is_empty()
        || has_aliased_segment(relative)
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return false;
    }
    let mut current = root.to_path_buf();
    if !usable_probe_directory(&current) {
        return false;
    }
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(name) = part else {
            return false;
        };
        current.push(name);
        let metadata = current.symlink_metadata();
        if parts.peek().is_some() {
            if !matches!(metadata, Ok(meta) if meta.is_dir() && !is_link(&meta)) {
                return false;
            }
        } else if !matches!(metadata, Ok(meta) if meta.is_file() && !is_link(&meta)) {
            return false;
        }
    }
    path_native_bytes(current.as_os_str()) <= PATH_BYTES_MAX
}

fn authentic_file_with_parents(path: &Path) -> bool {
    if path.as_os_str().is_empty()
        || path_native_bytes(path.as_os_str()) > PATH_BYTES_MAX
        || has_aliased_segment(path)
    {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    usable_probe_directory(parent)
        && matches!(path.symlink_metadata(), Ok(meta) if meta.is_file() && !is_link(&meta))
}

pub(super) fn is_local_probe_root(dir: &Path) -> bool {
    if dir.as_os_str().is_empty()
        || path_native_bytes(dir.as_os_str()) > PATH_BYTES_MAX
        || has_aliased_segment(dir)
    {
        return false;
    }
    if dir
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        use std::{
            os::windows::ffi::OsStrExt,
            path::{PathBuf, Prefix},
        };
        let Some(Component::Prefix(prefix)) = dir.components().next() else {
            return false;
        };
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) || !dir.is_absolute()
        {
            return false;
        }
        let mut volume = PathBuf::from(prefix.as_os_str());
        volume.push(r"\");
        let mut wide: Vec<u16> = volume.as_os_str().encode_wide().collect();
        wide.push(0);
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        const DRIVE_RAMDISK: u32 = 6;
        matches!(
            unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) },
            DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK
        )
    }
    #[cfg(not(windows))]
    {
        dir.is_absolute()
    }
}

// Path::components normalizes embedded and trailing `.` segments away. Inspect
// the native spelling first; ASCII separators are preserved by as_encoded_bytes.
// Windows also aliases trailing dots/spaces. Reject those spellings for ordinary
// and verbatim paths alike, including segments made entirely of dots/spaces.
fn has_aliased_segment(path: &Path) -> bool {
    path.as_os_str()
        .as_encoded_bytes()
        .split(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'))
        .any(|segment| {
            segment == b"." || (cfg!(windows) && matches!(segment.last(), Some(b'.' | b' ')))
        })
}

fn usable_probe_directory(dir: &Path) -> bool {
    if !is_local_probe_root(dir) {
        return false;
    }
    matches!(dir.symlink_metadata(), Ok(meta) if meta.is_dir() && !is_link(&meta))
}

fn path_native_bytes(path: &OsStr) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.encode_wide().count().saturating_mul(2)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_bytes().len()
    }
}

fn is_link(meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("sage-path-guard-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).expect("create fixture root");
            Self(root)
        }

        fn package(&self) -> PathBuf {
            let relative = PathBuf::from("node_modules/pkg/package.json");
            let file = self.0.join(&relative);
            fs::create_dir_all(file.parent().unwrap()).expect("create package directory");
            fs::write(file, "{}").expect("write package");
            relative
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove fixture root");
        }
    }

    #[test]
    fn plain_descendant_file_is_accepted() {
        let fixture = Fixture::new();
        assert!(authentic_descendant_file(&fixture.0, fixture.package()));
        assert!(is_local_probe_root(&fixture.0));
    }

    #[test]
    fn non_normal_relative_paths_are_rejected() {
        for relative in [
            "node_modules/pkg/../pkg/package.json",
            "./node_modules/pkg/package.json",
            "node_modules/./pkg/package.json",
            "node_modules/pkg/./package.json",
            "",
        ] {
            let fixture = Fixture::new();
            fixture.package();
            assert!(
                !authentic_descendant_file(&fixture.0, relative),
                "{relative}"
            );
        }
    }

    #[test]
    fn absolute_descendant_path_is_rejected() {
        let fixture = Fixture::new();
        let relative = fixture.package();
        assert!(!authentic_descendant_file(
            &fixture.0,
            fixture.0.join(relative)
        ));
    }

    #[test]
    fn relative_probe_root_is_rejected() {
        let fixture = Fixture::new();
        let relative = fixture.package();
        assert!(!is_local_probe_root(Path::new("relative")));
        assert!(!authentic_descendant_file(Path::new("relative"), relative));
    }

    #[test]
    fn dot_segments_in_probe_roots_are_rejected() {
        for suffix in [
            "node_modules/../node_modules",
            "node_modules/.",
            "node_modules/./pkg",
        ] {
            let fixture = Fixture::new();
            fixture.package();
            assert!(!is_local_probe_root(&fixture.0.join(suffix)), "{suffix}");
        }
    }

    #[cfg(windows)]
    fn windows_root(fixture: &Fixture, verbatim: bool) -> PathBuf {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let canonical = fixture.0.canonicalize().expect("canonical fixture root");
        if verbatim {
            canonical
        } else {
            // canonicalize produces a verbatim disk path; preserve all native
            // UTF-16 units while removing only its four-unit namespace prefix.
            let units: Vec<_> = canonical.as_os_str().encode_wide().collect();
            PathBuf::from(std::ffi::OsString::from_wide(&units[4..]))
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_aliased_probe_roots_are_rejected() {
        for verbatim in [false, true] {
            for alias in [
                ". ",
                "..  ",
                "node_modules.",
                "node_modules ",
                "...",
                " . . ",
                " ",
            ] {
                for intermediate in [false, true] {
                    let fixture = Fixture::new();
                    fixture.package();
                    let root = windows_root(&fixture, verbatim);
                    assert!(is_local_probe_root(&root));
                    let mut aliased = root.join(alias);
                    if intermediate {
                        aliased.push("pkg");
                    }
                    assert!(!is_local_probe_root(&aliased), "{aliased:?}");
                    assert!(!authentic_descendant_file(&aliased, "package.json"));
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_aliased_descendants_and_regular_files_are_rejected() {
        for verbatim in [false, true] {
            for relative in [
                "node_modules./pkg/package.json",
                "node_modules /pkg/package.json",
                "node_modules/. /pkg/package.json",
                "node_modules/pkg/..  /pkg/package.json",
                "node_modules/pkg/package.json.",
                "node_modules/pkg/package.json ",
                "node_modules/pkg/...",
                "node_modules/pkg/ . . ",
                "node_modules/pkg/ ",
            ] {
                for separator in ["/", "\\"] {
                    let fixture = Fixture::new();
                    let package = fixture.package();
                    let root = windows_root(&fixture, verbatim);
                    assert!(authentic_descendant_file(&root, &package));
                    assert!(authentic_regular_file(&root.join(&package)));
                    let relative = relative.replace('/', separator);
                    assert!(!authentic_descendant_file(&root, &relative), "{relative}");
                    assert!(!authentic_regular_file(&root.join(relative)));
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_literal_dot_and_space_suffixes_are_accepted() {
        for name in [". ", "..  ", "pkg.", "pkg ", "...", " . . ", " "] {
            let fixture = Fixture::new();
            let directory = fixture.0.join(name);
            fs::create_dir(&directory).unwrap();
            let file = directory.join("package.json ");
            fs::write(&file, "{}").unwrap();
            assert!(is_local_probe_root(&directory));
            assert!(authentic_descendant_file(
                &fixture.0,
                Path::new(name).join("package.json ")
            ));
            assert!(authentic_regular_file(&file));
        }
    }

    #[test]
    fn intermediate_regular_file_is_rejected() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("node_modules"), "{}").unwrap();
        assert!(!authentic_descendant_file(
            &fixture.0,
            "node_modules/pkg/package.json"
        ));
    }

    #[test]
    fn directory_leaf_is_rejected() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("node_modules/pkg/package.json")).unwrap();
        assert!(!authentic_descendant_file(
            &fixture.0,
            "node_modules/pkg/package.json"
        ));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn intermediate_symlink_is_rejected() {
        let fixture = Fixture::new();
        fixture.package();
        let target = fixture.0.join("real_modules");
        fs::rename(fixture.0.join("node_modules"), &target).unwrap();
        let link = fixture.0.join("node_modules");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).expect("create directory symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link).expect(
            "directory symlink creation is required; enable Developer Mode or symlink privileges",
        );
        assert!(!authentic_descendant_file(
            &fixture.0,
            "node_modules/pkg/package.json"
        ));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn leaf_symlink_is_rejected() {
        let fixture = Fixture::new();
        let relative = fixture.package();
        let target = fixture.0.join("real_package.json");
        let link = fixture.0.join(&relative);
        fs::rename(&link, &target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).expect("create file symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).expect(
            "file symlink creation is required; enable Developer Mode or symlink privileges",
        );
        assert!(!authentic_descendant_file(&fixture.0, relative));
    }
}
