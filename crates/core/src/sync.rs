//! Incremental sync of one account, generic over [`ImapClient`].

use crate::db::{Db, FolderRow, MessageRow};
use crate::error::Result;
use crate::imap::{folder_role, is_selectable, ImapClient};
use crate::parse;

const HEADER_BATCH: usize = 200;
const BODY_BATCH: usize = 20;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FolderSync {
    /// Row ids of messages that were not known before.
    pub new_ids: Vec<i64>,
    /// Anything at all changed locally (new, flags, removed, reset).
    pub changed: bool,
}

pub fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Refreshes the folder list and roles. Returns the selectable folders, INBOX first.
pub async fn sync_folder_list<C: ImapClient>(
    c: &mut C,
    db: &Db,
    account_id: i64,
) -> Result<Vec<FolderRow>> {
    let remote = c.list_folders().await?;
    let mut names = Vec::new();
    let mut taken = std::collections::HashSet::new();
    // SPECIAL-USE wins over names: process attributed folders first.
    let mut ordered: Vec<_> = remote.iter().filter(|f| is_selectable(f)).collect();
    ordered.sort_by_key(|f| {
        !f.attributes.iter().any(|a| {
            matches!(
                a.as_str(),
                "\\sent" | "\\trash" | "\\archive" | "\\drafts" | "\\junk" | "\\all"
            )
        })
    });
    for f in ordered {
        let role = folder_role(f).filter(|r| taken.insert(*r));
        db.upsert_folder(account_id, &f.name, f.delimiter.as_deref(), role)?;
        names.push(f.name.clone());
    }
    db.retain_folders(account_id, &names)?;
    db.folders(account_id)
}

async fn ensure_selected<C: ImapClient>(c: &mut C, folder: &str) -> Result<()> {
    if c.selected() != Some(folder) {
        c.select(folder).await?;
    }
    Ok(())
}

/// [`sync_folder_since`] without a date window: the whole folder.
pub async fn sync_folder<C: ImapClient>(
    c: &mut C,
    db: &Db,
    folder: &FolderRow,
    skip: &std::collections::HashSet<u32>,
) -> Result<FolderSync> {
    sync_folder_since(c, db, folder, skip, None).await
}

/// One incremental pass over a folder: UIDVALIDITY check, new envelopes
/// (newest first), flag refresh and removal of vanished messages.
/// UIDs in `skip` are being moved/removed by a pending action and must not be re-inserted.
/// With `since`, only messages that arrived on or after that day are kept:
/// older ones are never downloaded and drop out locally as the window moves.
pub async fn sync_folder_since<C: ImapClient>(
    c: &mut C,
    db: &Db,
    folder: &FolderRow,
    skip: &std::collections::HashSet<u32>,
    since: Option<chrono::NaiveDate>,
) -> Result<FolderSync> {
    let mut out = FolderSync::default();
    let status = c.select(&folder.name).await?;
    let mut known_next = folder.uidnext;
    if folder.uidvalidity != 0 && status.uidvalidity != folder.uidvalidity {
        tracing::info!("{}: UIDVALIDITY changed, resyncing", folder.name);
        db.clear_folder(folder.id)?;
        known_next = 0;
        out.changed = true;
    }
    let local = db.uid_flags(folder.id)?;
    if status.exists == 0 {
        if !local.is_empty() {
            let gone: Vec<u32> = local.iter().map(|(u, _)| *u).collect();
            out.changed |= db.reconcile(folder.id, &[], &gone)?;
        }
        db.set_folder_state(
            folder.id,
            status.uidvalidity,
            status.uidnext.max(known_next),
        )?;
        return Ok(out);
    }

    let server = match since {
        None => c.fetch_flags("1:*").await?,
        Some(day) => {
            let inside: std::collections::HashSet<u32> =
                c.search_since(day).await?.into_iter().collect();
            match (inside.iter().min(), inside.iter().max()) {
                (Some(lo), Some(hi)) => {
                    let hi = *hi;
                    // Flags from the oldest UID in the window on; anything that
                    // arrived after the SEARCH is newer than `hi` and stays too.
                    c.fetch_flags(&format!("{lo}:*"))
                        .await?
                        .into_iter()
                        .filter(|(u, _)| inside.contains(u) || *u > hi)
                        .collect()
                }
                _ => Vec::new(),
            }
        }
    };
    // Anything on the server we don't have yet (also fills holes left by
    // an interrupted first sync), newest first.
    let local_uids: std::collections::HashSet<u32> = local.iter().map(|(u, _)| *u).collect();
    let mut fresh: Vec<u32> = server
        .iter()
        .map(|(u, _)| *u)
        .filter(|u| !local_uids.contains(u) && !skip.contains(u))
        .collect();
    fresh.sort_unstable_by(|a, b| b.cmp(a));

    // flags of known messages and vanished ones
    let server_uids: std::collections::HashSet<u32> = server.iter().map(|(u, _)| *u).collect();
    let flags: Vec<(u32, String)> = server
        .iter()
        .filter(|(u, _)| local_uids.contains(u))
        .cloned()
        .collect();
    let gone: Vec<u32> = local_uids
        .iter()
        .filter(|u| !server_uids.contains(u))
        .copied()
        .collect();
    out.changed |= db.reconcile(folder.id, &flags, &gone)?;

    for chunk in fresh.chunks(HEADER_BATCH) {
        let headers = c.fetch_headers(&uid_set(chunk)).await?;
        let items: Vec<_> = headers
            .into_iter()
            .map(|m| (m.uid, parse::parse_envelope(&m.header), m.flags, m.size))
            .collect();
        let ids = db.insert_messages(folder.account_id, folder.id, &items)?;
        out.changed |= !ids.is_empty();
        out.new_ids.extend(ids);
    }

    let max_uid = server.iter().map(|(u, _)| *u).max().unwrap_or(0);
    let next = if status.uidnext > 0 {
        status.uidnext
    } else {
        max_uid + 1
    };
    db.set_folder_state(folder.id, status.uidvalidity, next.max(max_uid + 1))?;
    Ok(out)
}

/// Downloads and stores bodies for messages received after `since`. Returns their ids.
pub async fn prefetch_bodies<C: ImapClient>(
    c: &mut C,
    db: &Db,
    folder: &FolderRow,
    since: i64,
    limit: i64,
) -> Result<Vec<i64>> {
    let missing = db.missing_bodies(folder.id, since, limit)?;
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    ensure_selected(c, &folder.name).await?;
    let mut done = Vec::new();
    for chunk in missing.chunks(BODY_BATCH) {
        let uids: Vec<u32> = chunk.iter().map(|(_, u)| *u).collect();
        for (uid, raw) in c.fetch_raw(&uid_set(&uids)).await? {
            if let Some((id, _)) = chunk.iter().find(|(_, u)| *u == uid) {
                db.store_body(*id, &parse::parse_body(&raw))?;
                done.push(*id);
            }
        }
    }
    Ok(done)
}

/// Raw bytes of one message (for bodies on demand and attachments).
pub async fn fetch_raw<C: ImapClient>(
    c: &mut C,
    folder: &str,
    uid: u32,
) -> Result<Option<Vec<u8>>> {
    ensure_selected(c, folder).await?;
    Ok(c.fetch_raw(&uid.to_string())
        .await?
        .into_iter()
        .find(|(u, _)| *u == uid)
        .map(|(_, raw)| raw))
}

pub async fn fetch_body<C: ImapClient>(c: &mut C, db: &Db, m: &MessageRow) -> Result<bool> {
    let folder = db.folder(m.folder_id)?;
    match fetch_raw(c, &folder.name, m.uid).await? {
        Some(raw) => {
            db.store_body(m.id, &parse::parse_body(&raw))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub async fn store_flags<C: ImapClient>(
    c: &mut C,
    folder: &str,
    uids: &[u32],
    add: &[String],
    remove: &[String],
) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    ensure_selected(c, folder).await?;
    let set = uid_set(uids);
    if !add.is_empty() {
        c.store(&set, true, &add.join(" ")).await?;
    }
    if !remove.is_empty() {
        c.store(&set, false, &remove.join(" ")).await?;
    }
    Ok(())
}

/// `MOVE` when announced, otherwise `COPY` + `\Deleted` + `EXPUNGE`.
pub async fn move_messages<C: ImapClient>(
    c: &mut C,
    from: &str,
    uids: &[u32],
    dest: &str,
) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    ensure_selected(c, from).await?;
    let set = uid_set(uids);
    if c.has_capability("MOVE") {
        c.mv(&set, dest).await
    } else {
        c.copy(&set, dest).await?;
        c.store(&set, true, "\\Deleted").await?;
        c.expunge(&set).await
    }
}

/// Removes messages from a folder (`\Deleted` + `EXPUNGE`). On Gmail this
/// drops the label: the message stays in All Mail.
pub async fn expunge_messages<C: ImapClient>(c: &mut C, folder: &str, uids: &[u32]) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    ensure_selected(c, folder).await?;
    let set = uid_set(uids);
    c.store(&set, true, "\\Deleted").await?;
    c.expunge(&set).await
}
