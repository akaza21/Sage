use std::{fs, path::PathBuf};

use uuid::Uuid;

use super::*;

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-resource-map-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn root(id: &str, path: PathBuf) -> ConfiguredResourceRoot {
    ConfiguredResourceRoot {
        root_id: SageRootId::parse(id).unwrap(),
        canonical_native_root: path,
    }
}

fn native(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

#[test]
fn duplicate_relative_paths_keep_root_qualified_identity() {
    let tree = TempTree::new();
    let first = tree.path().join("first");
    let second = tree.path().join("second");
    fs::create_dir_all(first.join("src")).unwrap();
    fs::create_dir_all(second.join("src")).unwrap();
    fs::write(first.join("src/shared.rs"), "first").unwrap();
    fs::write(second.join("src/shared.rs"), "second").unwrap();
    let first = first.canonicalize().unwrap();
    let second = second.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([
        root("root-first", first.clone()),
        root("root-second", second.clone()),
    ])
    .unwrap();

    let mapped_first = mapper
        .try_map_existing(&native(&first.join("src/shared.rs")))
        .unwrap();
    let mapped_second = mapper
        .try_map_existing(&native(&second.join("src/shared.rs")))
        .unwrap();

    assert_eq!(mapped_first.root_id.as_str(), "root-first");
    assert_eq!(mapped_second.root_id.as_str(), "root-second");
    assert_eq!(mapped_first.relative_path.as_str(), "src/shared.rs");
    assert_eq!(mapped_second.relative_path.as_str(), "src/shared.rs");
}

#[test]
fn rejects_missing_foreign_and_non_absolute_paths() {
    let tree = TempTree::new();
    let project = tree.path().join("project");
    let foreign = tree.path().join("foreign.txt");
    fs::create_dir(&project).unwrap();
    fs::write(&foreign, "outside").unwrap();
    let project = project.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([root("root-one", project.clone())]).unwrap();

    assert_eq!(
        mapper.try_map_existing("relative/file.rs"),
        Err(ResourceMapError::PathNotAbsolute)
    );
    assert_eq!(
        mapper.try_map_existing(&native(&project.join("missing.rs"))),
        Err(ResourceMapError::PathMissing)
    );
    assert_eq!(
        mapper.try_map_existing(&native(&foreign.canonicalize().unwrap())),
        Err(ResourceMapError::ForeignPath)
    );
}

#[test]
fn rejects_a_resource_with_nested_root_membership() {
    let tree = TempTree::new();
    let outer = tree.path().join("outer");
    let inner = outer.join("inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("file.rs"), "content").unwrap();
    let outer = outer.canonicalize().unwrap();
    let inner = inner.canonicalize().unwrap();
    let mapper =
        NativeResourceMapper::new([root("root-outer", outer), root("root-inner", inner.clone())])
            .unwrap();

    assert_eq!(
        mapper.try_map_existing(&native(&inner.join("file.rs"))),
        Err(ResourceMapError::AmbiguousRoot)
    );
}

#[test]
fn shared_mapper_preserves_each_typed_unavailable_reason() {
    let tree = TempTree::new();
    let outer = tree.path().join("outer");
    let inner = outer.join("inner");
    let foreign = tree.path().join("foreign.rs");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("file.rs"), "content").unwrap();
    fs::write(&foreign, "foreign").unwrap();
    let outer = outer.canonicalize().unwrap();
    let inner = inner.canonicalize().unwrap();
    let foreign = foreign.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([
        root("root-outer", outer.clone()),
        root("root-inner", inner.clone()),
    ])
    .unwrap();
    let separator = std::path::MAIN_SEPARATOR;

    assert_eq!(
        mapper.map_existing(&native(&inner.join("file.rs"))),
        Err(UnavailableResourceReason::AmbiguousRoot)
    );
    assert_eq!(
        mapper.map_existing(&native(&foreign)),
        Err(UnavailableResourceReason::Foreign)
    );
    assert_eq!(
        mapper.map_existing(&native(&outer.join("missing.rs"))),
        Err(UnavailableResourceReason::Missing)
    );
    assert_eq!(
        mapper.map_existing(&format!(
            "{}{separator}.{separator}inner{separator}file.rs",
            native(&outer)
        )),
        Err(UnavailableResourceReason::Noncanonical)
    );
    assert_eq!(
        mapper.map_existing("relative.rs"),
        Err(UnavailableResourceReason::Noncanonical)
    );
}

#[test]
fn rejects_dot_repeated_separator_and_symlink_alias_ingress() {
    let tree = TempTree::new();
    let project = tree.path().join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/file.rs"), "content").unwrap();
    let project = project.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([root("root-one", project.clone())]).unwrap();
    let prefix = native(&project);
    let separator = std::path::MAIN_SEPARATOR;

    for alias in [
        format!("{prefix}{separator}src{separator}.{separator}file.rs"),
        format!("{prefix}{separator}src{separator}{separator}file.rs"),
    ] {
        assert_eq!(
            mapper.try_map_existing(&alias),
            Err(ResourceMapError::PathNotCanonical),
            "alias should be rejected: {alias}"
        );
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(project.join("src/file.rs"), project.join("alias.rs")).unwrap();
        assert_eq!(
            mapper.try_map_existing(&native(&project.join("alias.rs"))),
            Err(ResourceMapError::PathNotCanonical)
        );

        let outside = tree.path().join("outside.rs");
        fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, project.join("escape.rs")).unwrap();
        assert_eq!(
            mapper.try_map_existing(&native(&project.join("escape.rs"))),
            Err(ResourceMapError::PathNotCanonical)
        );
    }
}

#[test]
fn accepts_exact_root_limit_and_rejects_limit_plus_one() {
    let tree = TempTree::new();
    let mut roots = Vec::new();
    for index in 0..=MAX_PROJECT_ROOTS {
        let path = tree.path().join(format!("root-{index}"));
        fs::create_dir(&path).unwrap();
        roots.push(root(
            &format!("root-id-{index}"),
            path.canonicalize().unwrap(),
        ));
    }

    let exact = roots
        .drain(..MAX_PROJECT_ROOTS)
        .collect::<Vec<ConfiguredResourceRoot>>();
    assert!(NativeResourceMapper::new(exact).is_ok());
    assert_eq!(
        NativeResourceMapper::new(roots).err(),
        None,
        "remaining one root alone remains valid"
    );

    let mut plus_one = Vec::new();
    for index in 0..=MAX_PROJECT_ROOTS {
        let path = tree.path().join(format!("root-{index}"));
        plus_one.push(root(
            &format!("root-plus-{index}"),
            path.canonicalize().unwrap(),
        ));
    }
    assert_eq!(
        NativeResourceMapper::new(plus_one).err(),
        Some(ResourceMapError::TooManyRoots)
    );
}

#[test]
fn absolute_path_byte_limit_is_checked_before_filesystem_ingress() {
    let tree = TempTree::new();
    let mapper = NativeResourceMapper::new([root("root-one", tree.path().to_owned())]).unwrap();
    let limit = LIMITS.wire.absolute_path_bytes_native_only as usize;
    #[cfg(not(windows))]
    let exact = format!("/{}", "a".repeat(limit - 1));
    #[cfg(windows)]
    let exact = format!("C:\\{}", "a".repeat(limit - 3));
    let plus_one = format!("{exact}a");

    assert_eq!(exact.len(), limit);
    assert_eq!(validate_path_bytes(&exact, true), Ok(()));
    assert_eq!(
        validate_path_bytes(&plus_one, true),
        Err(ResourceMapError::RootPathBytes)
    );
    assert_eq!(
        mapper.try_map_existing(&exact),
        Err(ResourceMapError::PathMissing)
    );
    assert_eq!(
        mapper.try_map_existing(&plus_one),
        Err(ResourceMapError::PathBytes)
    );
    assert_eq!(
        mapper.map_existing(&plus_one),
        Err(UnavailableResourceReason::Unsupported)
    );
}

#[cfg(windows)]
#[test]
fn windows_case_alias_maps_to_actual_root_relative_casing() {
    let tree = TempTree::new();
    let project = tree.path().join("ProjectCase");
    fs::create_dir_all(project.join("SourceCase")).unwrap();
    fs::write(project.join("SourceCase/FileCase.RS"), "content").unwrap();
    let project = project.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([root("root-one", project.clone())]).unwrap();
    let alias = native(&project.join("SourceCase/FileCase.RS")).to_lowercase();

    let mapped = mapper.try_map_existing(&alias).unwrap();
    assert_eq!(mapped.relative_path.as_str(), "SourceCase/FileCase.RS");
}

#[cfg(not(windows))]
#[test]
fn case_variant_is_not_an_alias_on_case_sensitive_test_filesystems() {
    let tree = TempTree::new();
    let project = tree.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("ActualCase.rs"), "content").unwrap();
    let project = project.canonicalize().unwrap();
    let mapper = NativeResourceMapper::new([root("root-one", project.clone())]).unwrap();

    assert_eq!(
        mapper.try_map_existing(&native(&project.join("actualcase.rs"))),
        Err(ResourceMapError::PathMissing)
    );
    assert_eq!(
        mapper
            .try_map_existing(&native(&project.join("ActualCase.rs")))
            .unwrap()
            .relative_path
            .as_str(),
        "ActualCase.rs"
    );
}
