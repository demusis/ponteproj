//! Library settings: small named texts that belong to the library itself and
//! so reach every computer and phone that syncs it (today: the holidays, key
//! [`HOLIDAYS`]).
//!
//! A setting is a `key` and a text `value`, kept sealed like every payload.
//! It travels as the `SaveSetting` change and in the snapshot's `settings`,
//! in every mode (host and clients, shared folder, phone). A save carries no
//! `expected_version`: two computers that edit the same setting do not make
//! a conflict to resolve by hand, the change applied last wins, in the order
//! the sync already gives to every change (the main computer's arrival order;
//! in the shared folders, the changes of the other copies first and then this
//! computer's own). An empty value is a value: "cleared on purpose", which is
//! not the same as "never set" (`None`).
use crate::{
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    storage::Store,
    work_templates::version_key,
};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// The state and the days without work (`holidays::HolidayConfig::to_text`).
pub const HOLIDAYS: &str = "holidays";

/// Longest value, in bytes.
pub const MAX_VALUE: usize = 64 * 1024;
/// Most settings a library may have.
pub const MAX_SETTINGS: usize = 50;

/// One setting as it travels in a snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check(key: &str, value: &str) -> Result<()> {
    ensure!(valid_key(key), "O nome da configuração é inválido.");
    ensure!(
        value.len() <= MAX_VALUE,
        "O valor da configuração é grande demais."
    );
    ensure!(
        !value.contains('\0'),
        "O valor da configuração tem um caractere inválido."
    );
    Ok(())
}

fn read_one(conn: &Connection, key: &str) -> Result<Option<String>> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT payload FROM library_settings WHERE key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    blob.map(|blob| Ok(String::from_utf8(unseal(&blob)?)?))
        .transpose()
}

fn label(key: &str) -> &'static str {
    match key {
        HOLIDAYS => "os feriados e dias sem expediente",
        _ => "uma configuração do acervo",
    }
}

impl Store {
    pub(crate) fn ensure_library_settings(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS library_settings(
                key TEXT PRIMARY KEY,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// The value of a setting; `None` when it was never set.
    pub fn library_setting(&self, key: &str) -> Result<Option<String>> {
        self.cached(&format!("setting\u{1}{key}"), || read_one(&self.conn, key))
    }

    /// Sets a setting for the whole library. Nothing is done (no change is
    /// recorded or sent) when it already has this value. In a team mode the
    /// change goes through the main computer or the queue, like any other.
    pub fn set_library_setting(&mut self, key: &str, value: &str) -> Result<()> {
        check(key, value)?;
        if read_one(&self.conn, key)?.as_deref() == Some(value) {
            return Ok(());
        }
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_settings(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveSetting {
                    key: key.to_owned(),
                    value: value.to_owned(),
                },
            )?;
            return Ok(());
        }
        self.save_setting_applied(key, value)
    }

    /// Applies a change that came from another computer (or that is made
    /// here with no team mode): written as it is, whatever was there.
    pub(crate) fn save_setting_applied(&mut self, key: &str, value: &str) -> Result<()> {
        check(key, value)?;
        let actor = self.active_actor();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known: i64 = tx.query_row("SELECT count(*) FROM library_settings", [], |r| r.get(0))?;
        let exists = read_one(&tx, key)?.is_some();
        ensure!(
            exists || (known as usize) < MAX_SETTINGS,
            "A biblioteca já tem configurações demais."
        );
        tx.execute(
            "INSERT INTO library_settings(key,payload,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",
            params![key, seal(value.as_bytes())?, timestamp],
        )?;
        record_mutation(
            &tx,
            &actor,
            "setting",
            version_key(key),
            None,
            "save_setting",
            None,
            &format!("Alterou {}", label(key)),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn export_library_settings(&self) -> Result<Vec<SettingRow>> {
        let mut statement = self
            .conn
            .prepare("SELECT key,payload,updated_at FROM library_settings ORDER BY key")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (key, blob, updated_at) in rows {
            out.push(SettingRow {
                key,
                value: String::from_utf8(unseal(&blob)?)?,
                updated_at,
            });
        }
        Ok(out)
    }
}

/// Writes the table from a snapshot (the table was emptied by the caller),
/// re-sealing the values for this Windows account. A row that cannot be
/// stored (a key or value this build does not accept) is skipped.
pub(crate) fn apply_settings(tx: &Connection, settings: &[SettingRow]) -> Result<()> {
    for row in settings.iter().take(MAX_SETTINGS) {
        if check(&row.key, &row.value).is_err() {
            continue;
        }
        tx.execute(
            "INSERT OR REPLACE INTO library_settings(key,payload,updated_at) VALUES(?1,?2,?3)",
            params![row.key, seal(row.value.as_bytes())?, row.updated_at],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        (dir, store)
    }

    #[test]
    fn a_setting_is_unset_then_set_then_cleared_on_purpose() {
        let (_dir, mut store) = store();
        assert_eq!(store.library_setting(HOLIDAYS).unwrap(), None);
        store.set_library_setting(HOLIDAYS, "uf SP\n").unwrap();
        assert_eq!(
            store.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("uf SP\n")
        );
        // Cleared on purpose is a value, not "never set".
        store.set_library_setting(HOLIDAYS, "").unwrap();
        assert_eq!(
            store.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("")
        );
    }

    #[test]
    fn saving_the_same_value_records_nothing() {
        let (_dir, mut store) = store();
        store.set_library_setting(HOLIDAYS, "uf SP\n").unwrap();
        let revision = store.cabinet_revision().unwrap();
        store.set_library_setting(HOLIDAYS, "uf SP\n").unwrap();
        assert_eq!(store.cabinet_revision().unwrap(), revision);
        store.set_library_setting(HOLIDAYS, "uf RJ\n").unwrap();
        assert_eq!(store.cabinet_revision().unwrap(), revision + 1);
    }

    #[test]
    fn keys_and_values_are_checked() {
        let (_dir, mut store) = store();
        assert!(store.set_library_setting("", "x").is_err());
        assert!(store.set_library_setting("Holidays", "x").is_err());
        assert!(store.set_library_setting("a b", "x").is_err());
        assert!(store.set_library_setting(HOLIDAYS, "a\0b").is_err());
        let big = "x".repeat(MAX_VALUE + 1);
        assert!(store.set_library_setting(HOLIDAYS, &big).is_err());
        assert_eq!(store.library_setting(HOLIDAYS).unwrap(), None);
    }

    #[test]
    fn the_settings_travel_in_a_snapshot() {
        let (_dir_a, mut a) = store();
        let (_dir_b, mut b) = store();
        a.set_library_setting(HOLIDAYS, "uf MG\nanual 05-02 Recesso\n")
            .unwrap();
        let snapshot = a.export_snapshot().unwrap();
        assert_eq!(snapshot.settings.len(), 1);
        b.apply_snapshot(&snapshot, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        assert_eq!(
            b.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("uf MG\nanual 05-02 Recesso\n")
        );
    }

    #[test]
    fn a_snapshot_from_a_ponte_before_the_settings_keeps_this_librarys() {
        let (_dir_a, mut a) = store();
        a.set_library_setting(HOLIDAYS, "uf SP\n").unwrap();
        let mut old = a.export_snapshot().unwrap();
        old.settings.clear();
        old.schema_version = crate::intranet::SETTINGS_SCHEMA - 1;
        a.apply_snapshot(&old, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        assert_eq!(
            a.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("uf SP\n")
        );
    }
}
