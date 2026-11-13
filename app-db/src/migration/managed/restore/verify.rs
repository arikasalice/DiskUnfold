//! Verification of a restored database: identity, integrity and logical content digest.
use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags, types::ValueRef};

use crate::{APPLICATION_ID, BUSY_TIMEOUT, migration::MigrationError};

pub(super) fn verify_restored(
    path: &Path,
    version: u32,
    expected: &str,
) -> Result<(), MigrationError> {
    {
        let live = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        live.busy_timeout(BUSY_TIMEOUT)?;
        let id: u32 = live.pragma_query_value(None, "application_id", |row| row.get(0))?;
        if id != APPLICATION_ID || user_version(&live)? != version {
            return Err(MigrationError::Integrity);
        }
        let mut statement = live.prepare("PRAGMA integrity_check")?;
        let mut rows = statement.query([])?;
        let first: String = rows.next()?.ok_or(MigrationError::Integrity)?.get(0)?;
        if first != "ok" || rows.next()?.is_some() {
            return Err(MigrationError::Integrity);
        }
        if logical_digest(&live)? != expected {
            return Err(MigrationError::Integrity);
        }
    }
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    match fs::metadata(PathBuf::from(wal)) {
        Ok(meta) if meta.len() > 0 => Err(MigrationError::Integrity),
        _ => Ok(()),
    }
}
pub(super) fn user_version(connection: &Connection) -> Result<u32, MigrationError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Hashes the schema text and every row of every table in scan order.
///
/// Two databases with the same digest hold the same schema and the same rows.
pub(in crate::migration::managed) fn logical_digest(
    connection: &Connection,
) -> Result<String, MigrationError> {
    let mut hasher = blake3::Hasher::new();
    let mut tables = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT type, name, tbl_name, coalesce(sql, '') FROM sqlite_schema ORDER BY type, name",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            for index in 0..4 {
                let text: String = row.get(index)?;
                hasher.update(&(text.len() as u64).to_le_bytes());
                hasher.update(text.as_bytes());
            }
            if row.get::<_, String>(0)? == "table" {
                tables.push(row.get::<_, String>(1)?);
            }
        }
    }
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let mut statement = connection.prepare(&format!("SELECT * FROM \"{quoted}\""))?;
        let columns = statement.column_count();
        hasher.update(table.as_bytes());
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            hasher.update(b"R");
            for index in 0..columns {
                match row.get_ref(index)? {
                    ValueRef::Null => hasher.update(&[0]),
                    ValueRef::Integer(v) => hasher.update(&[1]).update(&v.to_le_bytes()),
                    ValueRef::Real(v) => hasher.update(&[2]).update(&v.to_bits().to_le_bytes()),
                    ValueRef::Text(b) => hasher
                        .update(&[3])
                        .update(&(b.len() as u64).to_le_bytes())
                        .update(b),
                    ValueRef::Blob(b) => hasher
                        .update(&[4])
                        .update(&(b.len() as u64).to_le_bytes())
                        .update(b),
                };
            }
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}
