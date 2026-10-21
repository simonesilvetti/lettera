//! The real engine behind plain HTTP, so the real frontend can be driven in a
//! headless browser without Tauri (see `tools/ui/drive.mjs` and `tools/ui/t.py`).
//!
//! - `POST /invoke/<command>` with the command's JSON arguments, like Tauri's
//!   `invoke`; the reply is the result, or the error as a JSON string (400).
//! - `GET /events?since=N`: events from N on, waiting up to 20 s for one.
//! - `GET /log`: what the bridge did instead of opening dialogs or links.
//! - anything else: files from `dist/`, with `tools/ui/shim.js` put into
//!   `index.html` in place of Tauri's own internals.
//!
//! Environment: `LETTERA_HOME` (config and database, required: never point it
//! at the real ones), `BRIDGE_PORT` (8977), `BRIDGE_PICK` (files `pick_files`
//! returns, `:`-separated) and `BRIDGE_SAVE` (where attachments are saved).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lettera_core::config::Paths;
use lettera_core::engine::Engine;
use lettera_core::events::EventSink;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;

#[derive(Default)]
struct Sink {
    events: Mutex<Vec<Value>>,
    notify: Notify,
}

impl EventSink for Sink {
    fn emit(&self, name: &str, payload: Value) {
        let mut events = self.events.lock().unwrap();
        let n = events.len();
        events.push(json!({ "n": n, "event": name, "payload": payload }));
        self.notify.notify_waiters();
    }
}

type Log = Arc<Mutex<Vec<String>>>;

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("argument {key}: {e}"))
}

fn to_json<T: serde::Serialize>(v: lettera_core::error::Result<T>) -> Result<Value, String> {
    let v = v.map_err(|e| e.to_string())?;
    serde_json::to_value(v).map_err(|e| e.to_string())
}

/// The Tauri commands of `src-tauri/src/main.rs`, over the same engine.
async fn call(e: &Engine, cmd: &str, a: &Value, log: &Log) -> Result<Value, String> {
    let note = |s: String| log.lock().unwrap().push(s);
    match cmd {
        "list_accounts" => to_json(Ok(e.list_accounts())),
        "add_account" => to_json(e.add_account(arg(a, "account")?).await),
        "remove_account" => to_json(e.remove_account(&arg::<String>(a, "email")?)),
        "list_folders" => to_json(e.list_folders(&arg::<String>(a, "account")?)),
        "list_threads" => to_json(e.list_threads(
            arg(a, "folder")?,
            &arg::<String>(a, "query")?,
            arg(a, "offset")?,
            arg(a, "limit")?,
        )),
        "get_thread" => to_json(e.get_thread(&arg::<String>(a, "thread_id")?)),
        "prefetch_folder" => to_json(e.prefetch_folder(arg(a, "folder_id")?)),
        "prefetch_thread" => to_json(e.prefetch_thread(&arg::<String>(a, "thread_id")?)),
        "get_body" => to_json(e.get_body(arg(a, "message_id")?, &arg::<String>(a, "mode")?)),
        "set_flags" => to_json(e.set_flags(
            &arg::<Vec<i64>>(a, "message_ids")?,
            &arg::<Vec<String>>(a, "add")?,
            &arg::<Vec<String>>(a, "remove")?,
        )),
        "archive" => to_json(e.archive(&arg::<Vec<i64>>(a, "message_ids")?)),
        "delete" => to_json(e.delete(&arg::<Vec<i64>>(a, "message_ids")?)),
        "move_to" => to_json(e.move_to(&arg::<Vec<i64>>(a, "message_ids")?, arg(a, "folder")?)),
        "prepare_draft" => to_json(e.prepare_draft(
            &arg::<String>(a, "account")?,
            arg(a, "kind")?,
            arg(a, "source")?,
        )),
        "save_draft" => to_json(e.save_draft(&arg(a, "draft")?)),
        "store_draft" => to_json(e.store_draft(arg(a, "draft")?).await),
        "discard_draft" => to_json(e.discard_draft(&arg(a, "draft")?)),
        "edit_draft" => to_json(e.edit_draft(arg(a, "message_id")?).await),
        "send" => to_json(e.send(arg(a, "draft")?).await),
        "list_attachments" => to_json(e.list_attachments(arg(a, "message_id")?)),
        "save_attachment" => {
            let id: i64 = arg(a, "attachment_id")?;
            let name = e.db().attachment(id).map_err(|x| x.to_string())?.filename;
            let dir = std::env::var("BRIDGE_SAVE").map_err(|_| "BRIDGE_SAVE is not set")?;
            let path = PathBuf::from(dir).join(name);
            e.save_attachment(id, &path)
                .await
                .map_err(|x| x.to_string())?;
            note(format!("saved {}", path.display()));
            Ok(json!(path.display().to_string()))
        }
        "pick_files" => {
            let files = std::env::var("BRIDGE_PICK").unwrap_or_default();
            Ok(json!(files
                .split(':')
                .filter(|f| !f.is_empty())
                .collect::<Vec<_>>()))
        }
        "sync_now" => {
            e.sync_now(arg::<Option<String>>(a, "account")?.as_deref());
            Ok(Value::Null)
        }
        "get_theme" => to_json(Ok(e.get_theme())),
        "open_config" => {
            let path = e.ensure_config_file().map_err(|x| x.to_string())?;
            note(format!("open_config {}", path.display()));
            Ok(json!(path.display().to_string()))
        }
        "open_url" => {
            let url = arg::<String>(a, "url")?;
            let clean = arg::<Option<bool>>(a, "clean")?.unwrap_or(false);
            let target = if clean {
                lettera_core::privacy::clean(&url).url
            } else {
                url.clone()
            };
            note(format!("open_url {target}"));
            Ok(json!(target != url))
        }
        "start_google_auth" => {
            note("start_google_auth".into());
            Ok(Value::Null)
        }
        "enable_images" => {
            e.enable_images(
                arg(a, "message_id")?,
                arg::<Option<bool>>(a, "all")?.unwrap_or(false),
            );
            Ok(Value::Null)
        }
        "privacy_report" => to_json(e.privacy_report(arg(a, "message_id")?).await),
        "get_protection" => to_json(Ok(e.protection())),
        "set_protection" => to_json(e.set_protection(arg(a, "on")?)),
        "suggest_contacts" => to_json(e.suggest_contacts(&arg::<String>(a, "query")?)),
        "app_info" => Ok(json!({
            "name": "lettera",
            "version": "bridge",
            "license": "",
            "config": e.config_path().display().to_string(),
        })),
        "quit" => {
            note("quit".into());
            Ok(Value::Null)
        }
        other => Err(format!("unknown command {other}")),
    }
}

/// Reads one request: (method, path, body).
async fn read_request(sock: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = sock.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..end]).to_string();
        let len = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while buf.len() < end + 4 + len {
            let n = sock.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
            buf.extend_from_slice(&chunk[..n]);
        }
        let mut first = head.lines().next()?.split(' ');
        let method = first.next()?.to_string();
        let path = first.next()?.to_string();
        return Some((method, path, buf[end + 4..end + 4 + len].to_vec()));
    }
}

struct Ctx {
    engine: Engine,
    sink: Arc<Sink>,
    log: Log,
    dist: PathBuf,
    shim: String,
}

async fn respond(ctx: &Ctx, method: &str, path: &str, body: &[u8]) -> (u16, &'static str, Vec<u8>) {
    const JSON: &str = "application/json";
    if method == "POST" {
        if let Some(cmd) = path.strip_prefix("/invoke/") {
            let args: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let started = std::time::Instant::now();
            let res = call(&ctx.engine, cmd, &args, &ctx.log).await;
            tracing::info!(
                "invoke {cmd}: {:?} {}",
                started.elapsed(),
                if res.is_ok() { "ok" } else { "error" }
            );
            return match res {
                Ok(v) => (200, JSON, v.to_string().into_bytes()),
                Err(e) => (400, JSON, json!(e).to_string().into_bytes()),
            };
        }
    }
    if let Some(query) = path.strip_prefix("/events") {
        let since: usize = query
            .split("since=")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let notified = ctx.sink.notify.notified();
            let new: Vec<Value> = ctx
                .sink
                .events
                .lock()
                .unwrap()
                .iter()
                .skip(since)
                .cloned()
                .collect();
            if !new.is_empty() || tokio::time::timeout_at(deadline, notified).await.is_err() {
                return (200, JSON, Value::Array(new).to_string().into_bytes());
            }
        }
    }
    if path == "/log" {
        return (
            200,
            JSON,
            json!(*ctx.log.lock().unwrap()).to_string().into_bytes(),
        );
    }
    let rel = path.split('?').next().unwrap_or("").trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.split('/').any(|p| p == "..") {
        return (404, "text/plain", b"not found".to_vec());
    }
    let Ok(bytes) = std::fs::read(ctx.dist.join(rel)) else {
        return (404, "text/plain", b"not found".to_vec());
    };
    let kind = match rel.rsplit('.').next() {
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("html") => "text/html",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    if rel == "index.html" {
        let page = String::from_utf8_lossy(&bytes).replacen(
            "<head>",
            &format!("<head><script>{}</script>", ctx.shim),
            1,
        );
        return (200, kind, page.into_bytes());
    }
    (200, kind, bytes)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let home = PathBuf::from(
        std::env::var("LETTERA_HOME").expect("LETTERA_HOME: a copy, never the real data"),
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shim = std::fs::read_to_string(root.join("tools/ui/shim.js")).expect("tools/ui/shim.js");
    let sink = Arc::new(Sink::default());
    let paths = Paths {
        config_file: home.join("config.toml"),
        data_dir: home,
    };
    let opener: lettera_core::engine::Opener =
        Arc::new(|url: String| tracing::info!("would open {url}"));
    let engine = Engine::start(
        tokio::runtime::Handle::current(),
        paths,
        sink.clone(),
        opener,
    )
    .expect("engine");
    let ctx = Arc::new(Ctx {
        engine,
        sink,
        log: Log::default(),
        dist: root.join("dist"),
        shim,
    });
    let port = std::env::var("BRIDGE_PORT").unwrap_or_else(|_| "8977".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .expect("port");
    tracing::info!("bridge ready on http://127.0.0.1:{port}/");
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            continue;
        };
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let Some((method, path, body)) = read_request(&mut sock).await else {
                return;
            };
            let (status, kind, out) = respond(&ctx, &method, &path, &body).await;
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
                out.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&out).await;
        });
    }
}
