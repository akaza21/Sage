//! Run it with `npm run test:native-lane` on Windows; see
//! docs/work-items/BROWSER-FIRST-CLASS/native-lane.md.

#[cfg(windows)]
mod annotate;
#[cfg(windows)]
mod device;
#[cfg(windows)]
mod report;
#[cfg(windows)]
mod s10_tls;
#[cfg(windows)]
mod s11_downloads;
#[cfg(windows)]
mod s1_javascript;
#[cfg(windows)]
mod s1_popups;
#[cfg(windows)]
mod s3_ipc;
#[cfg(windows)]
mod s8_devtools;
#[cfg(windows)]
mod site;
#[cfg(windows)]
mod tls;

#[cfg(not(windows))]
fn main() {
    eprintln!("[native-lane] RESULT: FAIL (the native lane runs only on Windows with WebView2)");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    std::process::exit(lane::run());
}

#[cfg(windows)]
mod lane {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use sage_lib::native_lane::{self, Lane};

    use crate::{
        annotate, device, report::Report, s1_popups, s3_ipc, s8_devtools, s10_tls, s11_downloads,
        site::Site,
    };

    const LANE_BUDGET: Duration = Duration::from_secs(300);
    const RELEASE_BUDGET: Duration = Duration::from_secs(30);

    type Case = fn(&Lane, &Site, &mut Report);
    const CASES: &[(&str, Case)] = &[
        ("s3", s3_ipc::run),
        ("s10", s10_tls::run),
        ("s8", s8_devtools::run),
        ("s1", s1_popups::run),
        ("s11", s11_downloads::run),
        ("annotate", annotate::run),
        ("device", device::run),
    ];

    pub fn run() -> i32 {
        let root = std::env::temp_dir().join(format!("sage-native-lane-{}", std::process::id()));
        if let Err(error) = fresh_directory(&root) {
            println!(
                "[native-lane] RESULT: FAIL (profile directory {}: {error})",
                root.display()
            );
            return 1;
        }
        println!("[native-lane] WebView2 profile: {}", root.display());
        let done = Arc::new(AtomicBool::new(false));
        watchdog(done.clone(), root.display().to_string());
        let code = native_lane::run(&root, |lane| {
            let mut report = Report::default();
            match Site::start() {
                Ok(site) => {
                    for (name, case) in CASES {
                        let outcome =
                            catch_unwind(AssertUnwindSafe(|| case(lane, &site, &mut report)));
                        if outcome.is_err() {
                            report.check(&format!("{name}.completed"), false, "the case panicked");
                        }
                        let closed = lane.close_all();
                        report.check(
                            &format!("{name}.cleanup"),
                            closed.is_ok() && lane.tabs().is_empty(),
                            format!("closed every tab: {closed:?}"),
                        );
                    }
                }
                Err(error) => {
                    report.check("fixture", false, error);
                }
            }
            report.finish()
        });
        done.store(true, Ordering::SeqCst);
        let code = code.unwrap_or_else(|error| {
            println!("[native-lane] RESULT: FAIL (Sage did not start: {error})");
            1
        });
        match release(&root) {
            Ok(waited) => {
                println!(
                    "[native-lane] profile removed after {} ms",
                    waited.as_millis()
                );
                code
            }
            Err(error) => {
                println!(
                    "[native-lane] RESULT: FAIL (WebView2 profile {} left behind: {error})",
                    root.display()
                );
                code.max(1)
            }
        }
    }

    fn fresh_directory(root: &Path) -> std::io::Result<()> {
        if root.exists() {
            std::fs::remove_dir_all(root)?;
        }
        std::fs::create_dir_all(root)
    }

    fn release(root: &Path) -> Result<Duration, String> {
        let started = Instant::now();
        loop {
            match std::fs::remove_dir_all(root) {
                Ok(()) => return Ok(started.elapsed()),
                Err(error) if started.elapsed() >= RELEASE_BUDGET => {
                    return Err(error.to_string());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }

    fn watchdog(done: Arc<AtomicBool>, root: String) {
        std::thread::spawn(move || {
            let deadline = Instant::now() + LANE_BUDGET;
            while Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            println!(
                "[native-lane] RESULT: FAIL (exceeded {} s; WebView2 profile {root} may remain)",
                LANE_BUDGET.as_secs()
            );
            std::process::exit(3);
        });
    }
}
