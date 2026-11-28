//! Pairing without a copied link. Each bridge run records its port and
//! single-use pairing code in the disposable profile; the Vite dev server
//! serves that record same-origin (`scripts/devBridgeRun.ts`) so the dev tab
//! pairs itself, and a tab whose backend restarted can see that a new run is
//! ready. Pairing forgets the code in the record as it consumes it in memory.

use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Kept beside the durable store: the disposable profile root admits no
/// other entries.
pub(super) const FILE_NAME: &str = "dev-bridge-run.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    run: &'a str,
    bridge_port: u16,
    /// Lets the dev server tell a run whose backend has exited (a rebuild)
    /// from one another tab has paired with.
    pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

pub(super) struct RunRecord {
    path: PathBuf,
    run: String,
    port: u16,
}

impl RunRecord {
    pub fn publish(directory: &Path, port: u16, code: &str) -> io::Result<Self> {
        let mut run = super::auth::random_secret()?;
        run.truncate(32);
        let record = Self {
            path: directory.join(FILE_NAME),
            run,
            port,
        };
        record.write(Some(code))?;
        Ok(record)
    }

    /// Rewrites the record without its code once pairing has used it.
    pub fn consume(&self) -> io::Result<()> {
        self.write(None)
    }

    fn write(&self, code: Option<&str>) -> io::Result<()> {
        let body = serde_json::to_vec(&Record {
            run: &self.run,
            bridge_port: self.port,
            pid: std::process::id(),
            code,
        })?;
        // Readers see either the old record or the new one, never a torn write.
        let staged = self.path.with_extension("json.tmp");
        match fs::remove_file(&staged) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let mut file = private_file().open(&staged)?;
        file.write_all(&body)?;
        drop(file);
        fs::rename(&staged, &self.path)
    }
}

/// The record holds a pairing code: on unix only its owner may read it.
fn private_file() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_bridge::auth::{Credentials, Pairing};

    fn read(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn a_run_publishes_its_code_and_pairing_removes_it_from_the_record() {
        let directory = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-bridge-run-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let code = "a".repeat(64);
        let record = RunRecord::publish(&directory, 34567, &code).unwrap();
        let path = directory.join(FILE_NAME);
        let published = read(&path);
        assert_eq!(published["bridgePort"], 34567);
        assert_eq!(published["pid"], std::process::id());
        assert_eq!(published["code"], code.as_str());
        let run = published["run"].as_str().unwrap().to_owned();
        assert_eq!(run.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let mut pairing = Pairing::with_run_record(code.clone(), record);
        pairing
            .authenticate(Credentials {
                pairing_code: Some(code.clone()),
                session: None,
            })
            .unwrap();
        let consumed = read(&path);
        assert_eq!(consumed["run"], run.as_str());
        assert_eq!(consumed["bridgePort"], 34567);
        assert!(consumed.get("code").is_none());
        assert!(!directory.join(format!("{FILE_NAME}.tmp")).exists());

        RunRecord::publish(&directory, 34568, &"b".repeat(64)).unwrap();
        assert_ne!(read(&path)["run"], run.as_str());
        fs::remove_dir_all(&directory).unwrap();
    }
}
