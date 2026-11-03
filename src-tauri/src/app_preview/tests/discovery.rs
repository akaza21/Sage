use std::fs;

use super::super::{
    discovery::{
        PreviewDiscoveryRequest, PreviewDiscoveryResponse, PreviewScriptName, PreviewTargetRef,
        discover, discover_uninstalled,
    },
    origin::loopback_origin,
};
use super::discovery_request;
use crate::{acp::limits::PREVIEW_LIMITS, project_registry::RootRecord};

#[test]
fn loopback_origins_allow_http_and_https_literals_only() {
    assert_eq!(
        loopback_origin("http://127.0.0.1:5173").unwrap(),
        "http://127.0.0.1:5173"
    );
    assert_eq!(
        loopback_origin("https://[::1]:3443").unwrap(),
        "https://[::1]:3443"
    );
    for rejected in [
        "http://example.com:80",
        "http://localhost:3000",
        "file:///tmp/index.html",
        "http://127.0.0.1",
    ] {
        assert!(
            loopback_origin(rejected).is_err(),
            "{rejected} must be rejected"
        );
    }
}

#[test]
fn discovery_rejects_no_roots_without_exposing_paths() {
    let response = discover(&discovery_request("project-preview", 0, 0), Vec::new()).unwrap();
    assert!(response.targets.is_empty());
    assert!(!response.truncated);
}

#[test]
fn discovery_returns_only_curated_scripts_without_package_paths() {
    let root = std::env::temp_dir().join(format!("sage-preview-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::create_dir_all(root.join("node_modules/ignored")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"vite","test":"vitest"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("nested/package.json"),
        r#"{"scripts":{"start":"next start","build":"next build"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("node_modules/ignored/package.json"),
        r#"{"scripts":{"dev":"must not appear"}}"#,
    )
    .unwrap();
    let request = discovery_request("project-preview", 4, 4);
    let response = discover(&request, vec![root_record(&request, &root)]).unwrap();
    assert_eq!(response.targets.len(), 2);
    assert!(
        response
            .targets
            .iter()
            .all(|target| matches!(target.script_name.as_str(), "dev" | "start"))
    );
    assert!(
        !serde_json::to_string(&response)
            .unwrap()
            .contains(&*root.to_string_lossy())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_stops_at_the_package_cap() {
    let root = std::env::temp_dir().join(format!("sage-preview-cap-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    for index in 0..PREVIEW_LIMITS.packages_per_root {
        fs::create_dir_all(root.join(format!("package-{index}"))).unwrap();
        fs::write(
            root.join(format!("package-{index}/package.json")),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
    }
    let request = discovery_request("project-preview", 4, 4);
    assert!(
        !discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::create_dir_all(root.join("package-plus-one")).unwrap();
    fs::write(
        root.join("package-plus-one/package.json"),
        r#"{"scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    assert!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_truncates_a_directory_at_the_entry_cap() {
    let root =
        std::env::temp_dir().join(format!("sage-preview-entry-cap-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    for index in 0..PREVIEW_LIMITS.directory_entries {
        fs::write(root.join(format!("file-{index}")), "").unwrap();
    }
    let request = discovery_request("project-preview", 4, 4);
    assert!(
        !discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::write(root.join("file-plus-one"), "").unwrap();
    assert!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_admits_exact_directory_count_and_truncates_plus_one() {
    let root = std::env::temp_dir().join(format!(
        "sage-preview-directory-cap-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    for index in 0..(PREVIEW_LIMITS.directories_per_root - 1) {
        fs::create_dir(root.join(format!("directory-{index:03}"))).unwrap();
    }
    let request = discovery_request("project-preview", 4, 4);
    assert!(
        !discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::create_dir(root.join("directory-plus-one")).unwrap();
    assert!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .truncated
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_admits_exact_manifest_and_script_bytes_and_rejects_plus_one() {
    let root =
        std::env::temp_dir().join(format!("sage-preview-byte-caps-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let request = discovery_request("project-preview", 4, 4);
    let exact_script = "x".repeat(PREVIEW_LIMITS.script_bytes as usize);
    let mut exact_manifest = format!(r#"{{"scripts":{{"dev":"{exact_script}"}}}}"#);
    exact_manifest
        .push_str(&" ".repeat(PREVIEW_LIMITS.manifest_bytes as usize - exact_manifest.len()));
    fs::write(root.join("package.json"), &exact_manifest).unwrap();
    assert_eq!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .targets
            .len(),
        1
    );
    fs::write(root.join("package.json"), format!("{exact_manifest} ")).unwrap();
    assert!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .targets
            .is_empty()
    );
    fs::write(
        root.join("package.json"),
        format!(
            r#"{{"scripts":{{"dev":"{}"}}}}"#,
            "x".repeat(PREVIEW_LIMITS.script_bytes as usize + 1)
        ),
    )
    .unwrap();
    assert!(
        discover(&request, vec![root_record(&request, &root)])
            .unwrap()
            .targets
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

fn root_record(
    request: &super::super::discovery::PreviewDiscoveryRequest,
    root: &std::path::Path,
) -> RootRecord {
    RootRecord {
        id: "root-preview".into(),
        project_id: request.project_id.as_str().into(),
        absolute_path: root.to_owned(),
        ordinal: 0,
    }
}

#[test]
fn discovery_skips_malformed_nested_manifest_and_keeps_root_scripts() {
    let root = std::env::temp_dir().join(format!("sage-preview-bad-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("fixtures/bad")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"vite","start":"vite preview"}}"#,
    )
    .unwrap();
    fs::write(root.join("fixtures/bad/package.json"), "{broken").unwrap();
    let request = discovery_request("project-preview", 4, 4);
    let (response, mappings) =
        discover_uninstalled(&request, vec![root_record(&request, &root)]).unwrap();
    assert!(response.truncated);
    assert_eq!(
        response
            .targets
            .iter()
            .map(|t| t.script_name.as_str())
            .collect::<Vec<_>>(),
        ["dev", "start"]
    );
    assert!(mappings.iter().all(|(_, path)| path == &root));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_skips_malformed_root_manifest() {
    let root = std::env::temp_dir().join(format!("sage-preview-bad-root-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{broken").unwrap();
    let request = discovery_request("project-preview", 4, 4);
    let (response, mappings) =
        discover_uninstalled(&request, vec![root_record(&request, &root)]).unwrap();
    assert!(response.truncated);
    assert!(response.targets.is_empty());
    assert!(mappings.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn discovery_skips_unreadable_nested_manifest() {
    use std::os::unix::fs::PermissionsExt;
    // Root can read mode-000 files, so it cannot exercise this failure.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let root =
        std::env::temp_dir().join(format!("sage-preview-unreadable-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
    let manifest = root.join("nested/package.json");
    fs::write(&manifest, r#"{"scripts":{"start":"vite preview"}}"#).unwrap();
    let permissions = fs::metadata(&manifest).unwrap().permissions();
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o000)).unwrap();
    let request = discovery_request("project-preview", 4, 4);
    let result = discover_uninstalled(&request, vec![root_record(&request, &root)]);
    fs::set_permissions(&manifest, permissions).unwrap();
    let (response, mappings) = result.unwrap();
    assert!(response.truncated);
    assert_eq!(response.targets.len(), 1);
    assert_eq!(response.targets[0].script_name.as_str(), "dev");
    assert_eq!(mappings[0].1, root);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_visits_siblings_in_byte_order_and_prefers_the_root() {
    for (names, first) in [(["b", "a"], "a"), (["a", "b"], "a"), (["a", "B"], "B")] {
        let root =
            std::env::temp_dir().join(format!("sage-preview-order-{}", uuid::Uuid::new_v4()));
        for name in names {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(
                root.join(name).join("package.json"),
                r#"{"scripts":{"dev":"vite","start":"vite preview"}}"#,
            )
            .unwrap();
        }
        let request = discovery_request("project-preview", 4, 4);
        assert_repeated_discovery(&request, &root, Some(&root.join(first)), false);
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"root","start":"root preview"}}"#,
        )
        .unwrap();
        assert_repeated_discovery(&request, &root, Some(&root), false);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn discovery_skips_the_entire_overflowing_directory() {
    for nested in [false, true] {
        for names in [["b", "a"], ["a", "b"]] {
            let root = std::env::temp_dir()
                .join(format!("sage-preview-overflow-{}", uuid::Uuid::new_v4()));
            let directory = if nested {
                root.join("overflow")
            } else {
                root.clone()
            };
            for name in names {
                fs::create_dir_all(directory.join(name)).unwrap();
                fs::write(
                    directory.join(name).join("package.json"),
                    r#"{"scripts":{"dev":"vite","start":"vite preview"}}"#,
                )
                .unwrap();
            }
            // Two package directories plus 255 files exceed the 256-entry cap.
            // Even omitting one raw entry would leave at least one package.
            for index in 0..PREVIEW_LIMITS.directory_entries - 1 {
                fs::write(directory.join(format!("file-{index:03}")), "").unwrap();
            }
            if nested {
                fs::write(
                    root.join("package.json"),
                    r#"{"scripts":{"dev":"root","start":"root preview"}}"#,
                )
                .unwrap();
            }
            let request = discovery_request("project-preview", 4, 4);
            assert_repeated_discovery(&request, &root, nested.then_some(root.as_path()), true);
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn discovery_keeps_root_manifest_when_root_listing_overflows() {
    let root = std::env::temp_dir().join(format!(
        "sage-preview-root-overflow-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
    for index in 0..PREVIEW_LIMITS.directory_entries {
        fs::write(root.join(format!("file-{index:03}")), "").unwrap();
    }
    let request = discovery_request("project-preview", 4, 4);
    let expected = PreviewTargetRef {
        project_id: request.project_id.clone(),
        project_generation: request.project_generation,
        expected_transition_epoch: request.expected_transition_epoch,
        chat_id: request.chat_id.clone(),
        session_id: request.session_id.clone(),
        runtime_epoch: request.runtime_epoch,
        root_id: crate::acp::contract::SageRootId::parse("root-preview").unwrap(),
        script_name: PreviewScriptName::Dev,
    };
    let expected_response = PreviewDiscoveryResponse {
        project_id: request.project_id.clone(),
        transition_epoch: request.expected_transition_epoch,
        targets: vec![expected.clone()],
        truncated: true,
    };
    let expected_mappings = vec![(expected, root.clone())];
    for _ in 0..3 {
        let (response, mappings) =
            discover_uninstalled(&request, vec![root_record(&request, &root)]).unwrap();
        assert_eq!(response, expected_response);
        assert_eq!(mappings, expected_mappings);
    }
    fs::remove_dir_all(root).unwrap();
}

fn assert_repeated_discovery(
    request: &PreviewDiscoveryRequest,
    root: &std::path::Path,
    package: Option<&std::path::Path>,
    truncated: bool,
) {
    let expected_mappings: Vec<_> = package
        .into_iter()
        .flat_map(|package| {
            [PreviewScriptName::Dev, PreviewScriptName::Start].map(|script_name| {
                (
                    PreviewTargetRef {
                        project_id: request.project_id.clone(),
                        project_generation: request.project_generation,
                        expected_transition_epoch: request.expected_transition_epoch,
                        chat_id: request.chat_id.clone(),
                        session_id: request.session_id.clone(),
                        runtime_epoch: request.runtime_epoch,
                        root_id: crate::acp::contract::SageRootId::parse("root-preview").unwrap(),
                        script_name,
                    },
                    package.to_owned(),
                )
            })
        })
        .collect();
    let expected_response = PreviewDiscoveryResponse {
        project_id: request.project_id.clone(),
        transition_epoch: request.expected_transition_epoch,
        targets: expected_mappings
            .iter()
            .map(|(target, _)| target.clone())
            .collect(),
        truncated,
    };
    for _ in 0..3 {
        let (response, mappings) =
            discover_uninstalled(request, vec![root_record(request, root)]).unwrap();
        assert_eq!(response, expected_response);
        assert_eq!(mappings, expected_mappings);
    }
}
