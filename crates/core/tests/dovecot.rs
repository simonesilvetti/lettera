//! End-to-end against a real Dovecot. Skipped unless LETTERA_IMAP_TEST is set:
//!   docker compose -f docker-compose.test.yml up -d
//!   LETTERA_IMAP_TEST=1 cargo test -p lettera-core --test dovecot

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use common::fixture;
use lettera_core::config::Endpoint;
use lettera_core::db::Db;
use lettera_core::imap::{self, Credentials, ImapClient, RealImap};
use lettera_core::sync;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use tokio_rustls::TlsConnector;

fn enabled() -> bool {
    if std::env::var("LETTERA_IMAP_TEST").is_err() {
        eprintln!("LETTERA_IMAP_TEST non impostata: test Dovecot saltato");
        return false;
    }
    true
}

fn connector() -> TlsConnector {
    let path = format!("{}/tests/dovecot/ca.pem", env!("CARGO_MANIFEST_DIR"));
    let ca = CertificateDer::from_pem_file(path).expect("ca.pem");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca).expect("ca");
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("tls versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    TlsConnector::from(Arc::new(cfg))
}

fn ep(port: u16) -> Endpoint {
    Endpoint {
        host: "localhost".into(),
        port,
        implicit_tls: port == 10993,
    }
}

fn creds(user: &str) -> Credentials {
    Credentials::Password {
        username: user.into(),
        password: "pass".into(),
    }
}

async fn login(user: &str, port: u16) -> RealImap {
    imap::connect_with(&ep(port), &creds(user), &connector())
        .await
        .expect("login")
}

fn unique_user() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("t{n}@test.local")
}

#[tokio::test]
async fn full_cycle_against_dovecot() {
    if !enabled() {
        return;
    }
    let user = unique_user();
    let mut c = login(&user, 10993).await;
    assert!(c.has_capability("IDLE"));
    for name in [
        "01_plain.eml",
        "02_reply_with_references.eml",
        "08_attachment_pdf.eml",
    ] {
        c.append("INBOX", "", &fixture(name)).await.unwrap();
    }

    let db = Db::open_memory().unwrap();
    let acc = db.upsert_account(&user, "test", "imap").unwrap();
    let folders = sync::sync_folder_list(&mut c, &db, acc).await.unwrap();
    let role = |r: &str| {
        folders
            .iter()
            .find(|f| f.role.as_deref() == Some(r))
            .cloned()
    };
    let inbox = role("inbox").unwrap();
    assert_eq!(role("sent").unwrap().name, "Sent");
    assert_eq!(role("trash").unwrap().name, "Trash");
    assert!(role("archive").is_none());

    let none = HashSet::new();
    let r = sync::sync_folder(&mut c, &db, &inbox, &none).await.unwrap();
    assert_eq!(r.new_ids.len(), 3);
    let page = db
        .list_threads(inbox.id, "", 0, 10, chrono::Local::now())
        .unwrap();
    assert_eq!(page.total, 2);

    // bodies and attachments
    let inbox = db.folder(inbox.id).unwrap();
    assert_eq!(
        sync::prefetch_bodies(&mut c, &db, &inbox, 0, 10)
            .await
            .unwrap()
            .len(),
        3
    );
    let pdf = db
        .list_threads(inbox.id, "fattura", 0, 1, chrono::Local::now())
        .unwrap()
        .items[0]
        .ids[0];
    assert_eq!(
        db.attachments(pdf).unwrap()[0].filename,
        "fattura-ottobre.pdf"
    );

    // flags
    let m = db.message(pdf).unwrap();
    sync::store_flags(
        &mut c,
        &inbox.name,
        &[m.uid],
        &["\\Seen".into(), "\\Flagged".into()],
        &[],
    )
    .await
    .unwrap();
    sync::sync_folder(&mut c, &db, &inbox, &none).await.unwrap();
    let flags = db.message(pdf).unwrap().flags;
    assert!(
        flags.contains("\\Seen") && flags.contains("\\Flagged"),
        "{flags}"
    );

    // archive: the folder does not exist yet, create it like the engine does
    c.create("Archive").await.unwrap();
    sync::move_messages(&mut c, &inbox.name, &[m.uid], "Archive")
        .await
        .unwrap();
    let folders = sync::sync_folder_list(&mut c, &db, acc).await.unwrap();
    let archive = folders
        .iter()
        .find(|f| f.role.as_deref() == Some("archive"))
        .unwrap()
        .clone();
    let r = sync::sync_folder(&mut c, &db, &inbox, &none).await.unwrap();
    assert!(r.changed);
    assert_eq!(db.uid_flags(inbox.id).unwrap().len(), 2);
    let r = sync::sync_folder(&mut c, &db, &archive, &none)
        .await
        .unwrap();
    assert_eq!(r.new_ids.len(), 1);

    // delete permanently from INBOX
    let first = db.uid_flags(inbox.id).unwrap()[0].0;
    sync::expunge_messages(&mut c, &inbox.name, &[first])
        .await
        .unwrap();
    sync::sync_folder(&mut c, &db, &inbox, &none).await.unwrap();
    assert_eq!(db.uid_flags(inbox.id).unwrap().len(), 1);
    c.logout().await;
}

#[tokio::test]
async fn starttls_login() {
    if !enabled() {
        return;
    }
    let mut c = login(&unique_user(), 10143).await;
    let st = c.select("INBOX").await.unwrap();
    assert!(st.uidvalidity > 0);
    c.logout().await;
}

#[tokio::test]
async fn idle_sees_new_mail() {
    if !enabled() {
        return;
    }
    let user = unique_user();
    let mut watcher = login(&user, 10993).await;
    watcher.select("INBOX").await.unwrap();
    let session = watcher.into_session();
    let mut handle = session.idle();
    handle.init().await.unwrap();

    let u = user.clone();
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut c = login(&u, 10993).await;
        c.append("INBOX", "", &fixture("01_plain.eml"))
            .await
            .unwrap();
        c.logout().await;
    });
    let res = {
        let (wait, _stop) = handle.wait_with_timeout(Duration::from_secs(15));
        wait.await.unwrap()
    };
    assert!(matches!(
        res,
        async_imap::extensions::idle::IdleResponse::NewData(_)
    ));
    writer.await.unwrap();
    let mut session = handle.done().await.unwrap();
    session.logout().await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_an_auth_error() {
    if !enabled() {
        return;
    }
    let bad = Credentials::Password {
        username: unique_user(),
        password: "nope".into(),
    };
    let err = imap::connect_with(&ep(10993), &bad, &connector())
        .await
        .err()
        .unwrap();
    assert!(matches!(err, lettera_core::Error::Auth(_)), "{err}");
}

#[tokio::test]
async fn date_window_against_dovecot() {
    if !enabled() {
        return;
    }
    let user = unique_user();
    let mut c = login(&user, 10993).await;
    for name in ["01_plain.eml", "02_reply_with_references.eml"] {
        c.append("INBOX", "", &fixture(name)).await.unwrap();
    }
    let db = Db::open_memory().unwrap();
    let acc = db.upsert_account(&user, "test", "imap").unwrap();
    let inbox = sync::sync_folder_list(&mut c, &db, acc).await.unwrap()[0].clone();
    let none = HashSet::new();
    let today = chrono::Local::now().date_naive();

    // arrived today: inside a window starting last month, outside one starting tomorrow
    let r = sync::sync_folder_since(
        &mut c,
        &db,
        &inbox,
        &none,
        Some(today - chrono::Months::new(1)),
    )
    .await
    .unwrap();
    assert_eq!(r.new_ids.len(), 2);
    let f = db.folder(inbox.id).unwrap();
    sync::sync_folder_since(&mut c, &db, &f, &none, Some(today + chrono::Days::new(1)))
        .await
        .unwrap();
    assert!(db.uid_flags(inbox.id).unwrap().is_empty());
    c.logout().await;
}
