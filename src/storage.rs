use rusqlite::{params, Connection, Result};

use crate::models::{ ClipboardPayload, ClipSummary };

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &str, password: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "key", password)?;
        conn.execute("PRAGMA foreign_keys = ON;", [])?;

        let db = Database { conn };
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

    pub fn save_snapshot(
        &self, 
        owner_name: &str,
        fg_title: &str,
        exe_path: &str,
        hash: &str,
        payloads: Vec<ClipboardPayload>
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let exists: u32 = tx.query_row(
            "SELECT COUNT(1) FROM clips WHERE content_hash = ?", 
            [hash], 
            |r| r.get(0)
        )?;

        if exists > 0 { return Ok(()); }

        tx.execute(
            "INSERT INTO clips (owner_process_name, foreground_window_title, exe_path, content_hash) 
             VALUES (?, ?, ?, ?)",
            params![owner_name, fg_title, exe_path, hash],
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

    pub fn get_latest_clips(&self, limit: i32, offset: i32) -> rusqlite::Result<Vec<ClipSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, owner_process_name, foreground_window_title, content_hash,
            (SELECT data FROM formats WHERE clip_id = clips.id AND (format_id = 13 OR format_id = 1) LIMIT 1) as preview
            FROM clips ORDER BY timestamp DESC LIMIT ? OFFSET ?"
        )?;

        let rows = stmt.query_map([limit, offset], |row| {
            let raw_data: Option<Vec<u8>> = row.get(5)?;
            let preview = match raw_data {
                Some(bytes) => {
                    if let Ok(utf16_str) = String::from_utf16(&bytes.chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<u16>>()) {
                        utf16_str.chars().take(50).collect()
                    } else {
                        String::from_utf8_lossy(&bytes).chars().take(50).collect()
                    }
                }
                None => "bins".to_string(),
            };

            Ok(ClipSummary {
                timestamp: row.get(1)?,
                owner: row.get(2)?,
                fg_title: row.get(3)?,
                hash: row.get(4)?,
                preview,
            })
        })?;

        let mut clips = Vec::new();
        for clip in rows { clips.push(clip?); }
        Ok(clips)
    }

    pub fn get_total_count(&self) -> Result<i32> {
        self.conn.query_row("SELECT COUNT(*) FROM clips", [], |r| r.get(0))
    }

    pub fn get_clip_payloads(&self, hash: &str) -> Result<Vec<ClipboardPayload>> {
        let mut stmt = self.conn.prepare(
            "SELECT format_id, format_name, data
            FROM formats
            WHERE clip_id = (SELECT id FROM clips WHERE content_hash = ?)
            ORDER BY format_id"
        )?;
        let payloads = stmt.query_map([hash], |row| {
            Ok(ClipboardPayload {
                format_id: row.get(0)?,
                format_name: row.get(1)?,
                data: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(payloads)
    }
}