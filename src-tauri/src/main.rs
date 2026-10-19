//! Tauri shell: thin commands over `lettera_core::Engine`, plus events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use lettera_core::config::Paths;
use lettera_core::events::EventSink;
use lettera_core::model::{
    AccountInfo, Addr, AttachmentInfo, BodyView, Draft, DraftKind, FolderInfo, NewAccount,
    ThreadPage, ThreadView,
};
use lettera_core::theme::ThemePayload;
use lettera_core::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
#[cfg(not(target_os = "linux"))]
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct Sink {
    app: AppHandle,
    /// PNG of the app icon on disk, for notifications.
    icon: Option<PathBuf>,
}

/// What a notification says: one message, or a summary of several.
fn notification_text(items: &[serde_json::Value]) -> (String, String) {
    let field = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    match items {
        [one] => (field(one, "from"), field(one, "subject")),
        _ => {
            let mut body: Vec<String> = items
                .iter()
                .take(4)
                .map(|m| format!("{} — {}", field(m, "from"), field(m, "subject")))
                .collect();
            if items.len() > 4 {
                body.push(format!("and {} more", items.len() - 4));
            }
            (format!("{} new messages", items.len()), body.join("\n"))
        }
    }
}

/// Brings the window to the front and asks the UI to open a message.
fn open_from_notification(app: &AppHandle, account: &str, id: i64) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit(
        "open_message",
        serde_json::json!({ "account": account, "id": id }),
    );
}

impl Sink {
    fn notify(&self, payload: &serde_json::Value) {
        let items = payload["items"].as_array().cloned().unwrap_or_default();
        let Some(first) = items.first() else {
            return;
        };
        let (title, body) = notification_text(&items);
        let account = payload["account"].as_str().unwrap_or("").to_string();
        let id = first["id"].as_i64().unwrap_or(0);

        #[cfg(target_os = "linux")]
        {
            let mut n = notify_rust::Notification::new();
            n.appname("lettera")
                .summary(&title)
                .body(&body)
                .action("default", "Open");
            if let Some(icon) = &self.icon {
                n.icon(&icon.display().to_string());
            }
            match n.show() {
                Ok(handle) => {
                    let app = self.app.clone();
                    // blocks until the notification is clicked or closed
                    std::thread::spawn(move || {
                        handle.wait_for_action(|action| {
                            if action == "default" {
                                open_from_notification(&app, &account, id);
                            }
                        });
                    });
                }
                Err(e) => tracing::debug!("notification: {e}"),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (&account, id, &self.icon);
            if let Err(e) = self
                .app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show()
            {
                tracing::debug!("notification: {e}");
            }
        }
    }
}

impl EventSink for Sink {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        if name == "new_mail" {
            self.notify(&payload);
        }
        if let Err(e) = self.app.emit(name, payload) {
            tracing::warn!("emit {name}: {e}");
        }
    }
}

/// Writes the icon next to the data so the notification daemon can read it.
fn notification_icon(paths: &Paths) -> Option<PathBuf> {
    let path = paths.data_dir.join("icon.png");
    let bytes = include_bytes!("../icons/128x128.png");
    if std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
        lettera_core::config::private_dir(&paths.data_dir).ok()?;
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

#[tauri::command(rename_all = "snake_case")]
fn list_accounts(engine: State<Engine>) -> Vec<AccountInfo> {
    engine.list_accounts()
}

#[tauri::command(rename_all = "snake_case")]
async fn add_account(engine: State<'_, Engine>, account: NewAccount) -> Res<()> {
    engine.add_account(account).await.map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn remove_account(engine: State<Engine>, email: String) -> Res<()> {
    engine.remove_account(&email).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn list_folders(engine: State<Engine>, account: String) -> Res<Vec<FolderInfo>> {
    engine.list_folders(&account).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn list_threads(
    engine: State<Engine>,
    folder: i64,
    query: String,
    offset: i64,
    limit: i64,
) -> Res<ThreadPage> {
    engine
        .list_threads(folder, &query, offset, limit)
        .map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn get_thread(engine: State<Engine>, thread_id: String) -> Res<ThreadView> {
    engine.get_thread(&thread_id).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn get_body(engine: State<Engine>, message_id: i64, mode: String) -> Res<BodyView> {
    engine.get_body(message_id, &mode).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn set_flags(
    engine: State<Engine>,
    message_ids: Vec<i64>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Res<()> {
    engine.set_flags(&message_ids, &add, &remove).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn archive(engine: State<Engine>, message_ids: Vec<i64>) -> Res<()> {
    engine.archive(&message_ids).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn delete(engine: State<Engine>, message_ids: Vec<i64>) -> Res<()> {
    engine.delete(&message_ids).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn move_to(engine: State<Engine>, message_ids: Vec<i64>, folder: i64) -> Res<()> {
    engine.move_to(&message_ids, folder).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn prepare_draft(
    engine: State<Engine>,
    account: String,
    kind: DraftKind,
    source: Option<i64>,
) -> Res<Draft> {
    engine.prepare_draft(&account, kind, source).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn save_draft(engine: State<Engine>, draft: Draft) -> Res<i64> {
    engine.save_draft(&draft).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn discard_draft(engine: State<Engine>, id: i64) -> Res<()> {
    engine.discard_draft(id).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
async fn send(engine: State<'_, Engine>, draft: Draft) -> Res<()> {
    engine.send(draft).await.map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn list_attachments(engine: State<Engine>, message_id: i64) -> Res<Vec<AttachmentInfo>> {
    engine.list_attachments(message_id).map_err(err)
}

/// Saves an attachment where the user chooses in the system dialog. The
/// webview never names a path itself, so it cannot write files on its own.
/// Returns the chosen path, or `None` if the user cancelled.
#[tauri::command(rename_all = "snake_case")]
async fn save_attachment(
    app: AppHandle,
    engine: State<'_, Engine>,
    attachment_id: i64,
) -> Res<Option<String>> {
    let name = engine.db().attachment(attachment_id).map_err(err)?.filename;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_file_name(name).blocking_save_file()
    })
    .await
    .map_err(err)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    engine
        .save_attachment(attachment_id, &path)
        .await
        .map_err(err)?;
    Ok(Some(path.display().to_string()))
}

/// Lets the user pick files to attach.
#[tauri::command(rename_all = "snake_case")]
async fn pick_files(app: AppHandle) -> Res<Vec<String>> {
    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_files())
            .await
            .map_err(err)?;
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
        .collect())
}

#[tauri::command(rename_all = "snake_case")]
fn sync_now(engine: State<Engine>, account: Option<String>) {
    engine.sync_now(account.as_deref());
}

#[tauri::command(rename_all = "snake_case")]
fn get_theme(engine: State<Engine>) -> ThemePayload {
    engine.get_theme()
}

/// On Linux outside Flatpak, `gio open` first: unlike xdg-open on desktops
/// it does not know (Hyprland & co.), GIO starts terminal editors such as
/// nvim inside a terminal (via xdg-terminal-exec) instead of with no tty.
fn open_with_gio(path: &std::path::Path) -> bool {
    if !cfg!(target_os = "linux") || std::env::var_os("FLATPAK_ID").is_some() {
        return false;
    }
    match std::process::Command::new("gio")
        .arg("open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => true,
        Ok(s) => {
            tracing::debug!("gio open: {s}");
            false
        }
        Err(e) => {
            tracing::debug!("gio open: {e}");
            false
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
async fn open_config(app: AppHandle, engine: State<'_, Engine>) -> Res<String> {
    let path = engine.ensure_config_file().map_err(err)?;
    let p = path.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || open_with_gio(&p))
        .await
        .map_err(err)?;
    if !opened {
        app.opener()
            .open_path(path.display().to_string(), None::<&str>)
            .map_err(err)?;
    }
    Ok(path.display().to_string())
}

/// Only web and mail links leave the app: never `file:`, `javascript:`,
/// custom schemes or anything a browser would run or open locally.
fn openable(url: &str) -> bool {
    let url = url.trim_start().to_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| url.starts_with(s))
        && !url.chars().any(char::is_control)
}

#[tauri::command(rename_all = "snake_case")]
fn open_url(app: AppHandle, url: String) -> Res<()> {
    if !openable(&url) {
        return Err(format!("unsupported link: {url}"));
    }
    app.opener()
        .open_url(url.trim_start(), None::<&str>)
        .map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn start_google_auth(engine: State<Engine>, account: NewAccount) -> Res<()> {
    engine.start_google_auth(account).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
fn enable_images(engine: State<Engine>, message_id: i64) {
    engine.enable_images(message_id);
}

#[tauri::command(rename_all = "snake_case")]
fn suggest_contacts(engine: State<Engine>, query: String) -> Res<Vec<Addr>> {
    engine.suggest_contacts(&query).map_err(err)
}

#[derive(Serialize)]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    license: &'static str,
    config: String,
}

#[tauri::command(rename_all = "snake_case")]
fn app_info(engine: State<Engine>) -> AppInfo {
    AppInfo {
        name: "lettera",
        version: env!("CARGO_PKG_VERSION"),
        license: env!("CARGO_PKG_LICENSE"),
        config: engine.config_path().display().to_string(),
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lettera=info,lettera_core=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let tauri::async_runtime::RuntimeHandle::Tokio(rt) = tauri::async_runtime::handle();
            let opener_app = handle.clone();
            let opener: lettera_core::engine::Opener = Arc::new(move |url: String| {
                if let Err(e) = opener_app.opener().open_url(url, None::<&str>) {
                    tracing::warn!("opening browser: {e}");
                }
            });
            let paths = Paths::detect()?;
            let sink = Sink {
                icon: notification_icon(&paths),
                app: handle,
            };
            let engine = Engine::start(rt, paths, Arc::new(sink), opener)?;
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_accounts,
            add_account,
            remove_account,
            list_folders,
            list_threads,
            get_thread,
            get_body,
            set_flags,
            archive,
            delete,
            move_to,
            prepare_draft,
            save_draft,
            discard_draft,
            send,
            list_attachments,
            save_attachment,
            pick_files,
            sync_now,
            get_theme,
            open_config,
            open_url,
            start_google_auth,
            enable_images,
            suggest_contacts,
            app_info,
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::openable;

    #[test]
    fn only_web_and_mail_links_open() {
        for ok in [
            "https://example.com/a?b=1",
            "HTTP://example.com",
            "mailto:a@b.it",
            " https://x.it",
        ] {
            assert!(openable(ok), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "vbscript:x",
            "ftp://x",
            "smb://host/share",
            "#anchor",
            "/relative",
            "https://x.it/\nfile:///etc",
            "https://x.it/\u{0}",
        ] {
            assert!(!openable(bad), "{bad}");
        }
    }

    #[test]
    fn notification_text_single_and_grouped() {
        let m = |f: &str, s: &str| serde_json::json!({ "from": f, "subject": s, "id": 1 });
        assert_eq!(
            super::notification_text(&[m("Anna", "Q4")]),
            ("Anna".to_string(), "Q4".to_string())
        );
        let many: Vec<_> = (0..6).map(|i| m(&format!("P{i}"), "s")).collect();
        let (title, body) = super::notification_text(&many);
        assert_eq!(title, "6 new messages");
        assert_eq!(body.lines().count(), 5);
        assert!(body.starts_with("P0 — s") && body.ends_with("and 2 more"));
    }
}
