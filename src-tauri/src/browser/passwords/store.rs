use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use uuid::Uuid;

use super::{
    Cancellation, VaultError, VaultResult,
    cipher::{Binding, VaultKey},
    secret::Secret,
};

/// The most rows one overview returns; search narrows a larger vault.
pub(super) const MAX_LISTED: u32 = 200;
/// The most passwords the vault holds, so every search and list scan stays
/// bounded.
pub(super) const MAX_STORED: u32 = 50_000;
const SCHEMA_VERSION: i64 = 1;
const NOW_MS: &str = "CAST(unixepoch('subsec') * 1000 AS INTEGER)";

/// One saved password as the Passwords page lists it: never the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialSummary {
    pub(crate) id: String,
    pub(crate) origin: String,
    pub(crate) username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialPage {
    pub(crate) total: u32,
    pub(crate) matching: u32,
    pub(crate) entries: Vec<CredentialSummary>,
}

/// Where a saved password came from.
#[derive(Debug, Clone, Copy)]
pub(super) enum Source {
    Csv,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
        }
    }
}

/// A credential read from an import, before it is sealed.
pub(super) struct NewCredential {
    pub(super) origin: String,
    pub(super) username: String,
    pub(super) password: Secret,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeCounts {
    pub(crate) added: u32,
    pub(crate) updated: u32,
    pub(crate) unchanged: u32,
}

/// The vault's SQLite file. Usernames and origins are plain text so search
/// works without the key; passwords are sealed blobs only.
pub(super) struct Store(Connection);

impl Store {
    pub(super) fn open(path: &Path) -> VaultResult<Self> {
        Self::migrate(Connection::open(path)?)
    }

    #[cfg(test)]
    pub(super) fn in_memory() -> VaultResult<Self> {
        Self::migrate(Connection::open_in_memory()?)
    }

    fn migrate(connection: Connection) -> VaultResult<Self> {
        // Deleted rows are overwritten, so a removed password's ciphertext
        // does not linger in free pages.
        connection.pragma_update(None, "secure_delete", true)?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(VaultError::Invalid(
                "Saved passwords were written by a newer version of Sage.",
            ));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS credentials (
               id TEXT PRIMARY KEY,
               origin TEXT NOT NULL CHECK (length(CAST(origin AS BLOB)) BETWEEN 1 AND 4096),
               username TEXT NOT NULL CHECK (length(CAST(username AS BLOB)) <= 4096),
               password BLOB NOT NULL,
               source TEXT NOT NULL,
               created_at_ms INTEGER NOT NULL,
               updated_at_ms INTEGER NOT NULL,
               UNIQUE (origin, username)
             ) STRICT;
             PRAGMA user_version = 1;",
        )?;
        Ok(Self(connection))
    }

    pub(super) fn is_empty(&self) -> VaultResult<bool> {
        Ok(!self
            .0
            .query_row("SELECT EXISTS (SELECT 1 FROM credentials)", [], |row| {
                row.get::<_, bool>(0)
            })?)
    }

    /// Rows whose site or username contains `query`, ignoring ASCII case as
    /// SQLite's `lower` does.
    pub(super) fn page(&self, query: &str) -> VaultResult<CredentialPage> {
        let needle = query.trim().to_ascii_lowercase();
        let filter = "instr(lower(origin), ?1) > 0 OR instr(lower(username), ?1) > 0";
        let total = self
            .0
            .query_row("SELECT count(*) FROM credentials", [], |row| row.get(0))?;
        let matching = self.0.query_row(
            &format!("SELECT count(*) FROM credentials WHERE {filter}"),
            [&needle],
            |row| row.get(0),
        )?;
        let mut statement = self.0.prepare(&format!(
            "SELECT id, origin, username FROM credentials
             WHERE {filter} ORDER BY origin, username, id LIMIT ?2"
        ))?;
        let entries = statement
            .query_map(params![needle, MAX_LISTED], |row| {
                Ok(CredentialSummary {
                    id: row.get(0)?,
                    origin: row.get(1)?,
                    username: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(CredentialPage {
            total,
            matching,
            entries,
        })
    }

    pub(super) fn contains(&self, id: &str) -> VaultResult<bool> {
        Ok(self.0.query_row(
            "SELECT EXISTS (SELECT 1 FROM credentials WHERE id = ?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )?)
    }

    pub(super) fn password(&self, key: &VaultKey, id: &str) -> VaultResult<Secret> {
        let (origin, username, sealed) = self
            .0
            .query_row(
                "SELECT origin, username, password FROM credentials WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(VaultError::NotFound)?;
        key.open(
            &Binding {
                id,
                origin: &origin,
                username: &username,
            },
            &sealed,
        )
    }

    /// Adds every credential in one transaction. An existing row for the same
    /// site and username takes the newer password; nothing is written unless
    /// every credential seals.
    pub(super) fn merge(
        &mut self,
        key: &VaultKey,
        source: Source,
        credentials: &[NewCredential],
        cancel: &impl Cancellation,
    ) -> VaultResult<MergeCounts> {
        let transaction = self.0.transaction()?;
        let mut counts = MergeCounts::default();
        for credential in credentials {
            // Dropping the transaction rolls every row back.
            if cancel.cancelled() {
                return Err(VaultError::Cancelled);
            }
            let existing = transaction
                .query_row(
                    "SELECT id, password FROM credentials WHERE origin = ?1 AND username = ?2",
                    params![credential.origin, credential.username],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()?;
            match existing {
                Some((id, sealed)) => {
                    let binding = Binding {
                        id: &id,
                        origin: &credential.origin,
                        username: &credential.username,
                    };
                    if key.open(&binding, &sealed)?.expose() == credential.password.expose() {
                        counts.unchanged += 1;
                        continue;
                    }
                    write_password(&transaction, key, &binding, &credential.password)?;
                    counts.updated += 1;
                }
                None => {
                    let id = Uuid::new_v4().to_string();
                    let binding = Binding {
                        id: &id,
                        origin: &credential.origin,
                        username: &credential.username,
                    };
                    let sealed = key.seal(&binding, &credential.password)?;
                    transaction.execute(
                        &format!(
                            "INSERT INTO credentials
                               (id, origin, username, password, source, created_at_ms, updated_at_ms)
                             VALUES (?1, ?2, ?3, ?4, ?5, {NOW_MS}, {NOW_MS})"
                        ),
                        params![id, credential.origin, credential.username, sealed, source.as_str()],
                    )?;
                    counts.added += 1;
                }
            }
        }
        let stored: u32 =
            transaction.query_row("SELECT count(*) FROM credentials", [], |row| row.get(0))?;
        if stored > MAX_STORED {
            return Err(VaultError::Import(
                "Sage keeps at most 50,000 saved passwords, and this import would pass that.",
            ));
        }
        cancel.commit(|| {
            transaction.commit()?;
            Ok(())
        })?;
        Ok(counts)
    }

    /// Changes a row's username and, when given, its password. A new username
    /// re-seals the password, because the seal is bound to it.
    pub(super) fn update(
        &mut self,
        key: &VaultKey,
        id: &str,
        username: &str,
        password: Option<&Secret>,
    ) -> VaultResult<()> {
        let current = match password {
            Some(_) => None,
            None => Some(self.password(key, id)?),
        };
        let origin: String = self.0.query_row(
            "SELECT origin FROM credentials WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let transaction = self.0.transaction()?;
        let taken = transaction
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM credentials WHERE origin = ?1 AND username = ?2 AND id <> ?3)",
                params![origin, username, id],
                |row| row.get::<_, bool>(0),
            )?;
        if taken {
            return Err(VaultError::Duplicate);
        }
        transaction.execute(
            "UPDATE credentials SET username = ?2 WHERE id = ?1",
            params![id, username],
        )?;
        write_password(
            &transaction,
            key,
            &Binding {
                id,
                origin: &origin,
                username,
            },
            password.or(current.as_ref()).ok_or(VaultError::NotFound)?,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn delete(&self, id: &str) -> VaultResult<()> {
        match self
            .0
            .execute("DELETE FROM credentials WHERE id = ?1", [id])?
        {
            0 => Err(VaultError::NotFound),
            _ => Ok(()),
        }
    }

    pub(super) fn delete_all(&self) -> VaultResult<()> {
        self.0.execute("DELETE FROM credentials", [])?;
        Ok(())
    }
}

fn write_password(
    transaction: &Transaction<'_>,
    key: &VaultKey,
    binding: &Binding<'_>,
    password: &Secret,
) -> VaultResult<()> {
    let sealed = key.seal(binding, password)?;
    transaction.execute(
        &format!("UPDATE credentials SET password = ?2, updated_at_ms = {NOW_MS} WHERE id = ?1"),
        params![binding.id, sealed],
    )?;
    Ok(())
}
