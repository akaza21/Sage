use std::{fs, path::PathBuf};

use uuid::Uuid;

use super::{ImportedFile, delete_if_same};

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-imported-file-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A newer export saved over the name after the check, before the removal,
/// survives: only the file that passed the check is deleted.
#[cfg(unix)]
#[test]
fn a_file_saved_over_the_name_during_the_delete_survives() {
    let folder = Folder::new();
    let path = folder.file("passwords.csv", "imported");
    let (_, imported) = ImportedFile::open(path.clone()).unwrap();
    delete_if_same(&path, |file| {
        imported.check(file)?;
        let newer = folder.file("newer.csv", "newer export");
        fs::rename(newer, &path).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(folder.names(), ["passwords.csv"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), "newer export");
}

/// A file that fails the check goes back under its own name.
#[cfg(unix)]
#[test]
fn a_file_that_fails_the_check_is_put_back() {
    let folder = Folder::new();
    let path = folder.file("passwords.csv", "imported");
    let (_, imported) = ImportedFile::open(path.clone()).unwrap();
    let newer = folder.file("newer.csv", "newer export");
    fs::rename(newer, &path).unwrap();
    assert!(matches!(
        imported.delete(),
        Err(super::VaultError::ImportedFileChanged)
    ));
    assert_eq!(folder.names(), ["passwords.csv"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), "newer export");
}

/// From the check to the delete, no program can open the file for writing,
/// and a program already writing to it stops the delete.
#[cfg(windows)]
#[test]
fn writers_are_shut_out_from_the_check_to_the_delete() {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    let folder = Folder::new();
    let path = folder.file("passwords.csv", "imported");
    let (_, imported) = ImportedFile::open(path.clone()).unwrap();

    let writer = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let refused = imported.delete().unwrap_err();
    assert!(matches!(
        refused,
        super::VaultError::Io(ref error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
    ));
    drop(writer);
    assert_eq!(fs::read_to_string(&path).unwrap(), "imported");

    delete_if_same(&path, |file| {
        imported.check(file)?;
        let opened = fs::OpenOptions::new().append(true).open(&path);
        assert_eq!(
            opened.unwrap_err().raw_os_error(),
            Some(ERROR_SHARING_VIOLATION)
        );
        Ok(())
    })
    .unwrap();
    assert!(folder.names().is_empty());
}
