//! Scoped observation of real Git children for process-budget tests.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

type Counts = Mutex<HashMap<String, usize>>;
type Observers = Mutex<HashMap<PathBuf, Weak<Counts>>>;
fn observers() -> &'static Observers {
    static OBSERVERS: OnceLock<Observers> = OnceLock::new();
    OBSERVERS.get_or_init(Mutex::default)
}

pub struct GitCommandCounts {
    root: PathBuf,
    counts: Arc<Counts>,
}

impl GitCommandCounts {
    pub fn observe(root: &Path) -> Self {
        let root = root.canonicalize().unwrap();
        let counts = Arc::new(Mutex::default());
        assert!(
            observers()
                .lock()
                .unwrap()
                .insert(root.clone(), Arc::downgrade(&counts))
                .is_none()
        );
        Self { root, counts }
    }

    pub fn count(&self, command: &str) -> usize {
        self.counts
            .lock()
            .unwrap()
            .get(command)
            .copied()
            .unwrap_or(0)
    }
}

impl Drop for GitCommandCounts {
    fn drop(&mut self) {
        observers().lock().unwrap().remove(&self.root);
    }
}

pub(super) fn record(root: &Path, args: &[&str]) {
    let observers = observers().lock().unwrap();
    if let Some(counts) = observers.get(root).and_then(Weak::upgrade) {
        let mut counts = counts.lock().unwrap();
        assert!(counts.len() < 64, "Git command vocabulary is bounded");
        *counts
            .entry(args.first().unwrap_or(&"").to_string())
            .or_default() += 1;
    }
}
