//! Built-in browser site permissions, saved per site (origin). One row per
//! origin and permission kind holds the user's Allow or Block answer; a
//! missing row means "Ask". The table is bounded and closed: the kinds and
//! decisions are fixed by CHECK constraints, and the browser module admits
//! every origin before it reaches this store.

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::{
    browser::site_permissions::{Decision, PermissionKind},
    error::{AppError, AppResult},
};

/// At most this many saved answers across every site.
pub const MAX_SITE_PERMISSIONS: usize = 256;
/// The longest origin the store admits; `scheme://host:port` with a full
/// DNS name fits well inside it.
pub const MAX_SITE_ORIGIN_BYTES: usize = 512;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_site_permissions (
           origin TEXT NOT NULL CHECK (length(origin) BETWEEN 8 AND 512),
           kind TEXT NOT NULL CHECK (kind IN
             ('camera', 'microphone', 'geolocation', 'notifications', 'clipboardRead')),
           decision TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
           PRIMARY KEY (origin, kind)
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

fn kind_text(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Camera => "camera",
        PermissionKind::Microphone => "microphone",
        PermissionKind::Geolocation => "geolocation",
        PermissionKind::Notifications => "notifications",
        PermissionKind::ClipboardRead => "clipboardRead",
    }
}

fn kind_from_text(text: &str) -> Option<PermissionKind> {
    Some(match text {
        "camera" => PermissionKind::Camera,
        "microphone" => PermissionKind::Microphone,
        "geolocation" => PermissionKind::Geolocation,
        "notifications" => PermissionKind::Notifications,
        "clipboardRead" => PermissionKind::ClipboardRead,
        _ => return None,
    })
}

fn decision_text(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
    }
}

fn decision_from_text(text: &str) -> Option<Decision> {
    match text {
        "allow" => Some(Decision::Allow),
        "deny" => Some(Decision::Deny),
        _ => None,
    }
}

impl SettingsStore {
    /// Every saved answer, at most [`MAX_SITE_PERMISSIONS`] rows, ordered by
    /// origin then kind.
    pub fn browser_site_permissions(&self) -> AppResult<Vec<(String, PermissionKind, Decision)>> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT origin, kind, decision FROM browser_site_permissions
             ORDER BY origin, kind LIMIT ?1",
        )?;
        let rows = statement.query_map(params![MAX_SITE_PERMISSIONS as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut saved = Vec::new();
        for row in rows {
            let (origin, kind, decision) = row?;
            // The CHECK constraints admit only known values; anything else is
            // skipped rather than trusted.
            if let (Some(kind), Some(decision)) =
                (kind_from_text(&kind), decision_from_text(&decision))
            {
                saved.push((origin, kind, decision));
            }
        }
        Ok(saved)
    }

    /// Saves one answer. A new row past [`MAX_SITE_PERMISSIONS`] is refused
    /// inside the same transaction; replacing an existing row always works.
    pub fn put_browser_site_permission(
        &self,
        origin: &str,
        kind: PermissionKind,
        decision: Decision,
    ) -> AppResult<()> {
        if origin.len() > MAX_SITE_ORIGIN_BYTES {
            return Err(AppError::InvalidRequest("site origin is too long".into()));
        }
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM browser_site_permissions WHERE origin = ?1 AND kind = ?2",
                params![origin, kind_text(kind)],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM browser_site_permissions",
                [],
                |row| row.get(0),
            )?;
            if count >= MAX_SITE_PERMISSIONS as i64 {
                return Err(AppError::ResourceLimit(format!(
                    "Sage keeps at most {MAX_SITE_PERMISSIONS} site permissions"
                )));
            }
        }
        transaction.execute(
            "INSERT INTO browser_site_permissions(origin, kind, decision) VALUES (?1, ?2, ?3)
             ON CONFLICT(origin, kind) DO UPDATE SET decision = excluded.decision",
            params![origin, kind_text(kind), decision_text(decision)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Forgets one answer, so the site asks again. Reports whether a row went.
    pub fn delete_browser_site_permission(
        &self,
        origin: &str,
        kind: PermissionKind,
    ) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(connection.execute(
            "DELETE FROM browser_site_permissions WHERE origin = ?1 AND kind = ?2",
            params![origin, kind_text(kind)],
        )? > 0)
    }

    /// Forgets every answer.
    pub fn clear_browser_site_permissions(&self) -> AppResult<()> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute("DELETE FROM browser_site_permissions", [])?;
        Ok(())
    }

    /// Makes every later site-permission read and write fail, so callers can
    /// prove they fail closed.
    #[cfg(test)]
    pub(crate) fn break_browser_site_permissions_for_tests(&self) {
        self.0
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE browser_site_permissions;")
            .unwrap();
    }
}

#[cfg(test)]
#[path = "browser_site_permissions_tests.rs"]
mod tests;
