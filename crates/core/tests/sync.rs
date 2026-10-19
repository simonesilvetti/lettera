mod common;

use std::collections::HashSet;

use common::{fixture, MockFolder, MockImap};
use lettera_core::db::Db;
use lettera_core::sync;

fn server() -> MockImap {
    let mut s = MockImap::default();
    let mut inbox = MockFolder::new(100);
    inbox.add(fixture("01_plain.eml"), "\\Seen");
    inbox.add(fixture("02_reply_with_references.eml"), "");
    inbox.add(fixture("08_attachment_pdf.eml"), "");
    s.folders.insert("INBOX".into(), inbox);
    let mut sent = MockFolder::new(7);
    sent.attributes = vec!["\\sent".into()];
    s.folders.insert("Posta inviata".into(), sent);
    s.folders.insert("Archivio".into(), MockFolder::new(9));
    let mut ns = MockFolder::new(1);
    ns.attributes = vec!["\\noselect".into()];
    s.folders.insert("Gruppo".into(), ns);
    s
}

fn setup() -> (Db, i64) {
    let db = Db::open_memory().unwrap();
    let acc = db
        .upsert_account("marco@esempio.it", "Marco", "imap")
        .unwrap();
    (db, acc)
}

#[tokio::test]
async fn folder_list_and_roles() {
    let (db, acc) = setup();
    let mut s = server();
    let folders = sync::sync_folder_list(&mut s, &db, acc).await.unwrap();
    let roles: Vec<_> = folders
        .iter()
        .map(|f| (f.name.as_str(), f.role.as_deref()))
        .collect();
    assert_eq!(roles[0], ("INBOX", Some("inbox")));
    assert!(roles.contains(&("Posta inviata", Some("sent"))));
    assert!(roles.contains(&("Archivio", Some("archive"))));
    assert!(!roles.iter().any(|(n, _)| *n == "Gruppo"));
    // removed on the server -> removed locally
    s.folders.remove("Archivio");
    let folders = sync::sync_folder_list(&mut s, &db, acc).await.unwrap();
    assert_eq!(folders.len(), 2);
}

#[tokio::test]
async fn incremental_sync() {
    let (db, acc) = setup();
    let mut s = server();
    let folders = sync::sync_folder_list(&mut s, &db, acc).await.unwrap();
    let inbox = folders[0].clone();
    let none = HashSet::new();

    let r = sync::sync_folder(&mut s, &db, &inbox, &none).await.unwrap();
    assert_eq!(r.new_ids.len(), 3);
    assert!(r.changed);
    let f = db.folder(inbox.id).unwrap();
    assert_eq!((f.uidvalidity, f.uidnext), (100, 4));

    // nothing new: no header fetch at all
    s.log.clear();
    let r = sync::sync_folder(&mut s, &db, &f, &none).await.unwrap();
    assert!(r.new_ids.is_empty() && !r.changed);
    assert!(!s.log.iter().any(|l| l.starts_with("FETCH HEADERS")));

    // a new message, a flag change and an expunge on the server
    let inbox_srv = s.folders.get_mut("INBOX").unwrap();
    let new_uid = inbox_srv.add(fixture("03_reply_in_reply_to_only.eml"), "");
    inbox_srv.messages.get_mut(&2).unwrap().0 = "\\Seen \\Flagged".into();
    inbox_srv.messages.remove(&3);
    s.log.clear();
    let r = sync::sync_folder(&mut s, &db, &f, &none).await.unwrap();
    assert_eq!(r.new_ids.len(), 1);
    assert!(s.log.contains(&format!("FETCH HEADERS {new_uid}")));
    let flags = db.uid_flags(f.id).unwrap();
    let uids: Vec<u32> = flags.iter().map(|(u, _)| *u).collect();
    assert_eq!(uids.len(), 3);
    assert!(!uids.contains(&3));
    assert!(flags.contains(&(2, "\\Seen \\Flagged".to_string())));

    let page = db
        .list_threads(f.id, "", 0, 10, chrono::Local::now())
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].count, 3);
    assert!(page.items[0].unread);
    assert!(page.items[0].flagged);
}

#[tokio::test]
async fn uidvalidity_change_resyncs_folder() {
    let (db, acc) = setup();
    let mut s = server();
    let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    sync::sync_folder(&mut s, &db, &inbox, &none).await.unwrap();
    let before = db.uid_flags(inbox.id).unwrap();
    assert_eq!(before.len(), 3);

    // the server rebuilt the mailbox: new UIDVALIDITY, different UIDs
    let mut rebuilt = MockFolder::new(200);
    rebuilt.uidnext = 50;
    rebuilt.add(fixture("06_multipart_alternative.eml"), "");
    rebuilt.add(fixture("01_plain.eml"), "\\Seen");
    s.folders.insert("INBOX".into(), rebuilt);

    let f = db.folder(inbox.id).unwrap();
    let r = sync::sync_folder(&mut s, &db, &f, &none).await.unwrap();
    assert!(r.changed);
    assert_eq!(r.new_ids.len(), 2);
    let after = db.uid_flags(inbox.id).unwrap();
    let uids: Vec<u32> = after.iter().map(|(u, _)| *u).collect();
    assert_eq!(uids.len(), 2);
    assert!(uids.iter().all(|u| *u >= 50));
    let f = db.folder(inbox.id).unwrap();
    assert_eq!((f.uidvalidity, f.uidnext), (200, 52));
}

#[tokio::test]
async fn empty_folder_clears_local_copy() {
    let (db, acc) = setup();
    let mut s = server();
    let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    sync::sync_folder(&mut s, &db, &inbox, &none).await.unwrap();
    s.folders.get_mut("INBOX").unwrap().messages.clear();
    let f = db.folder(inbox.id).unwrap();
    let r = sync::sync_folder(&mut s, &db, &f, &none).await.unwrap();
    assert!(r.changed);
    assert!(db.uid_flags(inbox.id).unwrap().is_empty());
}

#[tokio::test]
async fn pending_moves_are_not_reinserted() {
    let (db, acc) = setup();
    let mut s = server();
    let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    sync::sync_folder(&mut s, &db, &inbox, &none).await.unwrap();
    // archived locally, server not updated yet
    let id = db
        .list_threads(inbox.id, "fattura", 0, 1, chrono::Local::now())
        .unwrap()
        .items[0]
        .ids[0];
    let uid = db.message(id).unwrap().uid;
    db.delete_messages(&[id]).unwrap();
    let skip: HashSet<u32> = [uid].into();
    let f = db.folder(inbox.id).unwrap();
    let r = sync::sync_folder(&mut s, &db, &f, &skip).await.unwrap();
    assert!(r.new_ids.is_empty());
    // without the pending mark the server copy comes back (self-healing)
    let r = sync::sync_folder(&mut s, &db, &f, &none).await.unwrap();
    assert_eq!(r.new_ids.len(), 1);
}

#[tokio::test]
async fn bodies_on_demand_and_prefetch() {
    let (db, acc) = setup();
    let mut s = server();
    let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    sync::sync_folder(&mut s, &db, &inbox, &none).await.unwrap();
    let f = db.folder(inbox.id).unwrap();
    let done = sync::prefetch_bodies(&mut s, &db, &f, 0, 100)
        .await
        .unwrap();
    assert_eq!(done.len(), 3);
    assert!(sync::prefetch_bodies(&mut s, &db, &f, 0, 100)
        .await
        .unwrap()
        .is_empty());
    let pdf = db
        .list_threads(f.id, "fattura", 0, 1, chrono::Local::now())
        .unwrap()
        .items[0]
        .ids[0];
    let atts = db.attachments(pdf).unwrap();
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].filename, "fattura-ottobre.pdf");
    let (text, _) = db.body(pdf).unwrap().unwrap();
    assert!(text.contains("In allegato"));
}

#[tokio::test]
async fn move_uses_move_or_copy_expunge() {
    let (db, acc) = setup();
    let mut s = server();
    sync::sync_folder_list(&mut s, &db, acc).await.unwrap();

    sync::move_messages(&mut s, "INBOX", &[1], "Archivio")
        .await
        .unwrap();
    assert!(s.log.iter().any(|l| l.starts_with("COPY 1 Archivio")));
    assert!(s.log.iter().any(|l| l.starts_with("STORE 1 + \\Deleted")));
    assert!(s.log.iter().any(|l| l.starts_with("EXPUNGE 1")));
    assert_eq!(s.folders["Archivio"].messages.len(), 1);
    assert!(!s.folders["INBOX"].messages.contains_key(&1));

    s.capabilities.push("MOVE".into());
    s.log.clear();
    sync::move_messages(&mut s, "INBOX", &[2], "Archivio")
        .await
        .unwrap();
    assert_eq!(s.log, ["MOVE 2 Archivio", "COPY 2 Archivio"]);
    assert_eq!(s.folders["Archivio"].messages.len(), 2);
}

#[tokio::test]
async fn gmail_style_archive_drops_inbox_copy_only() {
    let (db, acc) = setup();
    let mut s = server();
    let mut all = MockFolder::new(5);
    all.attributes = vec!["\\all".into()];
    all.add(fixture("01_plain.eml"), "\\Seen");
    s.folders.insert("[Gmail]/All Mail".into(), all);
    sync::sync_folder_list(&mut s, &db, acc).await.unwrap();
    sync::expunge_messages(&mut s, "INBOX", &[1]).await.unwrap();
    assert!(!s.folders["INBOX"].messages.contains_key(&1));
    assert_eq!(s.folders["[Gmail]/All Mail"].messages.len(), 1);
}

#[tokio::test]
async fn flags_roundtrip() {
    let (db, acc) = setup();
    let mut s = server();
    sync::sync_folder_list(&mut s, &db, acc).await.unwrap();
    sync::store_flags(&mut s, "INBOX", &[2, 3], &["\\Seen".into()], &[])
        .await
        .unwrap();
    assert_eq!(s.folders["INBOX"].messages[&2].0, "\\Seen");
    sync::store_flags(&mut s, "INBOX", &[1], &[], &["\\Seen".into()])
        .await
        .unwrap();
    assert_eq!(s.folders["INBOX"].messages[&1].0, "");
}

#[tokio::test]
async fn date_window_skips_old_mail() {
    let (db, acc) = setup();
    let mut s = MockImap::default();
    let day = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
    let mut inbox = MockFolder::new(100);
    inbox.add_dated(fixture("01_plain.eml"), "", day(2019, 5, 1));
    inbox.add_dated(
        fixture("02_reply_with_references.eml"),
        "",
        day(2024, 1, 10),
    );
    inbox.add_dated(fixture("08_attachment_pdf.eml"), "", day(2026, 9, 1));
    s.folders.insert("INBOX".into(), inbox);
    let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    let uids = |db: &Db| {
        let mut v: Vec<u32> = db
            .uid_flags(inbox.id)
            .unwrap()
            .iter()
            .map(|(u, _)| *u)
            .collect();
        v.sort();
        v
    };

    // 2022-10-01 onwards: the 2019 message is never fetched
    let r = sync::sync_folder_since(&mut s, &db, &inbox, &none, Some(day(2022, 10, 1)))
        .await
        .unwrap();
    assert_eq!(r.new_ids.len(), 2);
    assert_eq!(uids(&db), vec![2, 3]);
    assert!(s.log.iter().any(|l| l == "FETCH FLAGS 2:*"));
    assert!(!s
        .log
        .iter()
        .any(|l| l.starts_with("FETCH HEADERS") && l.contains('1')));

    // the window moves forward: the 2024 message drops out locally
    let f = db.folder(inbox.id).unwrap();
    let r = sync::sync_folder_since(&mut s, &db, &f, &none, Some(day(2025, 1, 1)))
        .await
        .unwrap();
    assert!(r.changed && r.new_ids.is_empty());
    assert_eq!(uids(&db), vec![3]);
    assert_eq!(
        s.folders["INBOX"].messages.len(),
        3,
        "nothing touched on the server"
    );

    // a wider window (or none) brings older mail back
    let r = sync::sync_folder_since(&mut s, &db, &f, &none, None)
        .await
        .unwrap();
    assert_eq!(r.new_ids.len(), 2);
    assert_eq!(uids(&db), vec![1, 2, 3]);

    // nothing inside the window: folder empty locally, no flag fetch
    s.log.clear();
    sync::sync_folder_since(&mut s, &db, &f, &none, Some(day(2027, 1, 1)))
        .await
        .unwrap();
    assert!(uids(&db).is_empty());
    assert!(!s.log.iter().any(|l| l.starts_with("FETCH")));
}

#[tokio::test]
async fn to_me_and_attachment_marks() {
    async fn threads(me: &str) -> Vec<(String, bool, bool)> {
        let db = Db::open_memory().unwrap();
        let acc = db.upsert_account(me, "Me", "imap").unwrap();
        let mut s = server();
        let inbox = sync::sync_folder_list(&mut s, &db, acc).await.unwrap()[0].clone();
        sync::sync_folder(&mut s, &db, &inbox, &HashSet::new())
            .await
            .unwrap();
        let mut v: Vec<_> = db
            .list_threads(inbox.id, "", 0, 10, chrono::Local::now())
            .unwrap()
            .items
            .into_iter()
            .map(|t| (t.subject, t.to_me, t.has_attachments))
            .collect();
        v.sort();
        v
    }
    // Marco is in To: of both threads (case does not matter); only the invoice has a PDF
    let marco = threads("Marco@Esempio.it").await;
    assert_eq!(marco.len(), 2);
    assert!(marco.iter().all(|(_, to_me, _)| *to_me), "{marco:?}");
    assert_eq!(marco.iter().filter(|(_, _, att)| *att).count(), 1);
    // Giulia is only in Cc: no mark
    let giulia = threads("giulia@esempio.it").await;
    assert!(giulia.iter().all(|(_, to_me, _)| !to_me), "{giulia:?}");
}
