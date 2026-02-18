use rusqlite::{params, Connection, Result};
use std::collections::HashSet;
use crate::storage::Database;

pub struct CloudDatabase {
    conn: Connection,
}

fn apply_cipher_pragmas(conn: &Connection, password: &str) -> Result<()> {
    conn.pragma_update(None, "key", password)?;
    conn.pragma_update(None, "kdf_iter", 256000)?;
    conn.pragma_update(None, "cipher_page_size", 4096)?;
    conn.pragma_update(None, "cipher_memory_security", true)?;
    conn.pragma_update(None, "cipher_hmac_algorithm", "HMAC_SHA512")?;
    conn.pragma_update(None, "cipher_kdf_algorithm", "PBKDF2_HMAC_SHA512")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

impl CloudDatabase {
    pub fn new(path: &str, password: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        apply_cipher_pragmas(&conn, password)?;
        let db = CloudDatabase { conn };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS clips (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                owner_process_name TEXT,
                foreground_window_title TEXT,
                exe_path TEXT,
                content_hash TEXT,
                is_sensitive INTEGER DEFAULT 0,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS formats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clip_id INTEGER,
                format_id INTEGER,
                format_name TEXT,
                data BLOB,
                FOREIGN KEY(clip_id) REFERENCES clips(id) ON DELETE CASCADE
            )",
            [],
        )?;
        Ok(())
    }

    pub fn copy_clip_from(&self, hash: &str, source: &Database) -> Result<()> {
        let exists: u32 = self.conn.query_row(
            "SELECT COUNT(1) FROM clips WHERE content_hash = ?",
            [hash],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Ok(());
        }

        let (owner, fg_title, exe_path) = source.get_clip_meta(hash)?;
        let payloads = source.get_clip_payloads(hash)?;

        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO clips (owner_process_name, foreground_window_title, exe_path, content_hash)
             VALUES (?, ?, ?, ?)",
            params![owner, fg_title, exe_path, hash],
        )?;
        let clip_id = tx.last_insert_rowid();

        for p in payloads {
            tx.execute(
                "INSERT INTO formats (clip_id, format_id, format_name, data) VALUES (?, ?, ?, ?)",
                params![clip_id, p.format_id, p.format_name, p.data],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn get_synced_hashes(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT content_hash FROM clips")?;
        let hashes = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<HashSet<String>, _>>()?;
        Ok(hashes)
    }
}