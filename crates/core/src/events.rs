//! Events pushed from the engine to whoever hosts it (the Tauri shell).

use serde::Serialize;
use serde_json::{json, Value};

use crate::theme::ThemePayload;

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, name: &str, payload: Value);
}

/// Sink that drops everything: handy for tests and headless use.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _name: &str, _payload: Value) {}
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    Idle,
    Syncing,
    Error,
}

#[derive(Debug, Clone)]
pub enum Event {
    MessagesChanged {
        account: String,
        folder: i64,
    },
    SyncStatus {
        account: String,
        state: SyncState,
        message: String,
    },
    ThemeChanged(ThemePayload),
    NewMail {
        from: String,
        subject: String,
    },
    AuthDone {
        email: String,
        ok: bool,
        error: Option<String>,
    },
    BodyReady {
        id: i64,
    },
    AccountsChanged,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::MessagesChanged { .. } => "messages:changed",
            Event::SyncStatus { .. } => "sync:status",
            Event::ThemeChanged(_) => "theme:changed",
            Event::NewMail { .. } => "new_mail",
            Event::AuthDone { .. } => "auth:done",
            Event::BodyReady { .. } => "body:ready",
            Event::AccountsChanged => "accounts:changed",
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            Event::MessagesChanged { account, folder } => {
                json!({ "account": account, "folder": folder })
            }
            Event::SyncStatus {
                account,
                state,
                message,
            } => json!({
                "account": account,
                "state": state,
                "message": message,
                "at": chrono::Utc::now().timestamp(),
            }),
            Event::ThemeChanged(t) => json!(t),
            Event::NewMail { from, subject } => json!({ "from": from, "subject": subject }),
            Event::AuthDone { email, ok, error } => {
                json!({ "email": email, "ok": ok, "error": error })
            }
            Event::BodyReady { id } => json!({ "id": id }),
            Event::AccountsChanged => json!({}),
        }
    }
}
