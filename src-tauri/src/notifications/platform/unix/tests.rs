
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn unavailable_platform_releases_every_dispatch_slot() {
    const CHILD: &str = "SAGE_NOTIFICATION_FAILURE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Isolate the bus address in a child, never mutate the parallel
        // test process's environment or require a user's notification daemon.
        let output=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","notifications::platform::unix::tests::unavailable_platform_releases_every_dispatch_slot","--nocapture"])
                .env(CHILD,"1")
                .env("DBUS_SESSION_BUS_ADDRESS",format!("unix:path=/tmp/sage-no-bus-{}",uuid::Uuid::new_v4()))
                .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return;
    }
    fn memory(name: &str) -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse().ok())
            })
            .unwrap()
    }
    let baseline = memory("VmRSS:");
    let count = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let permit = Permit::acquire(&count).unwrap();
            std::thread::Builder::new()
                .stack_size(512 * 1024)
                .spawn(move || {
                    let _permit = permit;
                    assert!(
                        deliver(Event::Completed, || panic!(
                            "failed dispatch cannot activate a chat"
                        ))
                        .is_err()
                    );
                })
                .unwrap()
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(count.load(Ordering::Acquire), 0);
    eprintln!(
        "notification platform failure probe: workers=4; baseline_kib={baseline}; peak_kib={}; settled_kib={}",
        memory("VmHWM:"),
        memory("VmRSS:")
    );
}
