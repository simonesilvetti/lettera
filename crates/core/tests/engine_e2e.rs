//! The whole engine against Dovecot (IMAP) and Mailpit (SMTP), through the
//! same API the UI uses. Skipped unless LETTERA_IMAP_TEST is set; needs a
//! running Secret Service (the password goes in the real keyring and is
//! removed at the end).

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::fixture;
use lettera_core::config::{Endpoint, Paths};
use lettera_core::events::EventSink;
use lettera_core::imap::{self, Credentials, ImapClient};
use lettera_core::model::{Draft, DraftKind, NewAccount};
use lettera_core::Engine;
use serde_json::Value;

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl EventSink for Recorder {
    fn emit(&self, name: &str, payload: Value) {
        self.0.lock().unwrap().push((name.to_string(), payload));
    }
}

async fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "timeout: {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn http_json(url: &str) -> Value {
    let text = lettera_core::oauth::http_get(url).await.expect(url);
    serde_json::from_str(&text).expect("json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_end_to_end() {
    if std::env::var("LETTERA_IMAP_TEST").is_err() {
        eprintln!("LETTERA_IMAP_TEST non impostata: test end-to-end saltato");
        return;
    }
    let ca = format!("{}/tests/dovecot/ca.pem", env!("CARGO_MANIFEST_DIR"));
    std::env::set_var("LETTERA_CA_FILE", &ca);

    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let email = format!("e2e{n}@test.local");
    let dir = std::env::temp_dir().join(format!("lettera-e2e-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = Paths {
        config_file: dir.join("config.toml"),
        data_dir: dir.clone(),
    };

    // mail already waiting on the server
    let ep = Endpoint {
        host: "localhost".into(),
        port: 10993,
        implicit_tls: true,
    };
    let creds = Credentials::Password {
        username: email.clone(),
        password: "pass".into(),
    };
    let mut seed = imap::connect(&ep, &creds).await.expect("seed login");
    for f in [
        "01_plain.eml",
        "02_reply_with_references.eml",
        "08_attachment_pdf.eml",
    ] {
        seed.append("INBOX", "", &fixture(f)).await.unwrap();
    }

    let rec = Arc::new(Recorder::default());
    let engine = Engine::start(
        tokio::runtime::Handle::current(),
        paths.clone(),
        rec.clone(),
        Arc::new(|_| {}),
    )
    .unwrap();

    // a wrong password is refused before anything is saved
    let mut account = NewAccount {
        name: "Prova".into(),
        kind: "imap".into(),
        email: email.clone(),
        imap_host: Some("localhost".into()),
        imap_port: Some(10993),
        smtp_host: Some("localhost".into()),
        smtp_port: Some(10587),
        username: None,
        password: Some("sbagliata".into()),
        client_id: None,
        client_secret: None,
        imap_tls: Some("implicit".into()),
        // wrong on purpose: Mailpit wants STARTTLS on this port
        smtp_tls: Some("implicit".into()),
    };
    assert!(engine.add_account(account.clone()).await.is_err());
    assert!(!paths.config_file.exists());

    account.password = Some("pass".into());
    engine.add_account(account).await.expect("add account");
    let cfg = std::fs::read_to_string(&paths.config_file).unwrap();
    assert!(cfg.contains(&email) && !cfg.contains("\"pass\""), "{cfg}");
    let saved = lettera_core::config::parse(&cfg).unwrap();
    assert_eq!(
        saved.accounts[0].smtp_tls, None,
        "the mode that works is saved"
    );
    assert!(
        cfg.contains("# signature = "),
        "placeholders for unset options"
    );
    assert_eq!(engine.list_accounts().len(), 1);

    // initial sync
    let inbox = wait_for("inbox", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.role.as_deref() == Some("inbox") && f.total == 3)
    })
    .await;
    let page = engine.list_threads(inbox.id, "", 0, 50).unwrap();
    assert_eq!(page.total, 2);
    let pdf_thread = engine
        .list_threads(inbox.id, "fattura", 0, 1)
        .unwrap()
        .items[0]
        .clone();
    let main = page
        .items
        .iter()
        .find(|t| t.thread_id != pdf_thread.thread_id)
        .unwrap()
        .clone();

    // bodies arrive (prefetch or on demand)
    let view = engine.get_thread(&main.thread_id).unwrap();
    assert_eq!(view.messages.len(), 2);
    let root_id = view.messages[0].id;
    let body = wait_for("body", || {
        let b = engine.get_body(root_id, "text").ok()?;
        b.ready.then_some(b)
    })
    .await;
    assert!(body.blocks[0].text.contains("giovedì"));
    let pdf_id = pdf_thread.ids[0];
    let atts = wait_for("attachments", || {
        engine.get_body(pdf_id, "text").ok().filter(|b| b.ready)?;
        let a = engine.list_attachments(pdf_id).ok()?;
        (!a.is_empty()).then_some(a)
    })
    .await;
    let out = dir.join("salvato.pdf");
    engine.save_attachment(atts[0].id, &out).await.unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    let html = engine.get_body(root_id, "html").unwrap();
    assert!(html.html.unwrap().contains("default-src 'none'"));

    // drafts: stored in the Drafts folder, replaced on each store, reopened, discarded
    let mut nd = engine.prepare_draft(&email, DraftKind::New, None).unwrap();
    nd.to = "anna@esempio.it".into();
    nd.bcc = "nascosto@esempio.it".into();
    nd.subject = "Bozza di prova".into();
    nd.body = "prima versione".into();
    let mut nd = engine.store_draft(nd).await.expect("store draft");
    let drafts = wait_for("draft copy", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.role.as_deref() == Some("drafts") && f.total == 1)
    })
    .await;
    let fresh = engine.prepare_draft(&email, DraftKind::New, None).unwrap();
    assert!(
        fresh.id.is_none() && fresh.to.is_empty(),
        "c starts a new message"
    );
    let row = engine.list_threads(drafts.id, "", 0, 10).unwrap().items[0].ids[0];

    // edit → esc → edit, quickly: nothing waits for the server. Reopening
    // finds the local draft, storing shows the new version (body included)
    // in the same row at once, so an open thread stays valid.
    let fast = Duration::from_millis(50);
    for i in 2..=4 {
        let t = Instant::now();
        let mut d = engine.edit_draft(row).await.expect("edit draft");
        assert!(t.elapsed() < fast, "edit_draft took {:?}", t.elapsed());
        assert_eq!(d.id, nd.id, "the local draft, not a rebuild");
        d.body = format!("versione {i}");
        let t = Instant::now();
        nd = engine.store_draft(d).await.expect("store draft");
        assert!(t.elapsed() < fast, "store_draft took {:?}", t.elapsed());
        let t = Instant::now();
        let page = engine.list_threads(drafts.id, "", 0, 10).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].ids, vec![row]);
        let body = engine.get_body(row, "text").unwrap();
        assert!(body.ready, "no loading after esc");
        assert!(body.blocks[0].text.contains(&format!("versione {i}")));
        assert!(t.elapsed() < fast, "reading back took {:?}", t.elapsed());
    }

    // the server catches up: one copy, the last one, in the same row
    let mut check = imap::connect(&ep, &creds).await.expect("check login");
    let start = Instant::now();
    loop {
        check.select("Drafts").await.unwrap();
        let on_server = check.fetch_headers("1:*").await.unwrap();
        let ids: Vec<String> = on_server
            .iter()
            .map(|m| lettera_core::parse::parse_envelope(&m.header).message_id)
            .collect();
        let page = engine.list_threads(drafts.id, "", 0, 10).unwrap();
        if ids.len() == 1
            && Some(&ids[0]) == nd.message_id.as_ref()
            && page.total == 1
            && page.items[0].ids == vec![row]
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timeout: server copy {ids:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // settled: still the same draft in the same row, body kept
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(engine.get_body(row, "text").unwrap().ready);
    let again = engine.edit_draft(row).await.unwrap();
    assert_eq!(again.id, nd.id);
    assert_eq!(again.body, "versione 4");
    let copy = (row, again);
    // without the local row (another client's draft): rebuilt from the message
    engine
        .save_draft(&Draft {
            message_id: None,
            ..nd.clone()
        })
        .unwrap();
    let rebuilt = engine.edit_draft(copy.0).await.expect("edit draft");
    assert!(rebuilt.id.is_none());
    assert_eq!(rebuilt.message_id, nd.message_id);
    assert!(rebuilt.to.contains("anna@esempio.it"));
    assert!(rebuilt.bcc.contains("nascosto@esempio.it"));
    assert_eq!(rebuilt.subject, "Bozza di prova");
    assert!(rebuilt.body.contains("versione 4"));
    engine
        .discard_draft(&Draft {
            id: nd.id,
            ..rebuilt
        })
        .unwrap();
    wait_for("draft discarded", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.id == drafts.id && f.total == 0)
    })
    .await;

    // reply: threading headers, delivery and APPEND into Sent
    let reply_to = view.messages[1].id;
    let mut draft = engine
        .prepare_draft(&email, DraftKind::Reply, Some(reply_to))
        .unwrap();
    assert_eq!(draft.subject, "Re: Pianificazione Q4");
    assert!(draft.to.contains("marco@esempio.it"));
    draft.body = "Risposta di prova da lettera.".into();
    draft.id = Some(engine.save_draft(&draft).unwrap());
    let resumed = engine
        .prepare_draft(&email, DraftKind::Reply, Some(reply_to))
        .unwrap();
    assert_eq!(resumed.body, draft.body);
    // with the PDF saved earlier attached
    draft.attachments = vec![out.display().to_string()];
    engine.send(draft).await.expect("send");
    let list = http_json("http://127.0.0.1:18025/api/v1/messages?limit=50").await;
    let sent = list["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["Subject"] == "Re: Pianificazione Q4" && m["From"]["Address"] == email.as_str())
        .expect("message in mailpit")
        .clone();
    let headers = http_json(&format!(
        "http://127.0.0.1:18025/api/v1/message/{}/headers",
        sent["ID"].as_str().unwrap()
    ))
    .await;
    assert_eq!(headers["In-Reply-To"][0], "<reply-002@esempio.it>");
    let detail = http_json(&format!(
        "http://127.0.0.1:18025/api/v1/message/{}",
        sent["ID"].as_str().unwrap()
    ))
    .await;
    let att = &detail["Attachments"][0];
    assert_eq!(att["FileName"], "salvato.pdf");
    assert_eq!(att["ContentType"], "application/pdf");
    assert_eq!(
        headers["References"][0],
        "<root-001@esempio.it> <reply-002@esempio.it>"
    );
    let sent_folder = wait_for("sent copy", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.role.as_deref() == Some("sent") && f.total == 1)
    })
    .await;
    let sent_thread = engine.list_threads(sent_folder.id, "", 0, 1).unwrap().items[0].clone();
    assert_eq!(
        sent_thread.thread_id, main.thread_id,
        "reply joins the thread"
    );
    // the draft is gone
    let fresh = engine
        .prepare_draft(&email, DraftKind::Reply, Some(reply_to))
        .unwrap();
    assert!(fresh.id.is_none() && fresh.body.is_empty());

    // flags
    engine
        .set_flags(&pdf_thread.ids, &["\\Flagged".into()], &[])
        .unwrap();
    assert!(
        engine
            .list_threads(inbox.id, "fattura", 0, 1)
            .unwrap()
            .items[0]
            .flagged
    );
    tokio::time::sleep(Duration::from_millis(500)).await;

    // archive: Archive does not exist yet on this server and gets created
    engine.archive(&pdf_thread.ids).unwrap();
    assert_eq!(engine.list_threads(inbox.id, "", 0, 50).unwrap().total, 1);
    let archive = wait_for("archive folder", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.role.as_deref() == Some("archive") && f.total == 1)
    })
    .await;
    let archived = engine.list_threads(archive.id, "", 0, 1).unwrap().items[0].clone();
    assert!(archived.flagged, "flag survived the move");

    // delete goes to Trash
    engine.delete(&archived.ids).unwrap();
    wait_for("trash", || {
        engine
            .list_folders(&email)
            .ok()?
            .into_iter()
            .find(|f| f.role.as_deref() == Some("trash") && f.total == 1)
    })
    .await;

    // IDLE: new mail shows up by itself and raises a notification
    tokio::time::sleep(Duration::from_secs(1)).await;
    seed.append("INBOX", "", &fixture("06_multipart_alternative.eml"))
        .await
        .unwrap();
    let mail = wait_for("new_mail", || {
        rec.0
            .lock()
            .unwrap()
            .iter()
            .find(|(n, _)| n == "new_mail")
            .map(|(_, p)| p.clone())
    })
    .await;
    assert_eq!(mail["account"], email.as_str());
    let items = mail["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "one notification for one message");
    assert!(
        items[0]["id"].as_i64().unwrap() > 0 && !items[0]["subject"].as_str().unwrap().is_empty()
    );
    wait_for("inbox refresh", || {
        (engine.list_threads(inbox.id, "", 0, 50).ok()?.total == 2).then_some(())
    })
    .await;
    seed.logout().await;

    // nothing secret in the database
    let db_bytes = std::fs::read(paths.db_file()).unwrap();
    assert!(!db_bytes.windows(6).any(|w| w == b"\"pass\""));

    engine.remove_account(&email).unwrap();
    assert!(engine.list_accounts().is_empty());
    assert!(lettera_core::secrets::get(&email).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
