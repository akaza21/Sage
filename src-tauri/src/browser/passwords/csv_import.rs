//! Bounded reading of password-manager CSV exports (Chrome, Edge, Brave,
//! Firefox, Safari, Bitwarden, 1Password, KeePassXC and LastPass headers).
//! `csv-core` parses into fixed buffers (and drops a leading UTF-8 BOM), so no record grows past
//! `MAX_RECORD_BYTES` however the file is shaped, and every plaintext buffer
//! is wiped when it drops.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use csv_core::{ReadRecordResult, Reader};
use zeroize::Zeroizing;

use super::{
    Cancellation, MAX_PASSWORD_BYTES, MAX_USERNAME_BYTES, VaultError, VaultResult,
    origin::credential_origin, secret::Secret, store::NewCredential,
};

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 8 * 1024;
pub(super) const MAX_CREDENTIALS: usize = 10_000;
const MAX_FIELDS: usize = 64;
const READ_BYTES: usize = 64 * 1024;

const URL_HEADERS: &[&str] = &["url", "login_uri", "website"];
const USERNAME_HEADERS: &[&str] = &["username", "login_username"];
const PASSWORD_HEADERS: &[&str] = &["password", "login_password"];
/// Firefox marks HTTP authentication logins with a realm; they belong to the
/// browser's auth prompt, not to a page form, so they are skipped.
const HTTP_REALM_HEADER: &str = "httprealm";

/// The credentials a CSV holds, and how many of its rows could not be used.
pub(super) struct ParsedCsv {
    pub(super) credentials: Vec<NewCredential>,
    pub(super) skipped: u32,
}

pub(super) fn read_file(path: &Path, cancel: &dyn Cancellation) -> VaultResult<ParsedCsv> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(too_large());
    }
    read(file, cancel)
}

/// Reads `source`, checking `cancel` before every read of at most
/// `READ_BYTES`.
pub(super) fn read(source: impl Read, cancel: &dyn Cancellation) -> VaultResult<ParsedCsv> {
    let mut records = Records::new(source, cancel);
    let columns = match records.next()? {
        Some(Row::Fields(header)) => Columns::from_header(&header)?,
        Some(Row::Oversized) | None => return Err(no_columns()),
    };
    let mut parsed = ParsedCsv {
        credentials: Vec::new(),
        skipped: 0,
    };
    while let Some(row) = records.next()? {
        match row {
            Row::Fields(fields) => match columns.credential(&fields) {
                Some(_) if parsed.credentials.len() == MAX_CREDENTIALS => {
                    return Err(VaultError::Import(
                        "This file holds more than 10,000 passwords, which is more than Sage imports at once.",
                    ));
                }
                Some(credential) => parsed.credentials.push(credential),
                None => parsed.skipped += 1,
            },
            Row::Oversized => parsed.skipped += 1,
        }
    }
    Ok(parsed)
}

fn too_large() -> VaultError {
    VaultError::Import("This file is larger than 16 MB, which is more than Sage imports.")
}

fn no_columns() -> VaultError {
    VaultError::Import("This file has no url, username and password columns.")
}

struct Columns {
    url: usize,
    username: usize,
    password: usize,
    http_realm: Option<usize>,
}

impl Columns {
    fn from_header(header: &Fields<'_>) -> VaultResult<Self> {
        let names: Vec<String> = header
            .iter()
            .map(|name| String::from_utf8_lossy(name).trim().to_ascii_lowercase())
            .collect();
        let find = |candidates: &[&str]| {
            names
                .iter()
                .position(|name| candidates.contains(&name.as_str()))
        };
        Ok(Self {
            url: find(URL_HEADERS).ok_or_else(no_columns)?,
            username: find(USERNAME_HEADERS).ok_or_else(no_columns)?,
            password: find(PASSWORD_HEADERS).ok_or_else(no_columns)?,
            http_realm: find(&[HTTP_REALM_HEADER]),
        })
    }

    fn credential(&self, fields: &Fields<'_>) -> Option<NewCredential> {
        let text = |index| std::str::from_utf8(fields.get(index)?).ok();
        if self
            .http_realm
            .and_then(|index| fields.get(index))
            .is_some_and(|realm| !realm.is_empty())
        {
            return None;
        }
        let origin = credential_origin(text(self.url)?)?;
        let username = text(self.username)?;
        let password = text(self.password)?;
        if username.len() > MAX_USERNAME_BYTES
            || password.is_empty()
            || password.len() > MAX_PASSWORD_BYTES
        {
            return None;
        }
        Some(NewCredential {
            origin,
            username: username.to_owned(),
            password: Secret::new(password.to_owned()),
        })
    }
}

/// One parsed record: its bytes and where each field ends.
struct Fields<'a> {
    bytes: &'a [u8],
    ends: &'a [usize],
}

impl<'a> Fields<'a> {
    fn get(&self, index: usize) -> Option<&'a [u8]> {
        let end = *self.ends.get(index)?;
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous]);
        self.bytes.get(start..end)
    }

    fn iter(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        (0..self.ends.len()).filter_map(|index| self.get(index))
    }
}

enum Row<'a> {
    Fields(Fields<'a>),
    /// A record past `MAX_RECORD_BYTES` or `MAX_FIELDS`, consumed but not kept.
    Oversized,
}

struct Records<'c, R> {
    source: R,
    cancel: &'c dyn Cancellation,
    parser: Reader,
    input: Zeroizing<Vec<u8>>,
    start: usize,
    end: usize,
    read_total: u64,
    record: Zeroizing<Vec<u8>>,
    ends: [usize; MAX_FIELDS],
}

impl<'c, R: Read> Records<'c, R> {
    fn new(source: R, cancel: &'c dyn Cancellation) -> Self {
        Self {
            source,
            cancel,
            parser: Reader::new(),
            input: Zeroizing::new(vec![0; READ_BYTES]),
            start: 0,
            end: 0,
            read_total: 0,
            record: Zeroizing::new(vec![0; MAX_RECORD_BYTES]),
            ends: [0; MAX_FIELDS],
        }
    }

    /// Reads the next chunk; an empty chunk tells the parser the file ended.
    fn refill(&mut self) -> VaultResult<()> {
        if self.cancel.cancelled() {
            return Err(VaultError::Cancelled);
        }
        let read = loop {
            match self.source.read(&mut self.input) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => break result?,
            }
        };
        self.read_total += read as u64;
        if self.read_total > MAX_FILE_BYTES {
            return Err(too_large());
        }
        self.start = 0;
        self.end = read;
        Ok(())
    }

    fn next(&mut self) -> VaultResult<Option<Row<'_>>> {
        let (mut written, mut fields, mut consumed) = (0, 0, 0);
        let mut oversized = false;
        loop {
            if self.start == self.end {
                self.refill()?;
            }
            let (result, read, wrote, ended) = self.parser.read_record(
                &self.input[self.start..self.end],
                &mut self.record[written..],
                &mut self.ends[fields..],
            );
            self.start += read;
            consumed += read;
            written += wrote;
            fields += ended;
            if consumed > MAX_RECORD_BYTES {
                oversized = true;
            }
            match result {
                ReadRecordResult::InputEmpty => {}
                ReadRecordResult::OutputFull | ReadRecordResult::OutputEndsFull => {
                    // Keep consuming the record without keeping any of it.
                    oversized = true;
                    (written, fields) = (0, 0);
                }
                ReadRecordResult::Record if oversized => return Ok(Some(Row::Oversized)),
                ReadRecordResult::Record => {
                    return Ok(Some(Row::Fields(Fields {
                        bytes: &self.record[..written],
                        ends: &self.ends[..fields],
                    })));
                }
                ReadRecordResult::End => return Ok(None),
            }
        }
    }
}
