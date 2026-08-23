use std::{
    fs,
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewScriptName, PreviewTargetRef, discover},
    health, lifecycle,
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
fn discovery_truncates_a_directory_at_the_entry_cap() {
    let root =
        std::env::temp_dir().join(format!("sage-preview-entry-cap-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    for index in 0..256 {
        fs::write(root.join(format!("file-{index}")), "").unwrap();
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
    assert!(response.targets.is_empty());
    assert!(response.truncated);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stops_before_reserve_cancel_each_matching_start() {
    let target = PreviewTargetRef {
        project_id: format!("project-preview-{}", uuid::Uuid::new_v4()),
        project_generation: 2,
        expected_transition_epoch: 4,
        root_id: "root-preview".into(),
        script_name: PreviewScriptName::Dev,
    };
    let second_target = PreviewTargetRef {
        root_id: "second-root-preview".into(),
        script_name: PreviewScriptName::Start,
        ..target.clone()
    };

    let request = PreviewDiscoveryRequest {
        project_id: target.project_id.clone(),
        project_generation: target.project_generation,
        expected_transition_epoch: target.expected_transition_epoch,
    };
    lifecycle::replace_discovered(
        &request,
        vec![
            (target.clone(), std::env::temp_dir()),
            (second_target.clone(), std::env::temp_dir()),
        ],
    );
    assert_eq!(
        lifecycle::stop(&target).unwrap().ref_,
        target,
        "an early stop must be accepted so it can cancel the in-flight command"
    );
    assert_eq!(
        lifecycle::stop(&second_target).unwrap().ref_,
        second_target,
        "one target's cancellation must not overwrite another's"
    );
    assert!(
        lifecycle::reserve_start(target).is_err(),
        "the first later start must fail closed before it can spawn a child"
    );
    assert!(
        lifecycle::reserve_start(second_target).is_err(),
        "the second later start must fail closed before it can spawn a child"
    );
}

#[test]
fn same_epoch_rediscovery_stops_a_starting_child_that_cannot_be_promoted() {
    let root = std::env::temp_dir().join(format!("sage-preview-start-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("server.js"),
        r#"const fs = require("fs");
const http = require("http");
const server = http.createServer((_, response) => response.end());
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  fs.writeFileSync("preview-port", String(port));
  console.log(`http://127.0.0.1:${port}`);
});
"#,
    )
    .unwrap();
    let target = PreviewTargetRef {
        project_id: format!("project-preview-{}", uuid::Uuid::new_v4()),
        project_generation: 2,
        expected_transition_epoch: 4,
        root_id: "root-preview".into(),
        script_name: PreviewScriptName::Dev,
    };
    let request = PreviewDiscoveryRequest {
        project_id: target.project_id.clone(),
        project_generation: target.project_generation,
        expected_transition_epoch: target.expected_transition_epoch,
    };
    lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
    let start = lifecycle::reserve_start(target.clone()).unwrap();
    let port_file = root.join("preview-port");
    let deadline = Instant::now() + Duration::from_secs(5);
    let port = loop {
        if let Ok(value) = fs::read_to_string(&port_file) {
            if let Ok(port) = value.parse() {
                break port;
            }
        }
        assert!(
            Instant::now() < deadline,
            "preview child did not publish its listening port"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let rediscovered_target = PreviewTargetRef {
        root_id: "new-root-preview".into(),
        ..target.clone()
    };
    lifecycle::replace_discovered(&request, vec![(rediscovered_target, root.clone())]);

    assert!(matches!(
        lifecycle::finish_start(start),
        Err(super::error::PreviewError::Conflict {
            resource: "activePreview",
            ..
        })
    ));

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                break;
            }
            Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("preview child still owns port {port}: {error}"),
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn https_health_is_fail_closed() {
    assert!(!health::get(Some("https://127.0.0.1:443")));
}
