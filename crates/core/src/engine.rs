//! The engine: owns the database, the config and one set of background
//! workers per account. Every public method answers from SQLite right away;
//! network work is queued and results arrive as events.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::config::{self, AccountConfig, AccountKind, Config, Paths};
use crate::db::{has_flag, Db, FolderRow, MessageRow};
use crate::error::{Error, Result};
use crate::events::{Event, EventSink, NewMailItem, SyncState};
use crate::imap::{self, Credentials, ImapClient, RealImap};
use crate::model::{
    AccountInfo, Addr, AttachmentInfo, BodyView, Draft, DraftKind, FolderInfo, NewAccount,
    ThreadPage, ThreadView,
};
use crate::oauth::{self, AccessToken};
use crate::parse;
use crate::reading;
use crate::secrets;
use crate::smtp::{self, Outgoing};
use crate::sync;
use crate::theme::{self, ThemePayload};

const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PREFETCH_DAYS: i64 = 30;
const PREFETCH_LIMIT: i64 = 150;

pub type Opener = Arc<dyn Fn(String) + Send + Sync>;

/// Work for the foreground connection (user actions, bodies on demand).
enum Action {
    Body(i64),
    Flags {
        folder: FolderRow,
        uids: Vec<u32>,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Move {
        from: FolderRow,
        uids: Vec<u32>,
        dest: String,
        create: bool,
    },
    Expunge {
        folder: FolderRow,
        uids: Vec<u32>,
    },
    Append {
        folder: String,
        create: bool,
        raw: Vec<u8>,
    },
    Raw {
        folder: String,
        uid: u32,
        reply: oneshot::Sender<Result<Option<Vec<u8>>>>,
    },
}

/// Work for the background connection.
#[derive(Debug, Clone, Default)]
struct SyncJob {
    /// Empty = every folder (and the folder list).
    folders: Vec<i64>,
    inbox: bool,
    notify: bool,
}

struct Worker {
    config: AccountConfig,
    actions: mpsc::UnboundedSender<Action>,
    syncs: mpsc::UnboundedSender<SyncJob>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

struct Inner {
    rt: Handle,
    db: Db,
    paths: Paths,
    sink: Arc<dyn EventSink>,
    opener: Opener,
    config: RwLock<Config>,
    theme: Mutex<ThemePayload>,
    workers: Mutex<HashMap<String, Worker>>,
    tokens: tokio::sync::Mutex<HashMap<String, AccessToken>>,
    images: Mutex<HashSet<i64>>,
    /// (folder id, uid) of messages an action is moving away right now.
    pending: Mutex<HashSet<(i64, u32)>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Inner {
    fn emit(&self, e: Event) {
        self.sink.emit(e.name(), e.payload());
    }

    fn status(&self, account: &str, state: SyncState, message: impl Into<String>) {
        self.emit(Event::SyncStatus {
            account: account.to_string(),
            state,
            message: message.into(),
        });
    }

    fn account(&self, email: &str) -> Result<AccountConfig> {
        self.config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .accounts
            .iter()
            .find(|a| a.email == email)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("account {email}")))
    }

    async fn secret(&self, email: &str) -> Result<String> {
        let email = email.to_string();
        tokio::task::spawn_blocking(move || secrets::get(&email))
            .await
            .map_err(|e| Error::Other(e.to_string()))?
    }

    async fn access_token(&self, acc: &AccountConfig) -> Result<String> {
        let mut tokens = self.tokens.lock().await;
        if let Some(t) = tokens.get(&acc.email) {
            if t.fresh() {
                return Ok(t.token.clone());
            }
        }
        let refresh = self
            .secret(&acc.email)
            .await
            .map_err(|_| Error::Auth("google sign-in needed: use :add account".into()))?;
        let t = oauth::refresh(
            acc.client_id.as_deref().unwrap_or(""),
            acc.client_secret.as_deref().unwrap_or(""),
            &refresh,
        )
        .await?;
        let token = t.token.clone();
        tokens.insert(acc.email.clone(), t);
        Ok(token)
    }

    async fn credentials(&self, acc: &AccountConfig) -> Result<Credentials> {
        Ok(match acc.kind {
            AccountKind::Imap => Credentials::Password {
                username: acc.username().to_string(),
                password: self.secret(&acc.email).await?,
            },
            AccountKind::Google => Credentials::XOAuth2 {
                user: acc.email.clone(),
                access_token: self.access_token(acc).await?,
            },
        })
    }

    async fn connect(&self, email: &str) -> Result<RealImap> {
        let acc = self.account(email)?;
        let creds = self.credentials(&acc).await?;
        imap::connect(&acc.imap()?, &creds).await
    }

    fn pending_uids(&self, folder_id: i64) -> HashSet<u32> {
        lock(&self.pending)
            .iter()
            .filter(|(f, _)| *f == folder_id)
            .map(|(_, u)| *u)
            .collect()
    }

    fn set_pending(&self, folder_id: i64, uids: &[u32], on: bool) {
        let mut p = lock(&self.pending);
        for u in uids {
            if on {
                p.insert((folder_id, *u));
            } else {
                p.remove(&(folder_id, *u));
            }
        }
    }

    fn request_sync(&self, email: &str, job: SyncJob) {
        if let Some(w) = lock(&self.workers).get(email) {
            let _ = w.syncs.send(job);
        }
    }

    fn act(&self, email: &str, a: Action) -> Result<()> {
        lock(&self.workers)
            .get(email)
            .ok_or_else(|| Error::NotFound(format!("account {email} is not running")))?
            .actions
            .send(a)
            .map_err(|_| Error::Other("worker fermo".into()))
    }

    // ------------------------------------------------------------- background

    async fn run_sync(
        self: &Arc<Self>,
        email: &str,
        conn: &mut Option<RealImap>,
        job: &SyncJob,
    ) -> Result<()> {
        if conn.is_none() {
            *conn = Some(self.connect(email).await?);
        }
        let Some(c) = conn.as_mut() else {
            return Ok(());
        };
        let account_id = self.db.account_id(email)?;
        let since = {
            let cfg = self.config.read().unwrap_or_else(|p| p.into_inner());
            let months = cfg
                .accounts
                .iter()
                .find(|a| a.email == email)
                .and_then(|a| cfg.sync_months(a));
            config::sync_since(chrono::Local::now().date_naive(), months)
        };
        let folders = if job.folders.is_empty() && !job.inbox {
            sync::sync_folder_list(c, &self.db, account_id).await?
        } else {
            let mut all = self.db.folders(account_id)?;
            if all.is_empty() {
                all = sync::sync_folder_list(c, &self.db, account_id).await?;
            }
            all.into_iter()
                .filter(|f| {
                    job.folders.contains(&f.id) || (job.inbox && f.role.as_deref() == Some("inbox"))
                })
                .collect()
        };
        for f in &folders {
            let skip = self.pending_uids(f.id);
            let r = sync::sync_folder_since(c, &self.db, f, &skip, since).await?;
            if r.changed {
                self.emit(Event::MessagesChanged {
                    account: email.to_string(),
                    folder: f.id,
                });
            }
            if f.role.as_deref() == Some("inbox") {
                if job.notify {
                    let mut items: Vec<_> = self
                        .db
                        .messages(&r.new_ids)?
                        .into_iter()
                        .filter(|m| !has_flag(&m.flags, "\\Seen"))
                        .collect();
                    items.sort_by_key(|m| std::cmp::Reverse(m.date));
                    if !items.is_empty() {
                        self.emit(Event::NewMail {
                            account: email.to_string(),
                            items: items
                                .into_iter()
                                .map(|m| NewMailItem {
                                    id: m.id,
                                    thread_id: m.thread_id.clone(),
                                    from: m.from.display(),
                                    subject: m.subject.clone(),
                                })
                                .collect(),
                        });
                    }
                }
                let recent = chrono::Utc::now().timestamp() - PREFETCH_DAYS * 86_400;
                let fresh = self.db.folder(f.id)?;
                for id in sync::prefetch_bodies(c, &self.db, &fresh, recent, PREFETCH_LIMIT).await?
                {
                    self.emit(Event::BodyReady { id });
                }
            }
        }
        Ok(())
    }

    async fn sync_worker(self: Arc<Self>, email: String, mut rx: mpsc::UnboundedReceiver<SyncJob>) {
        let mut conn: Option<RealImap> = None;
        let mut tick = tokio::time::interval(SYNC_INTERVAL);
        let mut failures = 0u32;
        loop {
            let mut job = tokio::select! {
                j = rx.recv() => match j { Some(j) => j, None => break },
                _ = tick.tick() => SyncJob::default(),
            };
            // coalesce whatever else queued up meanwhile
            while let Ok(more) = rx.try_recv() {
                if more.folders.is_empty() && !more.inbox || job.folders.is_empty() && !job.inbox {
                    job = SyncJob {
                        notify: job.notify || more.notify,
                        ..SyncJob::default()
                    };
                } else {
                    job.folders.extend(more.folders);
                    job.inbox |= more.inbox;
                    job.notify |= more.notify;
                }
            }
            self.status(&email, SyncState::Syncing, "sync");
            let mut res = self.run_sync(&email, &mut conn, &job).await;
            if res.is_err() && conn.is_some() {
                // stale connection: one fresh attempt
                conn = None;
                res = self.run_sync(&email, &mut conn, &job).await;
            }
            match res {
                Ok(()) => {
                    failures = 0;
                    self.status(&email, SyncState::Idle, "");
                }
                Err(e) => {
                    conn = None;
                    failures += 1;
                    tracing::warn!("sync {email}: {e}");
                    self.status(&email, SyncState::Error, e.to_string());
                    let wait = Duration::from_secs((30u64 << failures.min(4)).min(300));
                    let me = self.clone();
                    let em = email.clone();
                    self.rt.spawn(async move {
                        tokio::time::sleep(wait).await;
                        me.request_sync(&em, SyncJob::default());
                    });
                }
            }
        }
        if let Some(c) = conn {
            c.logout().await;
        }
    }

    async fn idle_once(&self, email: &str) -> Result<()> {
        let mut c = self.connect(email).await?;
        if !c.has_capability("IDLE") {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                c.noop().await?;
                self.request_sync(
                    email,
                    SyncJob {
                        inbox: true,
                        notify: true,
                        ..Default::default()
                    },
                );
            }
        }
        c.select("INBOX").await?;
        let mut session = c.into_session();
        loop {
            let mut handle = session.idle();
            handle.init().await?;
            let res = {
                let (wait, _stop) = handle.wait_with_timeout(Duration::from_secs(25 * 60));
                wait.await?
            };
            session = handle.done().await?;
            if let async_imap::extensions::idle::IdleResponse::NewData(_) = res {
                self.request_sync(
                    email,
                    SyncJob {
                        inbox: true,
                        notify: true,
                        ..Default::default()
                    },
                );
            }
        }
    }

    async fn idle_worker(self: Arc<Self>, email: String) {
        let mut backoff = Duration::from_secs(2);
        loop {
            let started = std::time::Instant::now();
            if let Err(e) = self.idle_once(&email).await {
                tracing::info!("idle {email}: {e}");
            }
            if started.elapsed() > Duration::from_secs(120) {
                backoff = Duration::from_secs(2);
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(300));
        }
    }

    // ------------------------------------------------------------- foreground

    async fn run_action(
        &self,
        email: &str,
        conn: &mut Option<RealImap>,
        a: &mut Option<Action>,
    ) -> Result<Vec<i64>> {
        if conn.is_none() {
            *conn = Some(self.connect(email).await?);
        }
        let Some(c) = conn.as_mut() else {
            return Ok(Vec::new());
        };
        let Some(action) = a.as_ref() else {
            return Ok(Vec::new());
        };
        match action {
            Action::Body(id) => {
                let m = self.db.message(*id)?;
                sync::fetch_body(c, &self.db, &m).await?;
                self.emit(Event::BodyReady { id: *id });
                Ok(Vec::new())
            }
            Action::Flags {
                folder,
                uids,
                add,
                remove,
            } => {
                sync::store_flags(c, &folder.name, uids, add, remove).await?;
                Ok(vec![folder.id])
            }
            Action::Move {
                from,
                uids,
                dest,
                create,
            } => {
                if *create {
                    if let Err(e) = c.create(dest).await {
                        tracing::debug!("create {dest}: {e} (probably exists already)");
                    }
                }
                sync::move_messages(c, &from.name, uids, dest).await?;
                let account_id = self.db.account_id(email)?;
                let folders = if *create {
                    sync::sync_folder_list(c, &self.db, account_id).await?
                } else {
                    self.db.folders(account_id)?
                };
                let mut touched = vec![from.id];
                touched.extend(folders.iter().filter(|f| &f.name == dest).map(|f| f.id));
                Ok(touched)
            }
            Action::Expunge { folder, uids } => {
                sync::expunge_messages(c, &folder.name, uids).await?;
                Ok(vec![folder.id])
            }
            Action::Append {
                folder,
                create,
                raw,
            } => {
                if *create {
                    let _ = c.create(folder).await;
                }
                c.append(folder, "\\Seen", raw).await?;
                let account_id = self.db.account_id(email)?;
                if *create {
                    sync::sync_folder_list(c, &self.db, account_id).await?;
                }
                Ok(self
                    .db
                    .folders(account_id)?
                    .into_iter()
                    .filter(|f| &f.name == folder)
                    .map(|f| f.id)
                    .collect())
            }
            Action::Raw { folder, uid, .. } => {
                let raw = sync::fetch_raw(c, folder, *uid).await?;
                if let Some(Action::Raw { reply, .. }) = a.take() {
                    let _ = reply.send(Ok(raw));
                }
                Ok(Vec::new())
            }
        }
    }

    fn release(&self, a: &Action) {
        match a {
            Action::Move { from, uids, .. } => self.set_pending(from.id, uids, false),
            Action::Expunge { folder, uids } => self.set_pending(folder.id, uids, false),
            _ => {}
        }
    }

    async fn action_worker(
        self: Arc<Self>,
        email: String,
        mut rx: mpsc::UnboundedReceiver<Action>,
    ) {
        let mut conn: Option<RealImap> = None;
        while let Some(action) = rx.recv().await {
            let mut slot = Some(action);
            let mut res = self.run_action(&email, &mut conn, &mut slot).await;
            if res.is_err() && slot.is_some() {
                conn = None;
                res = self.run_action(&email, &mut conn, &mut slot).await;
            }
            if let Some(a) = &slot {
                self.release(a);
            }
            match res {
                Ok(touched) => {
                    if !touched.is_empty() {
                        self.request_sync(
                            &email,
                            SyncJob {
                                folders: touched,
                                ..Default::default()
                            },
                        );
                    }
                }
                Err(e) => {
                    conn = None;
                    tracing::warn!("azione {email}: {e}");
                    self.status(&email, SyncState::Error, e.to_string());
                    if let Some(Action::Raw { reply, .. }) = slot.take() {
                        let _ = reply.send(Err(e));
                    }
                    // the server is the truth: resync to undo optimistic changes
                    self.request_sync(&email, SyncJob::default());
                }
            }
        }
        if let Some(c) = conn {
            c.logout().await;
        }
    }

    fn start_worker(self: &Arc<Self>, acc: &AccountConfig) {
        let kind = if acc.is_google() { "google" } else { "imap" };
        if let Err(e) = self.db.upsert_account(&acc.email, &acc.name, kind) {
            tracing::error!("account {}: {e}", acc.email);
            return;
        }
        let (atx, arx) = mpsc::unbounded_channel();
        let (stx, srx) = mpsc::unbounded_channel();
        let tasks = vec![
            self.rt
                .spawn(self.clone().action_worker(acc.email.clone(), arx)),
            self.rt
                .spawn(self.clone().sync_worker(acc.email.clone(), srx)),
            self.rt.spawn(self.clone().idle_worker(acc.email.clone())),
        ];
        lock(&self.workers).insert(
            acc.email.clone(),
            Worker {
                config: acc.clone(),
                actions: atx,
                syncs: stx,
                tasks,
            },
        );
    }

    /// Starts/stops/restarts workers so they match the config.
    fn reconcile_accounts(self: &Arc<Self>) {
        let accounts = self
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .accounts
            .clone();
        let emails: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();
        {
            let mut workers = lock(&self.workers);
            workers.retain(|email, w| accounts.iter().any(|a| &a.email == email && a == &w.config));
        }
        for acc in &accounts {
            if let Err(e) = acc.validate() {
                self.status(&acc.email, SyncState::Error, e.to_string());
                continue;
            }
            let running = lock(&self.workers).contains_key(&acc.email);
            if !running {
                self.start_worker(acc);
            }
        }
        match self.db.accounts_not_in(&emails) {
            Ok(stale) => {
                for e in stale {
                    let _ = self.db.delete_account(&e);
                }
            }
            Err(e) => tracing::warn!("account cleanup: {e}"),
        }
    }

    fn reload(self: &Arc<Self>) {
        match config::load(&self.paths.config_file) {
            Ok(cfg) => {
                let old = self
                    .config
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                if old != cfg {
                    tracing::info!("config reloaded");
                    let resync: Vec<String> = cfg
                        .accounts
                        .iter()
                        .filter(|a| {
                            old.accounts
                                .iter()
                                .find(|o| o.email == a.email)
                                .is_some_and(|o| old.sync_months(o) != cfg.sync_months(a))
                        })
                        .map(|a| a.email.clone())
                        .collect();
                    *self.config.write().unwrap_or_else(|p| p.into_inner()) = cfg;
                    self.reconcile_accounts();
                    self.emit(Event::AccountsChanged);
                    for email in resync {
                        tracing::info!("{email}: sync window changed, resyncing");
                        self.request_sync(&email, SyncJob::default());
                    }
                }
            }
            Err(e) => {
                tracing::warn!("invalid config: {e}");
                self.status("", SyncState::Error, format!("invalid config: {e}"));
            }
        }
        let cfg = self
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let t = theme::resolve(&cfg.theme, &cfg.font, cfg.font_size);
        let mut current = lock(&self.theme);
        if *current != t {
            tracing::info!("theme updated");
            *current = t.clone();
            drop(current);
            self.emit(Event::ThemeChanged(t));
        }
    }

    fn watch(self: &Arc<Self>) {
        use notify::{RecursiveMode, Watcher};
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("watcher unavailable: {e}");
                return;
            }
        };
        if let Some(dir) = self.paths.config_file.parent() {
            let _ = config::private_dir(dir);
            if self.paths.config_file.exists() {
                let _ = config::restrict(&self.paths.config_file, 0o600);
            }
            if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                tracing::warn!("watch {}: {e}", dir.display());
            }
        }
        for dir in theme::omarchy_watch_dirs() {
            if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
                tracing::warn!("watch {}: {e}", dir.display());
            }
        }
        *lock(&self.watcher) = Some(watcher);
        let me = self.clone();
        self.rt.spawn(async move {
            while rx.recv().await.is_some() {
                tokio::time::sleep(Duration::from_millis(200)).await;
                while rx.try_recv().is_ok() {}
                me.reload();
            }
        });
    }
}

impl Engine {
    /// Opens the database, loads the config and starts the account workers.
    pub fn start(
        rt: Handle,
        paths: Paths,
        sink: Arc<dyn EventSink>,
        opener: Opener,
    ) -> Result<Engine> {
        let db = Db::open(&paths.db_file())?;
        let cfg = match config::load(&paths.config_file) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("invalid config: {e}");
                Config::default()
            }
        };
        let theme = theme::resolve(&cfg.theme, &cfg.font, cfg.font_size);
        let inner = Arc::new(Inner {
            rt,
            db,
            paths,
            sink,
            opener,
            config: RwLock::new(cfg),
            theme: Mutex::new(theme),
            workers: Mutex::new(HashMap::new()),
            tokens: tokio::sync::Mutex::new(HashMap::new()),
            images: Mutex::new(HashSet::new()),
            pending: Mutex::new(HashSet::new()),
            watcher: Mutex::new(None),
        });
        {
            let _guard = inner.rt.enter();
            inner.reconcile_accounts();
            inner.watch();
        }
        Ok(Engine { inner })
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    /// The config as last loaded.
    pub fn config(&self) -> Config {
        self.inner
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn config_path(&self) -> PathBuf {
        self.inner.paths.config_file.clone()
    }

    /// Makes sure the config file exists (for `:config`) and returns its path.
    pub fn ensure_config_file(&self) -> Result<PathBuf> {
        let p = self.config_path();
        if !p.exists() {
            config::write(&p, &config::template())?;
        }
        Ok(p)
    }

    pub fn list_accounts(&self) -> Vec<AccountInfo> {
        self.inner
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .accounts
            .iter()
            .map(|a| AccountInfo {
                email: a.email.clone(),
                name: a.name.clone(),
                kind: if a.is_google() { "google" } else { "imap" }.into(),
            })
            .collect()
    }

    fn write_config(&self, f: impl FnOnce(&str) -> String) -> Result<()> {
        let path = &self.inner.paths.config_file;
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let new = f(&text);
        let cfg = config::parse(&new)?;
        config::write(path, &new)?;
        *self.inner.config.write().unwrap_or_else(|p| p.into_inner()) = cfg;
        self.inner.reconcile_accounts();
        self.inner.emit(Event::AccountsChanged);
        Ok(())
    }

    fn account_from(n: &NewAccount) -> Result<AccountConfig> {
        let kind = match n.kind.as_str() {
            "google" => AccountKind::Google,
            "imap" => AccountKind::Imap,
            other => return Err(Error::Config(format!("unknown account type: {other}"))),
        };
        let clean = |s: &Option<String>| {
            s.as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let acc = AccountConfig {
            name: if n.name.trim().is_empty() {
                n.email.trim().to_string()
            } else {
                n.name.trim().to_string()
            },
            kind,
            email: n.email.trim().to_string(),
            imap_host: clean(&n.imap_host),
            imap_port: n.imap_port,
            smtp_host: clean(&n.smtp_host),
            smtp_port: n.smtp_port,
            username: clean(&n.username).filter(|u| u != n.email.trim()),
            client_id: clean(&n.client_id),
            client_secret: clean(&n.client_secret),
            imap_tls: clean(&n.imap_tls),
            smtp_tls: clean(&n.smtp_tls),
            sync_months: None,
            signature: None,
        };
        acc.validate()?;
        Ok(acc)
    }

    /// Adds an IMAP account after checking that the login works.
    pub async fn add_account(&self, n: NewAccount) -> Result<()> {
        let acc = Self::account_from(&n)?;
        if acc.is_google() {
            return Err(Error::Config("for google use sign in with google".into()));
        }
        if self.list_accounts().iter().any(|a| a.email == acc.email) {
            return Err(Error::Config(format!("{} already exists", acc.email)));
        }
        let password = n.password.clone().unwrap_or_default();
        if password.is_empty() {
            return Err(Error::Config("password is missing".into()));
        }
        let creds = Credentials::Password {
            username: acc.username().to_string(),
            password: password.clone(),
        };
        imap::connect(&acc.imap()?, &creds).await?.logout().await;
        let mut acc = acc;
        let smtp_ep = acc.smtp()?;
        let implicit = smtp::check(&smtp_ep, &creds).await.map_err(|e| {
            Error::Smtp(format!(
                "sending is not set up correctly ({}:{}): {e}",
                smtp_ep.host, smtp_ep.port
            ))
        })?;
        // keep what actually worked; the port-based default needs no line
        acc.smtp_tls = (implicit != (smtp_ep.port == 465))
            .then(|| if implicit { "implicit" } else { "starttls" }.to_string());
        let email = acc.email.clone();
        tokio::task::spawn_blocking(move || secrets::set(&email, &password))
            .await
            .map_err(|e| Error::Other(e.to_string()))??;
        self.write_config(|text| config::append_account(text, &acc))
    }

    pub fn remove_account(&self, email: &str) -> Result<()> {
        lock(&self.inner.workers).remove(email);
        self.write_config(|text| config::remove_account(text, email))?;
        let _ = secrets::delete(email);
        self.inner.db.delete_account(email)?;
        Ok(())
    }

    /// Runs the Google OAuth flow in the background; `auth:done` reports the outcome.
    pub fn start_google_auth(&self, n: NewAccount) -> Result<()> {
        let mut n = n;
        n.kind = "google".into();
        let acc = Self::account_from(&n)?;
        let me = self.clone();
        self.inner.rt.spawn(async move {
            let email = acc.email.clone();
            let opener = me.inner.opener.clone();
            let res = async {
                let refresh = oauth::authorize(
                    acc.client_id.as_deref().unwrap_or(""),
                    acc.client_secret.as_deref().unwrap_or(""),
                    &acc.email,
                    move |url| opener(url),
                )
                .await?;
                let e = acc.email.clone();
                tokio::task::spawn_blocking(move || secrets::set(&e, &refresh))
                    .await
                    .map_err(|e| Error::Other(e.to_string()))??;
                me.inner.tokens.lock().await.remove(&acc.email);
                let exists = me.list_accounts().iter().any(|a| a.email == acc.email);
                if exists {
                    // re-authorization of an existing account: restart its workers
                    lock(&me.inner.workers).remove(&acc.email);
                    me.inner.reconcile_accounts();
                    Ok(())
                } else {
                    me.write_config(|text| config::append_account(text, &acc))
                }
            }
            .await;
            me.inner.emit(Event::AuthDone {
                email,
                ok: res.is_ok(),
                error: res.err().map(|e| e.to_string()),
            });
        });
        Ok(())
    }

    pub fn list_folders(&self, account: &str) -> Result<Vec<FolderInfo>> {
        self.inner.db.folder_infos(account)
    }

    pub fn list_threads(
        &self,
        folder: i64,
        query: &str,
        offset: i64,
        limit: i64,
    ) -> Result<ThreadPage> {
        self.inner.db.list_threads(
            folder,
            query,
            offset,
            limit.clamp(1, 500),
            chrono::Local::now(),
        )
    }

    pub fn get_thread(&self, thread_id: &str) -> Result<ThreadView> {
        self.inner.db.thread(thread_id)
    }

    pub fn get_body(&self, id: i64, mode: &str) -> Result<BodyView> {
        let m = self.inner.db.message(id)?;
        match self.inner.db.body(id)? {
            Some((text, html)) => {
                let has_html = html.is_some();
                if mode == "html" {
                    let allow = lock(&self.inner.images).contains(&id);
                    let doc = html
                        .map(|h| reading::html_document(&h, allow))
                        .unwrap_or_else(|| {
                            reading::html_document(&format!("<pre>{}</pre>", escape(&text)), false)
                        });
                    Ok(BodyView {
                        id,
                        ready: true,
                        blocks: Vec::new(),
                        html: Some(doc),
                        has_html,
                    })
                } else {
                    Ok(BodyView {
                        id,
                        ready: true,
                        blocks: reading::blocks(&text),
                        html: None,
                        has_html,
                    })
                }
            }
            None => {
                let email = self.inner.db.account_email(m.account_id)?;
                self.inner.act(&email, Action::Body(id))?;
                Ok(BodyView {
                    id,
                    ready: false,
                    blocks: Vec::new(),
                    html: None,
                    has_html: false,
                })
            }
        }
    }

    pub fn enable_images(&self, id: i64) {
        lock(&self.inner.images).insert(id);
    }

    /// Messages grouped by folder: (folder, account email, rows).
    fn by_folder(&self, ids: &[i64]) -> Result<Vec<(FolderRow, String, Vec<MessageRow>)>> {
        let mut groups: Vec<(FolderRow, String, Vec<MessageRow>)> = Vec::new();
        for m in self.inner.db.messages(ids)? {
            if let Some(g) = groups.iter_mut().find(|g| g.0.id == m.folder_id) {
                g.2.push(m);
                continue;
            }
            let f = self.inner.db.folder(m.folder_id)?;
            let email = self.inner.db.account_email(m.account_id)?;
            groups.push((f, email, vec![m]));
        }
        Ok(groups)
    }

    fn changed(&self, email: &str, folder: i64) {
        self.inner.emit(Event::MessagesChanged {
            account: email.to_string(),
            folder,
        });
    }

    pub fn set_flags(&self, ids: &[i64], add: &[String], remove: &[String]) -> Result<()> {
        self.inner.db.change_flags(ids, add, remove)?;
        for (folder, email, rows) in self.by_folder(ids)? {
            self.changed(&email, folder.id);
            let uids = rows.iter().map(|m| m.uid).collect();
            self.inner.act(
                &email,
                Action::Flags {
                    folder,
                    uids,
                    add: add.to_vec(),
                    remove: remove.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    /// Removes rows locally and queues the server-side move/expunge.
    fn take_away(
        &self,
        folder: FolderRow,
        email: &str,
        rows: &[MessageRow],
        dest: Option<(String, bool)>,
    ) -> Result<()> {
        let uids: Vec<u32> = rows.iter().map(|m| m.uid).collect();
        let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
        self.inner.set_pending(folder.id, &uids, true);
        self.inner.db.delete_messages(&ids)?;
        self.changed(email, folder.id);
        let action = match dest {
            Some((dest, create)) => Action::Move {
                from: folder,
                uids,
                dest,
                create,
            },
            None => Action::Expunge { folder, uids },
        };
        self.inner.act(email, action)
    }

    pub fn archive(&self, ids: &[i64]) -> Result<()> {
        for (folder, email, rows) in self.by_folder(ids)? {
            let acc = self.inner.account(&email)?;
            let role = folder.role.clone().unwrap_or_default();
            if acc.gmail_semantics() {
                match role.as_str() {
                    "all" => continue,
                    "trash" | "junk" => {
                        let all = self.inner.db.folder_by_role(folder.account_id, "all")?;
                        let dest = all
                            .map(|f| f.name)
                            .unwrap_or_else(|| "[Gmail]/All Mail".into());
                        self.take_away(folder, &email, &rows, Some((dest, false)))?;
                    }
                    // Gmail: archiving = dropping the label; it stays in All Mail.
                    _ => self.take_away(folder, &email, &rows, None)?,
                }
            } else {
                if role == "archive" {
                    continue;
                }
                let dest = self.inner.db.folder_by_role(folder.account_id, "archive")?;
                let (name, create) = match dest {
                    Some(f) => (f.name, false),
                    None => ("Archive".to_string(), true),
                };
                self.take_away(folder, &email, &rows, Some((name, create)))?;
            }
        }
        Ok(())
    }

    pub fn delete(&self, ids: &[i64]) -> Result<()> {
        for (folder, email, rows) in self.by_folder(ids)? {
            if folder.role.as_deref() == Some("trash") {
                self.take_away(folder, &email, &rows, None)?;
                continue;
            }
            let acc = self.inner.account(&email)?;
            let (name, create) = match self.inner.db.folder_by_role(folder.account_id, "trash")? {
                Some(f) => (f.name, false),
                None if acc.gmail_semantics() => ("[Gmail]/Trash".to_string(), false),
                None => ("Trash".to_string(), true),
            };
            self.take_away(folder, &email, &rows, Some((name, create)))?;
        }
        Ok(())
    }

    pub fn move_to(&self, ids: &[i64], dest_folder: i64) -> Result<()> {
        let dest = self.inner.db.folder(dest_folder)?;
        for (folder, email, rows) in self.by_folder(ids)? {
            if folder.id == dest.id || folder.account_id != dest.account_id {
                continue;
            }
            self.take_away(folder, &email, &rows, Some((dest.name.clone(), false)))?;
        }
        Ok(())
    }

    pub fn sync_now(&self, account: Option<&str>) {
        let emails: Vec<String> = match account {
            Some(a) => vec![a.to_string()],
            None => self.list_accounts().into_iter().map(|a| a.email).collect(),
        };
        for e in emails {
            self.inner.request_sync(&e, SyncJob::default());
        }
    }

    pub fn get_theme(&self) -> ThemePayload {
        lock(&self.inner.theme).clone()
    }

    pub fn suggest_contacts(&self, q: &str) -> Result<Vec<Addr>> {
        self.inner.db.suggest_contacts(q, 8)
    }

    pub fn list_attachments(&self, id: i64) -> Result<Vec<AttachmentInfo>> {
        self.inner.db.attachments(id)
    }

    async fn raw_message(&self, m: &MessageRow) -> Result<Vec<u8>> {
        let folder = self.inner.db.folder(m.folder_id)?;
        let email = self.inner.db.account_email(m.account_id)?;
        let (tx, rx) = oneshot::channel();
        self.inner.act(
            &email,
            Action::Raw {
                folder: folder.name,
                uid: m.uid,
                reply: tx,
            },
        )?;
        rx.await
            .map_err(|_| Error::Other("worker fermo".into()))??
            .ok_or_else(|| Error::NotFound("message is no longer on the server".into()))
    }

    pub async fn save_attachment(&self, attachment_id: i64, path: &Path) -> Result<()> {
        let a = self.inner.db.attachment(attachment_id)?;
        let m = self.inner.db.message(a.message_id)?;
        let raw = self.raw_message(&m).await?;
        let bytes = parse::extract_attachment(&raw, a.part_index)
            .ok_or_else(|| Error::NotFound(format!("attachment {}", a.filename)))?;
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    /// A local draft to resume, or a fresh one built from the source message.
    pub fn prepare_draft(
        &self,
        account: &str,
        kind: DraftKind,
        source: Option<i64>,
    ) -> Result<Draft> {
        if let Some(d) = self.inner.db.find_draft(account, kind, source)? {
            return Ok(d);
        }
        let body = {
            let cfg = self.inner.config.read().unwrap_or_else(|p| p.into_inner());
            let sig = cfg
                .accounts
                .iter()
                .find(|a| a.email == account)
                .and_then(|a| cfg.signature(a));
            config::signed_body(sig.as_deref())
        };
        let Some(src_id) = source.filter(|_| kind != DraftKind::New) else {
            return Ok(Draft {
                account: account.to_string(),
                kind: DraftKind::New,
                body,
                ..Default::default()
            });
        };
        let src = self.inner.db.message(src_id)?;
        let text = self
            .inner
            .db
            .body(src_id)?
            .map(|(t, _)| t)
            .unwrap_or_default();
        let t = smtp::template(kind, &src, &text, account);
        let join = |v: &[Addr]| v.iter().map(Addr::full).collect::<Vec<_>>().join(", ");
        Ok(Draft {
            id: None,
            account: account.to_string(),
            kind,
            source: Some(src_id),
            to: join(&t.to),
            cc: join(&t.cc),
            bcc: String::new(),
            subject: t.subject,
            body,
            quote: t.quote,
            attachments: Vec::new(),
            saved_at: None,
        })
    }

    pub fn save_draft(&self, d: &Draft) -> Result<i64> {
        self.inner.db.save_draft(d)
    }

    pub fn discard_draft(&self, id: i64) -> Result<()> {
        self.inner.db.delete_draft(id)
    }

    pub async fn send(&self, d: Draft) -> Result<()> {
        let acc = self.inner.account(&d.account)?;
        let src = match d.source {
            Some(id) if d.kind != DraftKind::New => Some(self.inner.db.message(id)?),
            _ => None,
        };
        let (in_reply_to, references) = match (&src, d.kind) {
            (Some(s), DraftKind::Reply | DraftKind::ReplyAll) => smtp::thread_headers(s),
            _ => (None, Vec::new()),
        };
        let mut body = d.body.trim_end().to_string();
        if !d.quote.trim().is_empty() {
            body.push_str("\n\n");
            body.push_str(d.quote.trim_end());
        }
        body.push('\n');
        let mut attachments = Vec::new();
        for p in &d.attachments {
            let bytes = tokio::fs::read(p).await?;
            let name = Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".into());
            let mime = smtp::guess_mime(&name).to_string();
            attachments.push((name, mime, bytes));
        }
        let out = Outgoing {
            from: Addr {
                name: acc.name.clone(),
                addr: acc.email.clone(),
            },
            to: parse::parse_recipients(&d.to),
            cc: parse::parse_recipients(&d.cc),
            bcc: parse::parse_recipients(&d.bcc),
            subject: d.subject.trim().to_string(),
            body,
            message_id: smtp::new_message_id(&acc.email),
            in_reply_to,
            references,
            attachments,
        };
        let raw = smtp::build(&out)?;
        let creds = self.inner.credentials(&acc).await?;
        smtp::send(&acc.smtp()?, &creds, &out, &raw).await?;
        tracing::info!("sent {} from {}", out.message_id, acc.email);

        let account_id = self.inner.db.account_id(&acc.email)?;
        let sent = self.inner.db.folder_by_role(account_id, "sent")?;
        if acc.gmail_semantics() {
            // Gmail files sent mail by itself.
            if let Some(f) = sent {
                self.inner.request_sync(
                    &acc.email,
                    SyncJob {
                        folders: vec![f.id],
                        ..Default::default()
                    },
                );
            }
        } else {
            let (folder, create) = match sent {
                Some(f) => (f.name, false),
                None => ("Sent".to_string(), true),
            };
            self.inner.act(
                &acc.email,
                Action::Append {
                    folder,
                    create,
                    raw,
                },
            )?;
        }
        if let (Some(s), DraftKind::Reply | DraftKind::ReplyAll) = (&src, d.kind) {
            let _ = self.set_flags(&[s.id], &["\\Answered".to_string()], &[]);
        }
        if let Some(id) = d.id {
            self.inner.db.delete_draft(id)?;
        }
        Ok(())
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
