//! Prints each check as it is decided, so a hung or killed run still shows
//! every result up to that point.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Report {
    passed: usize,
    failed: Vec<String>,
}

impl Report {
    pub fn section(&mut self, title: &str) {
        println!("\n[native-lane] {title}");
    }

    pub fn check(&mut self, id: &str, pass: bool, evidence: impl AsRef<str>) -> bool {
        let evidence = evidence.as_ref().replace(['\r', '\n'], " ");
        println!("  {} {id}: {evidence}", if pass { "PASS" } else { "FAIL" });
        if pass {
            self.passed += 1;
        } else {
            self.failed.push(id.to_owned());
        }
        pass
    }

    pub fn finish(&self) -> i32 {
        println!(
            "\n[native-lane] {} passed, {} failed",
            self.passed,
            self.failed.len()
        );
        if self.failed.is_empty() {
            println!("[native-lane] RESULT: PASS");
            0
        } else {
            println!("[native-lane] failed: {}", self.failed.join(", "));
            println!("[native-lane] RESULT: FAIL");
            1
        }
    }
}

pub fn wait_for<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Lets late effects land before asserting that something never happened.
pub fn quiet_period() {
    std::thread::sleep(Duration::from_millis(2_000));
}

pub fn sage_sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    let mut directories = vec![root.join("src"), root.to_owned()];
    while let Some(directory) = directories.pop() {
        let top = directory == root;
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if top {
                if name.starts_with("tauri.") && name.ends_with(".conf.json") {
                    files.push(path);
                }
            } else if path.is_dir() {
                directories.push(path);
            } else if name.ends_with(".rs") {
                files.push(path);
            }
        }
    }
    files
        .into_iter()
        .map(|file| {
            let source = std::fs::read_to_string(&file).unwrap_or_default();
            (file, source)
        })
        .collect()
}
