/// Local SQLite database using rusqlite (bundled — no system dep).
///
/// Mirrors the mobile app's local-first DB schema.
/// Also stores decrypted epoch keys per-session so we don't re-derive them
/// from the server on every message.
use chrono::{DateTime, Utc};
use dirs::data_dir;
use rusqlite::{params, Connection};
use std::path::PathBuf;

use crate::error::Result;
use crate::types::{DownloadState, LocalChat, LocalMessage};

pub struct Database {
    conn: Connection,
}

fn db_path() -> PathBuf {
    let base = data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("omnis").join("omnis.db")
}

impl Database {
    pub fn open() -> Result<Self> {
        let path = db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch("
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS chats (
                chat_id          INTEGER PRIMARY KEY,
                with_user        TEXT    NOT NULL,
                with_user_id     INTEGER,
                last_message     TEXT,
                last_message_time TEXT,
                unread_count     INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS messages (
                id               INTEGER PRIMARY KEY,
                chat_id          INTEGER NOT NULL,
                sender_id        INTEGER NOT NULL,
                epoch_id         INTEGER NOT NULL,
                reply_id         INTEGER,
                ciphertext       TEXT    NOT NULL,
                nonce            TEXT    NOT NULL,
                plaintext        TEXT,
                created_at       TEXT    NOT NULL,
                synced           INTEGER NOT NULL DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_messages_chat
                ON messages(chat_id, created_at);

            CREATE TABLE IF NOT EXISTS epochs (
                id               INTEGER PRIMARY KEY,
                chat_id          INTEGER NOT NULL,
                epoch_index      INTEGER NOT NULL,
                epoch_key        TEXT,           -- decrypted AES key (null until first use)
                UNIQUE(chat_id, epoch_index)
            );
        ")?;
        Ok(())
    }

    // ── Chats ────────────────────────────────────────────────────────────────

    pub fn upsert_chat(&self, chat: &LocalChat) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chats (chat_id, with_user, with_user_id, last_message, last_message_time, unread_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(chat_id) DO UPDATE SET
               with_user        = excluded.with_user,
               with_user_id     = excluded.with_user_id,
               last_message     = excluded.last_message,
               last_message_time= excluded.last_message_time,
               unread_count     = excluded.unread_count",
            params![
                chat.chat_id,
                chat.with_user,
                chat.with_user_id,
                chat.last_message,
                chat.last_message_time.map(|t| t.to_rfc3339()),
                chat.unread_count,
            ],
        )?;
        Ok(())
    }

    pub fn list_chats(&self) -> Result<Vec<LocalChat>> {
        let mut stmt = self.conn.prepare(
            "SELECT chat_id, with_user, with_user_id, last_message, last_message_time, unread_count
             FROM chats ORDER BY last_message_time DESC NULLS LAST",
        )?;
        let rows = stmt.query_map([], |row| {
            let time_str: Option<String> = row.get(4)?;
            let last_message_time = time_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());
            Ok(LocalChat {
                chat_id: row.get(0)?,
                with_user: row.get(1)?,
                with_user_id: row.get(2)?,
                last_message: row.get(3)?,
                last_message_time,
                unread_count: row.get(5)?,
            })
        })?;
        let mut chats = Vec::new();
        for row in rows {
            chats.push(row?);
        }
        Ok(chats)
    }

    pub fn clear_unread(&self, chat_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE chats SET unread_count = 0 WHERE chat_id = ?1",
            params![chat_id],
        )?;
        Ok(())
    }

    pub fn delete_all(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM messages; DELETE FROM chats; DELETE FROM epochs;")?;
        Ok(())
    }

    // ── Messages ─────────────────────────────────────────────────────────────

    pub fn upsert_message(&self, msg: &LocalMessage) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, chat_id, sender_id, epoch_id, reply_id, ciphertext, nonce, plaintext, created_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET plaintext = excluded.plaintext, synced = excluded.synced",
            params![
                msg.id,
                msg.chat_id,
                msg.sender_id,
                msg.epoch_id,
                msg.reply_id,
                msg.ciphertext,
                msg.nonce,
                msg.plaintext,
                msg.created_at.to_rfc3339(),
                msg.synced as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_messages(
        &self,
        chat_id: i64,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<LocalMessage>> {
        let sql = if before_id.is_some() {
            "SELECT id, chat_id, sender_id, epoch_id, reply_id, ciphertext, nonce, plaintext, created_at, synced
             FROM messages WHERE chat_id=?1 AND id < ?2 ORDER BY id DESC LIMIT ?3"
        } else {
            "SELECT id, chat_id, sender_id, epoch_id, reply_id, ciphertext, nonce, plaintext, created_at, synced
             FROM messages WHERE chat_id=?1 ORDER BY id DESC LIMIT ?3"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(
            params![chat_id, before_id.unwrap_or(i64::MAX), limit as i64],
            |row| {
                let time_str: String = row.get(8)?;
                let created_at = time_str.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now());
                Ok(LocalMessage {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    epoch_id: row.get(3)?,
                    reply_id: row.get(4)?,
                    ciphertext: row.get(5)?,
                    nonce: row.get(6)?,
                    plaintext: row.get(7)?,
                    media_info: None,
                    download_state: DownloadState::None,
                    created_at,
                    synced: row.get::<_, i64>(9)? != 0,
                    expires_at: None,
                    pixel_preview: None,
                })
            },
        )?;
        let mut msgs: Vec<LocalMessage> = rows.filter_map(|r| r.ok()).collect();
        msgs.reverse(); // return chronological order
        Ok(msgs)
    }

    pub fn get_message_by_id(&self, id: i64) -> Result<Option<LocalMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chat_id, sender_id, epoch_id, reply_id, ciphertext, nonce, plaintext, created_at, synced
             FROM messages WHERE id=?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            let time_str: String = row.get(8)?;
            let created_at = time_str.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now());
            Ok(LocalMessage {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_id: row.get(2)?,
                epoch_id: row.get(3)?,
                reply_id: row.get(4)?,
                ciphertext: row.get(5)?,
                nonce: row.get(6)?,
                plaintext: row.get(7)?,
                media_info: None,
                download_state: DownloadState::None,
                created_at,
                synced: row.get::<_, i64>(9)? != 0,
                expires_at: None,
                pixel_preview: None,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    // ── Epochs ───────────────────────────────────────────────────────────────

    pub fn get_epoch_key(&self, chat_id: i64, epoch_id: i64) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT epoch_key FROM epochs WHERE id=?1 AND chat_id=?2",
        )?;
        let mut rows = stmt.query_map(params![epoch_id, chat_id], |row| row.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Returns the (epoch_id, epoch_key) of the highest-index epoch for a chat.
    pub fn get_latest_epoch_key(&self, chat_id: i64) -> Result<Option<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, epoch_key FROM epochs WHERE chat_id=?1 AND epoch_key IS NOT NULL ORDER BY epoch_index DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![chat_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn save_epoch_key(&self, chat_id: i64, epoch_id: i64, epoch_index: i64, key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO epochs (id, chat_id, epoch_index, epoch_key)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id, epoch_index) DO UPDATE SET epoch_key = excluded.epoch_key",
            params![epoch_id, chat_id, epoch_index, key],
        )?;
        Ok(())
    }

    pub fn wipe_epoch_keys(&self) -> Result<()> {
        self.conn.execute("UPDATE epochs SET epoch_key = NULL", [])?;
        Ok(())
    }
}
