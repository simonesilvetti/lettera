//! SQLite storage. One connection behind a mutex: every query is short.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::error::{Error, Result};
use crate::model::{
    Addr, AttachmentInfo, DateGroup, Draft, DraftKind, FolderInfo, MessageView, ThreadPage,
    ThreadSummary, ThreadView,
};
use crate::parse::{Envelope, ParsedBody};
use crate::thread;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderRow {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    pub delimiter: Option<String>,
    pub role: Option<String>,
    pub uidvalidity: u32,
    pub uidnext: u32,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub from: Addr,
    pub reply_to: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: i64,
    pub flags: String,
    pub thread_id: String,
}

#[derive(Debug, Clone)]
pub struct AttachmentRow {
    pub id: i64,
    pub message_id: i64,
    pub part_index: u32,
    pub filename: String,
}

pub fn has_flag(flags: &str, flag: &str) -> bool {
    flags
        .split_whitespace()
        .any(|f| f.eq_ignore_ascii_case(flag))
}

fn json_addrs(v: &[Addr]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn addrs_of(s: Option<String>) -> Vec<Addr> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Turns what the user types into an FTS5 query: every word must match as a prefix.
pub fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

const MESSAGE_COLS: &str = "id, account_id, folder_id, uid, message_id, in_reply_to, refs, from_name, from_addr, reply_to, to_addrs, cc_addrs, subject, date, flags, thread_id";

fn message_row(r: &Row) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        folder_id: r.get(2)?,
        uid: r.get(3)?,
        message_id: r.get(4)?,
        in_reply_to: r.get(5)?,
        refs: r
            .get::<_, Option<String>>(6)?
            .map(|s| s.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        from: Addr {
            name: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            addr: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        },
        reply_to: addrs_of(r.get(9)?),
        to: addrs_of(r.get(10)?),
        cc: addrs_of(r.get(11)?),
        subject: r.get(12)?,
        date: r.get(13)?,
        flags: r.get(14)?,
        thread_id: r.get(15)?,
    })
}

/// Start of today, yesterday and of the last 7 days, in local time.
pub fn day_boundaries(now: chrono::DateTime<chrono::Local>) -> (i64, i64, i64) {
    use chrono::{Duration, TimeZone};
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| chrono::Local.from_local_datetime(&d).earliest())
        .map(|d| d.timestamp())
        .unwrap_or_else(|| now.timestamp() - 86_400);
    let day = Duration::days(1).num_seconds();
    (midnight, midnight - day, midnight - 6 * day)
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            crate::config::private_dir(dir)?;
        }
        let conn = Connection::open(path)?;
        crate::config::restrict(path, 0o600)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!(
                "BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))?;
        }
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---------------------------------------------------------------- accounts

    pub fn upsert_account(&self, email: &str, name: &str, kind: &str) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO accounts (email, name, kind) VALUES (?1, ?2, ?3)
             ON CONFLICT(email) DO UPDATE SET name = excluded.name, kind = excluded.kind",
            params![email, name, kind],
        )?;
        Ok(
            c.query_row("SELECT id FROM accounts WHERE email = ?1", [email], |r| {
                r.get(0)
            })?,
        )
    }

    pub fn account_id(&self, email: &str) -> Result<i64> {
        self.lock()
            .query_row("SELECT id FROM accounts WHERE email = ?1", [email], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("account {email}")))
    }

    pub fn account_email(&self, id: i64) -> Result<String> {
        Ok(self
            .lock()
            .query_row("SELECT email FROM accounts WHERE id = ?1", [id], |r| {
                r.get(0)
            })?)
    }

    pub fn delete_account(&self, email: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM drafts WHERE account = ?1", [email])?;
        c.execute("DELETE FROM accounts WHERE email = ?1", [email])?;
        Ok(())
    }

    /// Accounts in the database that are no longer in the config.
    pub fn accounts_not_in(&self, emails: &[String]) -> Result<Vec<String>> {
        let c = self.lock();
        let mut stmt = c.prepare("SELECT email FROM accounts")?;
        let all: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(all.into_iter().filter(|e| !emails.contains(e)).collect())
    }

    // ----------------------------------------------------------------- folders

    pub fn upsert_folder(
        &self,
        account_id: i64,
        name: &str,
        delimiter: Option<&str>,
        role: Option<&str>,
    ) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO folders (account_id, name, delimiter, role) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, name) DO UPDATE SET delimiter = excluded.delimiter, role = excluded.role",
            params![account_id, name, delimiter, role],
        )?;
        Ok(c.query_row(
            "SELECT id FROM folders WHERE account_id = ?1 AND name = ?2",
            params![account_id, name],
            |r| r.get(0),
        )?)
    }

    /// Drops local folders that disappeared from the server.
    pub fn retain_folders(&self, account_id: i64, names: &[String]) -> Result<()> {
        let c = self.lock();
        let mut stmt = c.prepare("SELECT id, name FROM folders WHERE account_id = ?1")?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, name) in rows {
            if !names.contains(&name) {
                c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
            }
        }
        Ok(())
    }

    fn folder_row(r: &Row) -> rusqlite::Result<FolderRow> {
        Ok(FolderRow {
            id: r.get(0)?,
            account_id: r.get(1)?,
            name: r.get(2)?,
            delimiter: r.get(3)?,
            role: r.get(4)?,
            uidvalidity: r.get(5)?,
            uidnext: r.get(6)?,
        })
    }

    pub fn folders(&self, account_id: i64) -> Result<Vec<FolderRow>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id, account_id, name, delimiter, role, uidvalidity, uidnext FROM folders
             WHERE account_id = ?1 ORDER BY (role = 'inbox') DESC, name",
        )?;
        let rows = stmt
            .query_map([account_id], Self::folder_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn folder(&self, id: i64) -> Result<FolderRow> {
        self.lock()
            .query_row(
                "SELECT id, account_id, name, delimiter, role, uidvalidity, uidnext FROM folders WHERE id = ?1",
                [id],
                Self::folder_row,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("folder {id}")))
    }

    pub fn folder_by_role(&self, account_id: i64, role: &str) -> Result<Option<FolderRow>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT id, account_id, name, delimiter, role, uidvalidity, uidnext FROM folders
                 WHERE account_id = ?1 AND role = ?2 ORDER BY id LIMIT 1",
                params![account_id, role],
                Self::folder_row,
            )
            .optional()?)
    }

    pub fn folder_infos(&self, account: &str) -> Result<Vec<FolderInfo>> {
        let account_id = self.account_id(account)?;
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT f.id, f.name, f.role,
                    (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id),
                    (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id AND instr(m.flags, '\\Seen') = 0)
             FROM folders f WHERE f.account_id = ?1
             ORDER BY CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2
                      WHEN 'archive' THEN 3 WHEN 'all' THEN 4 WHEN 'junk' THEN 5 WHEN 'trash' THEN 6 ELSE 7 END, f.name",
        )?;
        let rows = stmt
            .query_map([account_id], |r| {
                Ok(FolderInfo {
                    id: r.get(0)?,
                    account: account.to_string(),
                    name: r.get(1)?,
                    role: r.get(2)?,
                    total: r.get(3)?,
                    unread: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn set_folder_state(&self, folder_id: i64, uidvalidity: u32, uidnext: u32) -> Result<()> {
        self.lock().execute(
            "UPDATE folders SET uidvalidity = ?2, uidnext = ?3, last_sync = strftime('%s','now') WHERE id = ?1",
            params![folder_id, uidvalidity, uidnext],
        )?;
        Ok(())
    }

    /// Empties a folder (used when UIDVALIDITY changes).
    pub fn clear_folder(&self, folder_id: i64) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM messages WHERE folder_id = ?1", [folder_id])?;
        c.execute(
            "UPDATE folders SET uidvalidity = 0, uidnext = 0 WHERE id = ?1",
            [folder_id],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- messages

    /// Inserts envelopes in one transaction, threading each one. Returns new row ids.
    pub fn insert_messages(
        &self,
        account_id: i64,
        folder_id: i64,
        items: &[(u32, Envelope, String, u32)],
    ) -> Result<Vec<i64>> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        let mut ids = Vec::with_capacity(items.len());
        for (uid, env, flags, size) in items {
            let thread = thread::assign(&tx, account_id, env)?;
            let changed = tx.execute(
                "INSERT OR IGNORE INTO messages (account_id, folder_id, uid, message_id, in_reply_to, refs,
                    from_name, from_addr, reply_to, to_addrs, cc_addrs, subject, subject_norm, date, flags,
                    size, has_attachments, thread_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    account_id,
                    folder_id,
                    uid,
                    env.message_id,
                    env.in_reply_to,
                    env.references.join(" "),
                    env.from.name,
                    env.from.addr.to_lowercase(),
                    json_addrs(&env.reply_to),
                    json_addrs(&env.to),
                    json_addrs(&env.cc),
                    env.subject,
                    thread::normalize_subject(&env.subject),
                    env.date,
                    flags,
                    size,
                    env.has_attachments,
                    thread
                ],
            )?;
            if changed == 0 {
                continue;
            }
            thread::adopt_children(&tx, account_id, &env.message_id, &thread)?;
            ids.push(tx.last_insert_rowid());
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn uid_flags(&self, folder_id: i64) -> Result<Vec<(u32, String)>> {
        let c = self.lock();
        let mut stmt = c.prepare("SELECT uid, flags FROM messages WHERE folder_id = ?1")?;
        let rows = stmt
            .query_map([folder_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Applies server flags and removes messages that vanished. Returns true if anything changed.
    pub fn reconcile(&self, folder_id: i64, flags: &[(u32, String)], gone: &[u32]) -> Result<bool> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        let mut changed = 0;
        {
            let mut up = tx.prepare_cached(
                "UPDATE messages SET flags = ?3 WHERE folder_id = ?1 AND uid = ?2 AND flags != ?3",
            )?;
            for (uid, f) in flags {
                changed += up.execute(params![folder_id, uid, f])?;
            }
            let mut del =
                tx.prepare_cached("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2")?;
            for uid in gone {
                changed += del.execute(params![folder_id, uid])?;
            }
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    pub fn message(&self, id: i64) -> Result<MessageRow> {
        self.lock()
            .query_row(
                &format!("SELECT {MESSAGE_COLS} FROM messages WHERE id = ?1"),
                [id],
                message_row,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("message {id}")))
    }

    pub fn messages(&self, ids: &[i64]) -> Result<Vec<MessageRow>> {
        ids.iter().map(|id| self.message(*id)).collect()
    }

    pub fn delete_messages(&self, ids: &[i64]) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Local, optimistic flag change. Returns the new flag strings.
    pub fn change_flags(&self, ids: &[i64], add: &[String], remove: &[String]) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        for id in ids {
            let flags: Option<String> = tx
                .query_row("SELECT flags FROM messages WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            let Some(flags) = flags else { continue };
            let mut set: Vec<String> = flags
                .split_whitespace()
                .filter(|f| !remove.iter().any(|r| r.eq_ignore_ascii_case(f)))
                .map(str::to_string)
                .collect();
            for a in add {
                if !set.iter().any(|f| f.eq_ignore_ascii_case(a)) {
                    set.push(a.clone());
                }
            }
            tx.execute(
                "UPDATE messages SET flags = ?2 WHERE id = ?1",
                params![id, set.join(" ")],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_threads(
        &self,
        folder_id: i64,
        query: &str,
        offset: i64,
        limit: i64,
        now: chrono::DateTime<chrono::Local>,
    ) -> Result<ThreadPage> {
        let fts = fts_query(query);
        let base = match &fts {
            Some(_) => "SELECT * FROM messages WHERE folder_id = ?1 AND id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?2)",
            None => "SELECT * FROM messages WHERE folder_id = ?1 AND ?2 IS NULL",
        };
        let c = self.lock();
        let p = params![folder_id, fts];
        let total: i64 = c.query_row(
            &format!("WITH m AS ({base}) SELECT COUNT(DISTINCT thread_id) FROM m"),
            p,
            |r| r.get(0),
        )?;
        let unread: i64 = c.query_row(
            &format!("WITH m AS ({base}) SELECT COUNT(DISTINCT thread_id) FROM m WHERE instr(flags, '\\Seen') = 0"),
            p,
            |r| r.get(0),
        )?;
        let (today, yesterday, week) = day_boundaries(now);
        let mut groups = Vec::new();
        {
            let mut stmt = c.prepare(&format!(
                "WITH m AS ({base}), t AS (SELECT MAX(date) d FROM m GROUP BY thread_id)
                 SELECT CASE WHEN d >= ?3 THEN 0 WHEN d >= ?4 THEN 1 WHEN d >= ?5 THEN 2 ELSE 3 END b, COUNT(*)
                 FROM t GROUP BY b ORDER BY b"
            ))?;
            let labels = ["today", "yesterday", "this week", "earlier"];
            let rows = stmt.query_map(params![folder_id, fts, today, yesterday, week], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (b, count) = row?;
                groups.push(DateGroup {
                    label: labels[b.clamp(0, 3) as usize].to_string(),
                    count,
                });
            }
        }
        let mut stmt = c.prepare(&format!(
            "WITH m AS ({base})
             SELECT thread_id, MAX(date) AS d, from_name, from_addr, subject, COUNT(*),
                    SUM(instr(flags, '\\Seen') = 0), SUM(instr(flags, '\\Flagged') > 0),
                    MAX(has_attachments), group_concat(id),
                    MAX(EXISTS(SELECT 1 FROM json_each(m.to_addrs) j, accounts a
                               WHERE a.id = m.account_id
                                 AND lower(json_extract(j.value, '$.addr')) = lower(a.email)))
             FROM m GROUP BY thread_id ORDER BY d DESC, thread_id LIMIT ?3 OFFSET ?4"
        ))?;
        let items = stmt
            .query_map(params![folder_id, fts, limit, offset], |r| {
                let name: Option<String> = r.get(2)?;
                let addr: Option<String> = r.get(3)?;
                let ids: String = r.get(9)?;
                Ok(ThreadSummary {
                    thread_id: r.get(0)?,
                    date: r.get(1)?,
                    from: Addr {
                        name: name.unwrap_or_default(),
                        addr: addr.unwrap_or_default(),
                    }
                    .display(),
                    subject: r.get(4)?,
                    count: r.get(5)?,
                    unread: r.get::<_, i64>(6)? > 0,
                    flagged: r.get::<_, i64>(7)? > 0,
                    has_attachments: r.get::<_, i64>(8)? > 0,
                    to_me: r.get::<_, i64>(10)? > 0,
                    ids: ids.split(',').filter_map(|s| s.parse().ok()).collect(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ThreadPage {
            total,
            unread,
            offset,
            groups,
            items,
        })
    }

    pub fn thread(&self, thread_id: &str) -> Result<ThreadView> {
        let c = self.lock();
        let mut stmt = c.prepare(&format!(
            "SELECT {MESSAGE_COLS}, snippet, has_attachments FROM messages m
             WHERE thread_id = ?1
             ORDER BY date, (SELECT role FROM folders f WHERE f.id = m.folder_id) = 'trash', id"
        ))?;
        let rows: Vec<(MessageRow, String, bool)> = stmt
            .query_map([thread_id], |r| {
                Ok((message_row(r)?, r.get(16)?, r.get(17)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let Some(first) = rows.first() else {
            return Err(Error::NotFound(format!("thread {thread_id}")));
        };
        let account: String = c.query_row(
            "SELECT email FROM accounts WHERE id = ?1",
            [first.0.account_id],
            |r| r.get(0),
        )?;
        let subject = first.0.subject.clone();
        let mut seen = std::collections::HashSet::new();
        let mut participants: Vec<String> = Vec::new();
        let mut messages = Vec::new();
        for (m, snippet, has_attachments) in rows {
            if !seen.insert(m.message_id.clone()) {
                continue;
            }
            let who = m.from.display();
            if !who.is_empty() && !participants.contains(&who) {
                participants.push(who);
            }
            messages.push(MessageView {
                id: m.id,
                folder_id: m.folder_id,
                unread: !has_flag(&m.flags, "\\Seen"),
                flagged: has_flag(&m.flags, "\\Flagged"),
                from: m.from,
                to: m.to,
                cc: m.cc,
                subject: m.subject,
                date: m.date,
                snippet,
                has_attachments,
            });
        }
        Ok(ThreadView {
            thread_id: thread_id.to_string(),
            account,
            subject,
            participants,
            messages,
        })
    }

    // ------------------------------------------------------------------ bodies

    /// Body of a message, looking at other copies of the same Message-ID too.
    pub fn body(&self, id: i64) -> Result<Option<(String, Option<String>)>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT b.text, b.html FROM bodies b JOIN messages m ON m.id = b.message_id
                 WHERE m.id = ?1 OR (m.account_id, m.message_id) =
                    (SELECT account_id, message_id FROM messages WHERE id = ?1)
                 ORDER BY m.id = ?1 DESC LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn store_body(&self, id: i64, body: &ParsedBody) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO bodies (message_id, text, html) VALUES (?1, ?2, ?3)",
            params![id, body.text, body.html],
        )?;
        tx.execute("DELETE FROM attachments WHERE message_id = ?1", [id])?;
        for a in &body.attachments {
            tx.execute(
                "INSERT INTO attachments (message_id, part_index, filename, mime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, a.index, a.filename, a.mime, a.size as i64],
            )?;
        }
        tx.execute(
            "UPDATE messages SET snippet = ?2, has_attachments = ?3 WHERE id = ?1",
            params![id, body.snippet, !body.attachments.is_empty()],
        )?;
        tx.execute(
            "UPDATE messages_fts SET body = ?2 WHERE rowid = ?1",
            params![id, body.text],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Messages in a folder without a body, newest first, received after `since`.
    pub fn missing_bodies(
        &self,
        folder_id: i64,
        since: i64,
        limit: i64,
    ) -> Result<Vec<(i64, u32)>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id, uid FROM messages m WHERE folder_id = ?1 AND date >= ?2
             AND NOT EXISTS (SELECT 1 FROM bodies b WHERE b.message_id = m.id)
             ORDER BY date DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![folder_id, since, limit], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Attachments of a message. Like [`Db::body`], they come from whichever
    /// copy of the same Message-ID (INBOX, All Mail, …) has a downloaded body,
    /// preferring the message itself: a thread shows one copy per Message-ID,
    /// not necessarily the one that was downloaded.
    pub fn attachments(&self, message_id: i64) -> Result<Vec<AttachmentInfo>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id, message_id, filename, mime, size FROM attachments
             WHERE message_id = (
                SELECT m.id FROM messages m JOIN bodies b ON b.message_id = m.id
                WHERE m.id = ?1 OR (m.account_id, m.message_id) =
                    (SELECT account_id, message_id FROM messages WHERE id = ?1)
                ORDER BY m.id = ?1 DESC LIMIT 1)
             ORDER BY part_index",
        )?;
        let rows = stmt
            .query_map([message_id], |r| {
                Ok(AttachmentInfo {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    filename: r.get(2)?,
                    mime: r.get(3)?,
                    size: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn attachment(&self, id: i64) -> Result<AttachmentRow> {
        self.lock()
            .query_row(
                "SELECT id, message_id, part_index, filename FROM attachments WHERE id = ?1",
                [id],
                |r| {
                    Ok(AttachmentRow {
                        id: r.get(0)?,
                        message_id: r.get(1)?,
                        part_index: r.get(2)?,
                        filename: r.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("attachment {id}")))
    }

    // ------------------------------------------------------------------ drafts

    pub fn save_draft(&self, draft: &Draft) -> Result<i64> {
        let c = self.lock();
        let now = chrono::Utc::now().timestamp();
        let mut d = draft.clone();
        d.saved_at = Some(now);
        let data = serde_json::to_string(&d).map_err(|e| Error::Other(e.to_string()))?;
        match d.id {
            Some(id) => {
                let n = c.execute(
                    "UPDATE drafts SET data = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, data, now],
                )?;
                if n > 0 {
                    return Ok(id);
                }
                c.execute(
                    "INSERT INTO drafts (id, account, kind, source, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, d.account, d.kind.as_str(), d.source, data, now],
                )?;
                Ok(id)
            }
            None => {
                c.execute(
                    "INSERT INTO drafts (account, kind, source, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![d.account, d.kind.as_str(), d.source, data, now],
                )?;
                Ok(c.last_insert_rowid())
            }
        }
    }

    /// The pending local draft for this account/kind/source, if any.
    pub fn find_draft(
        &self,
        account: &str,
        kind: DraftKind,
        source: Option<i64>,
    ) -> Result<Option<Draft>> {
        let data: Option<(i64, String)> = self
            .lock()
            .query_row(
                "SELECT id, data FROM drafts WHERE account = ?1 AND kind = ?2 AND source IS ?3
                 ORDER BY updated_at DESC LIMIT 1",
                params![account, kind.as_str(), source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(data.and_then(|(id, s)| {
            serde_json::from_str::<Draft>(&s).ok().map(|mut d| {
                d.id = Some(id);
                d
            })
        }))
    }

    /// Every local draft of the account, newest first.
    pub fn drafts(&self, account: &str) -> Result<Vec<Draft>> {
        let c = self.lock();
        let mut stmt =
            c.prepare("SELECT id, data FROM drafts WHERE account = ?1 ORDER BY updated_at DESC")?;
        let rows = stmt
            .query_map([account], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, s)| {
                serde_json::from_str::<Draft>(&s).ok().map(|mut d| {
                    d.id = Some(id);
                    d
                })
            })
            .collect())
    }

    /// The local draft whose copy in the Drafts folder has this Message-ID.
    pub fn draft_by_message_id(&self, account: &str, message_id: &str) -> Result<Option<Draft>> {
        Ok(self
            .drafts(account)?
            .into_iter()
            .find(|d| d.message_id.as_deref() == Some(message_id)))
    }

    pub fn draft(&self, id: i64) -> Result<Option<Draft>> {
        let data: Option<String> = self
            .lock()
            .query_row("SELECT data FROM drafts WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(data.and_then(|s| {
            serde_json::from_str::<Draft>(&s).ok().map(|mut d| {
                d.id = Some(id);
                d
            })
        }))
    }

    /// Database id of a message of the account with this Message-ID.
    pub fn message_by_message_id(&self, account_id: i64, message_id: &str) -> Result<Option<i64>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND message_id = ?2 LIMIT 1",
                params![account_id, message_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn delete_draft(&self, id: i64) -> Result<()> {
        self.lock()
            .execute("DELETE FROM drafts WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- contacts

    pub fn suggest_contacts(&self, query: &str, limit: i64) -> Result<Vec<Addr>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let like = format!("%{}%", q.replace(['%', '_'], ""));
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT max(from_name), from_addr, COUNT(*) n FROM messages
             WHERE from_addr != '' AND (from_addr LIKE ?1 OR from_name LIKE ?1)
             GROUP BY from_addr ORDER BY n DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![like, limit], |r| {
                Ok(Addr {
                    name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    addr: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Generic helper for engine-level queries that don't deserve a method.
    pub fn ids_in_folder_by_message_id(
        &self,
        folder_id: i64,
        message_ids: &[String],
    ) -> Result<Vec<(i64, u32)>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let c = self.lock();
        let marks = vec!["?"; message_ids.len()].join(",");
        let mut stmt = c.prepare(&format!(
            "SELECT id, uid FROM messages WHERE folder_id = {folder_id} AND message_id IN ({marks})"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(message_ids.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}
