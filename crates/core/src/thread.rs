//! Threading: References / In-Reply-To first, then normalized subject
//! (within 7 days and with a shared participant).

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::model::Addr;
use crate::parse::Envelope;

const SUBJECT_WINDOW: i64 = 7 * 24 * 3600;

/// Lowercased subject without any stack of `Re:`, `Fwd:`, `R:`, `I:` (and
/// common variants like `Re[2]:`, `AW:`, `WG:`, `RIF:`).
pub fn normalize_subject(s: &str) -> String {
    let mut t = s.trim().to_lowercase();
    loop {
        let before = t.clone();
        // mailing-list tags like "[lista] "
        if t.starts_with('[') {
            if let Some(end) = t.find(']') {
                let rest = t[end + 1..].trim_start();
                if !rest.is_empty() {
                    t = rest.to_string();
                }
            }
        }
        if let Some(colon) = t.find(':') {
            let prefix = t[..colon].trim();
            let base = prefix.split(['[', '(']).next().unwrap_or("").trim();
            if matches!(
                base,
                "re" | "fwd"
                    | "fw"
                    | "r"
                    | "i"
                    | "rif"
                    | "aw"
                    | "wg"
                    | "tr"
                    | "sv"
                    | "vs"
                    | "inoltro"
            ) && prefix.len() <= base.len() + 4
            {
                t = t[colon + 1..].trim_start().to_string();
            }
        }
        if t == before {
            break;
        }
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn participants<'a>(addrs: impl IntoIterator<Item = &'a Addr>) -> HashSet<String> {
    addrs
        .into_iter()
        .map(|a| a.addr.to_lowercase())
        .filter(|a| !a.is_empty())
        .collect()
}

fn env_participants(env: &Envelope) -> HashSet<String> {
    participants(std::iter::once(&env.from).chain(&env.to).chain(&env.cc))
}

fn key(account_id: i64, root: &str) -> String {
    format!("{account_id}:{root}")
}

/// Picks the thread for a message that is about to be inserted.
pub fn assign(conn: &Connection, account_id: i64, env: &Envelope) -> Result<String> {
    let mut refs: Vec<&str> = env.references.iter().map(String::as_str).collect();
    if let Some(irt) = env.in_reply_to.as_deref() {
        if !refs.contains(&irt) {
            refs.push(irt);
        }
    }
    // same message already stored in another folder
    if let Some(t) = lookup_by_message_id(conn, account_id, &env.message_id)? {
        return Ok(t);
    }
    if !refs.is_empty() {
        for r in &refs {
            if let Some(t) = lookup_by_message_id(conn, account_id, r)? {
                return Ok(t);
            }
            let k = key(account_id, r);
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM messages WHERE account_id = ?1 AND thread_id = ?2 LIMIT 1",
                    params![account_id, k],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_some() {
                return Ok(k);
            }
        }
        return Ok(key(account_id, refs[0]));
    }
    let norm = normalize_subject(&env.subject);
    if !norm.is_empty() {
        let mine = env_participants(env);
        let mut stmt = conn.prepare_cached(
            "SELECT thread_id, from_addr, to_addrs, cc_addrs FROM messages
             WHERE account_id = ?1 AND subject_norm = ?2 AND date BETWEEN ?3 AND ?4
             ORDER BY abs(date - ?5) LIMIT 50",
        )?;
        let rows = stmt.query_map(
            params![
                account_id,
                norm,
                env.date - SUBJECT_WINDOW,
                env.date + SUBJECT_WINDOW,
                env.date
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )?;
        for row in rows {
            let (thread, from, to, cc) = row?;
            let mut theirs: HashSet<String> = HashSet::new();
            if let Some(f) = from {
                theirs.insert(f.to_lowercase());
            }
            for list in [to, cc].into_iter().flatten() {
                let v: Vec<Addr> = serde_json::from_str(&list).unwrap_or_default();
                theirs.extend(participants(&v));
            }
            if !mine.is_disjoint(&theirs) {
                return Ok(thread);
            }
        }
    }
    Ok(key(account_id, &env.message_id))
}

fn lookup_by_message_id(conn: &Connection, account_id: i64, mid: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT thread_id FROM messages WHERE account_id = ?1 AND message_id = ?2 LIMIT 1",
            params![account_id, mid],
            |r| r.get(0),
        )
        .optional()?)
}

/// Replies that arrived before their parent were keyed on the parent's id:
/// move them into the parent's thread.
pub fn adopt_children(
    conn: &Connection,
    account_id: i64,
    message_id: &str,
    thread: &str,
) -> Result<()> {
    let orphan_key = key(account_id, message_id);
    if orphan_key != thread {
        conn.execute(
            "UPDATE messages SET thread_id = ?1 WHERE account_id = ?2 AND thread_id = ?3",
            params![thread, account_id, orphan_key],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_subject as n;

    #[test]
    fn normalizes_prefixes() {
        assert_eq!(n("Re: Ciao"), "ciao");
        assert_eq!(n("RE: Fwd: R: I: ciao   mondo"), "ciao mondo");
        assert_eq!(n("Re[2]: progetto"), "progetto");
        assert_eq!(n("AW: WG: Termin"), "termin");
        assert_eq!(n("[lista] Re: annuncio"), "annuncio");
        assert_eq!(n("Rif: offerta"), "offerta");
        assert_eq!(n("Ordine: 1234"), "ordine: 1234");
        assert_eq!(n("Re:"), "");
        assert_eq!(n(""), "");
        assert_eq!(n("Riunione: domani"), "riunione: domani");
    }
}
