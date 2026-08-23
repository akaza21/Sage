use std::fs;

use super::{
    discovery::{PreviewDiscoveryRequest, discover},
    health,
    origin::loopback_origin,
};
use crate::project_registry::RootRecord;

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
    let request = PreviewDiscoveryRequest {
        project_id: "project-preview".into(),
        project_generation: 0,
        expected_transition_epoch: 0,
    };
    let response = discover(&request, Vec::new()).unwrap();
    assert!(response.targets.is_empty());
    assert!(!response.truncated);
}

#[test]
fn discovery_returns_only_curated_scripts_without_package_paths() {
    let root = std::env::temp_dir().join(format!("sage-preview-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::create_dir_all(root.join("node_modules").join("ignored")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"vite","test":"vitest"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("nested").join("package.json"),
        r#"{"scripts":{"start":"next start","build":"next build"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("node_modules")
            .join("ignored")
            .join("package.json"),
        r#"{"scripts":{"dev":"must not appear"}}"#,
    )
    .unwrap();
    let request = PreviewDiscoveryRequest {
        project_id: "project-preview".into(),
        project_generation: 2,
        expected_transition_epoch: 4,
    };
    let response = discover(
        &request,
        vec![RootRecord {
            id: "root-preview".into(),
            project_id: request.project_id.clone(),
            absolute_path: root.clone(),
            ordinal: 0,
        }],
    )
    .unwrap();
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
    for index in 0..9 {
        fs::create_dir_all(root.join(format!("package-{index}"))).unwrap();
        fs::write(
            root.join(format!("package-{index}")).join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
    }
    let request = PreviewDiscoveryRequest {
        project_id: "project-preview".into(),
        project_generation: 2,
        expected_transition_epoch: 4,
    };
    let response = discover(
        &request,
        vec![RootRecord {
            id: "root-preview".into(),
            project_id: request.project_id.clone(),
            absolute_path: root.clone(),
            ordinal: 0,
        }],
    )
    .unwrap();
    // Target identity is root-scoped, so the nine manifests collapse to the
    // single curated `dev` target while the package traversal cap still
    // fail-closes through `truncated`.
    assert_eq!(response.targets.len(), 1);
    assert!(response.truncated);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn https_health_is_fail_closed() {
    assert!(!health::get(Some("https://127.0.0.1:443")));
}
