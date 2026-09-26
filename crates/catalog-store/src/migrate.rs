//! Forward-only migrations tracked in `PRAGMA user_version`.

use rusqlite::Connection;

use crate::StoreError;

/// `(version, file name, SQL)`, in order. Files live in `migrations/` and are
/// never edited once released; a change is a new file.
pub const MIGRATIONS: &[(u32, &str, &str)] = &[(1, "001_init.sql", include_str!("../migrations/001_init.sql"))];

pub const LATEST_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].0;

pub(crate) fn user_version(conn: &Connection) -> Result<u32, StoreError> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    u32::try_from(v).map_err(|_| StoreError::Corrupt {
        table: "user_version",
        detail: v.to_string(),
    })
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let current = user_version(conn)?;
    if current > LATEST_VERSION {
        return Err(StoreError::TooNew {
            found: current,
            supported: LATEST_VERSION,
        });
    }
    for (version, _, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i64::from(*version))?;
        tx.commit()?;
    }
    Ok(())
}
