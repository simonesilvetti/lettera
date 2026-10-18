mod common;

use common::fixture;
use lettera_core::db::Db;
use lettera_core::parse::parse_envelope;

fn setup() -> (Db, i64, i64) {
    let db = Db::open_memory().unwrap();
    let acc = db
        .upsert_account("marco@esempio.it", "Marco", "imap")
        .unwrap();
    let inbox = db
        .upsert_folder(acc, "INBOX", Some("/"), Some("inbox"))
        .unwrap();
    (db, acc, inbox)
}

fn insert(db: &Db, acc: i64, folder: i64, uid: u32, name: &str) -> String {
    let env = parse_envelope(&fixture(name));
    let ids = db
        .insert_messages(acc, folder, &[(uid, env, String::new(), 100)])
        .unwrap();
    db.message(ids[0]).unwrap().thread_id
}

fn thread_of(db: &Db, folder: i64, uid: u32) -> String {
    let rows = db.uid_flags(folder).unwrap();
    assert!(rows.iter().any(|(u, _)| *u == uid));
    let page = db
        .list_threads(folder, "", 0, 100, chrono::Local::now())
        .unwrap();
    for t in page.items {
        for id in &t.ids {
            if db.message(*id).unwrap().uid == uid {
                return t.thread_id;
            }
        }
    }
    panic!("uid {uid} not found")
}

#[test]
fn references_chain_and_subject_fallback() {
    let (db, acc, inbox) = setup();
    let root = insert(&db, acc, inbox, 1, "01_plain.eml");
    assert_eq!(root, format!("{acc}:root-001@esempio.it"));
    assert_eq!(
        insert(&db, acc, inbox, 2, "02_reply_with_references.eml"),
        root
    );
    // only In-Reply-To pointing at the reply
    assert_eq!(
        insert(&db, acc, inbox, 3, "03_reply_in_reply_to_only.eml"),
        root
    );
    // no reply headers, same subject, shared participants, within 7 days
    assert_eq!(
        insert(&db, acc, inbox, 4, "04_subject_only_same_people.eml"),
        root
    );
    // same subject but strangers: separate thread
    assert_ne!(
        insert(&db, acc, inbox, 5, "05_subject_only_strangers.eml"),
        root
    );
    assert_eq!(insert(&db, acc, inbox, 6, "11_folded_headers.eml"), root);

    let page = db
        .list_threads(inbox, "", 0, 50, chrono::Local::now())
        .unwrap();
    assert_eq!(page.total, 2);
    let main = page.items.iter().find(|t| t.thread_id == root).unwrap();
    assert_eq!(main.count, 5);
    let view = db.thread(&root).unwrap();
    assert_eq!(view.messages.len(), 5);
    assert_eq!(view.subject, "Pianificazione Q4");
    assert!(view.participants.contains(&"Anna Rossi".to_string()));
}

#[test]
fn children_arriving_first_are_adopted() {
    let (db, acc, inbox) = setup();
    // the In-Reply-To-only reply arrives before its parent
    let early = insert(&db, acc, inbox, 3, "03_reply_in_reply_to_only.eml");
    assert_eq!(early, format!("{acc}:reply-002@esempio.it"));
    let parent = insert(&db, acc, inbox, 2, "02_reply_with_references.eml");
    assert_eq!(parent, format!("{acc}:root-001@esempio.it"));
    assert_eq!(thread_of(&db, inbox, 3), parent);
    assert_eq!(insert(&db, acc, inbox, 1, "01_plain.eml"), parent);
}

#[test]
fn same_message_in_two_folders_shares_thread() {
    let (db, acc, inbox) = setup();
    let all = db
        .upsert_folder(acc, "All Mail", Some("/"), Some("all"))
        .unwrap();
    let a = insert(&db, acc, inbox, 1, "01_plain.eml");
    let b = insert(&db, acc, all, 77, "01_plain.eml");
    assert_eq!(a, b);
    // the thread view shows it once
    assert_eq!(db.thread(&a).unwrap().messages.len(), 1);
}

#[test]
fn full_text_search() {
    let (db, acc, inbox) = setup();
    insert(&db, acc, inbox, 1, "01_plain.eml");
    insert(&db, acc, inbox, 2, "08_attachment_pdf.eml");
    let now = chrono::Local::now();
    assert_eq!(
        db.list_threads(inbox, "fattura", 0, 10, now).unwrap().total,
        1
    );
    assert_eq!(db.list_threads(inbox, "ann", 0, 10, now).unwrap().total, 2);
    assert_eq!(
        db.list_threads(inbox, "\"weird", 0, 10, now).unwrap().total,
        0
    );
    // body text becomes searchable once it is stored
    let id = db.uid_flags(inbox).unwrap();
    assert_eq!(id.len(), 2);
    let m = db
        .list_threads(inbox, "pianificazione", 0, 10, now)
        .unwrap()
        .items[0]
        .ids[0];
    db.store_body(
        m,
        &lettera_core::parse::parse_body(&fixture("01_plain.eml")),
    )
    .unwrap();
    assert_eq!(
        db.list_threads(inbox, "trimestre", 0, 10, now)
            .unwrap()
            .total,
        1
    );
}
