use std::time::{Duration, Instant};

use tokio::{sync::oneshot, time::sleep};

use super::*;

#[test]
fn language_fixture_test_child() {
    crate::workspace::search::language_fixture::run_test_stdio_child();
}

fn request(document: &str) -> LanguageFixtureRequest {
    LanguageFixtureRequest {
        resource: ResourceRef {
            root_id: "root-a".into(),
            relative_path: "src/example.ts".into(),
        },
        generation: 1,
        document: document.into(),
    }
}

#[test]
fn language_fixture_process_surfaces_one_typescript_diagnostic() {
    let report = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (_keep_alive, receiver) = oneshot::channel();
        run_language_fixture(request("const value = SAGE_TS_DIAGNOSTIC;"), receiver)
            .await
            .unwrap()
    });

    assert_eq!(report.resource.root_id, "root-a");
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].severity, "error");
}

#[test]
fn language_fixture_cancel_reaps_the_child() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let started = Instant::now();
        let (cancel, receiver) = oneshot::channel();
        let cancellation = tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            let _ = cancel.send(());
        });

        let error = run_language_fixture(request("SAGE_TS_FIXTURE_SLEEP"), receiver)
            .await
            .unwrap_err();

        cancellation.await.unwrap();
        assert!(error.contains("cancelled"));
        assert!(!error.contains("cleanup remains contained"));
        assert!(started.elapsed() < Duration::from_secs(1));
    });
}

#[test]
fn language_fixture_overrun_or_crash_fails_closed() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for document in ["SAGE_TS_FIXTURE_OVERRUN", "SAGE_TS_FIXTURE_CRASH"] {
            let (_keep_alive, receiver) = oneshot::channel();
            let error = run_language_fixture(request(document), receiver)
                .await
                .unwrap_err();
            assert!(error.contains("failed") || error.contains("exceeded"));
        }
    });
}

#[test]
fn language_session_generations_remain_unique_across_resource_switches() {
    let first = register_session(request("").resource).unwrap().0;
    let second = register_session(ResourceRef {
        root_id: "root-a".into(),
        relative_path: "src/other.ts".into(),
    })
    .unwrap()
    .0;
    let third = register_session(request("").resource).unwrap().0;

    assert!(first < second && second < third);
    clear_session(&request("").resource, third);
}

#[test]
fn language_session_is_at_most_one_process() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (cancel, receiver) = oneshot::channel();
        let first = tokio::spawn(run_language_fixture(
            request("SAGE_TS_FIXTURE_SLEEP"),
            receiver,
        ));
        for _ in 0..20 {
            if active_language_processes() == 1 {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(active_language_processes(), 1);
        let (_keep_alive, receiver) = oneshot::channel();
        let second = tokio::spawn(run_language_fixture(request(""), receiver));

        sleep(Duration::from_millis(50)).await;
        assert!(!second.is_finished());
        let _ = cancel.send(());
        assert!(first.await.unwrap().unwrap_err().contains("cancelled"));
        assert!(second.await.unwrap().is_ok());
    });
}
