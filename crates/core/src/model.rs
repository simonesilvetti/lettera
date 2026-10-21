//! Types shared with the frontend (mirrored by hand in `src/lib/types.ts`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Addr {
    pub name: String,
    pub addr: String,
}

impl Addr {
    pub fn display(&self) -> String {
        if self.name.is_empty() {
            self.addr.clone()
        } else {
            self.name.clone()
        }
    }

    pub fn full(&self) -> String {
        if self.name.is_empty() {
            self.addr.clone()
        } else {
            format!("{} <{}>", self.name, self.addr)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountInfo {
    pub email: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderInfo {
    pub id: i64,
    pub account: String,
    pub name: String,
    pub role: Option<String>,
    pub total: i64,
    pub unread: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    pub ids: Vec<i64>,
    pub subject: String,
    pub from: String,
    pub date: i64,
    pub count: i64,
    pub unread: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    /// The account's own address is among the `To:` recipients of some message.
    pub to_me: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DateGroup {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadPage {
    pub total: i64,
    pub unread: i64,
    pub offset: i64,
    pub groups: Vec<DateGroup>,
    pub items: Vec<ThreadSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub id: i64,
    pub folder_id: i64,
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: i64,
    pub unread: bool,
    pub flagged: bool,
    pub snippet: String,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadView {
    pub thread_id: String,
    pub account: String,
    pub subject: String,
    pub participants: Vec<String>,
    pub messages: Vec<MessageView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    Text,
    Quote,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BodyView {
    pub id: i64,
    /// `false` while the body is being downloaded; a `body:ready` event follows.
    pub ready: bool,
    pub blocks: Vec<Block>,
    /// Complete `srcdoc` for the sandboxed iframe (mode = "html" only).
    pub html: Option<String>,
    pub has_html: bool,
    /// Remote images left out of `html` until `enable_images`.
    pub blocked_images: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentInfo {
    pub id: i64,
    pub message_id: i64,
    pub filename: String,
    pub mime: String,
    pub size: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    #[default]
    New,
    Reply,
    ReplyAll,
    Forward,
}

impl DraftKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DraftKind::New => "new",
            DraftKind::Reply => "reply",
            DraftKind::ReplyAll => "reply_all",
            DraftKind::Forward => "forward",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Draft {
    #[serde(default)]
    pub id: Option<i64>,
    pub account: String,
    #[serde(default)]
    pub kind: DraftKind,
    /// Database id of the message being answered or forwarded.
    #[serde(default)]
    pub source: Option<i64>,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Quoted original, kept apart from `body` and always appended on send.
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub saved_at: Option<i64>,
    /// Message-ID of the copy in the server's Drafts folder, if one was stored.
    #[serde(default)]
    pub message_id: Option<String>,
    /// Threading headers of a reply, fixed when it is started: the message
    /// answered may leave the database later (archived, moved, deleted), and
    /// the draft must still be saved and sent in its thread.
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAccount {
    pub name: String,
    pub kind: String,
    pub email: String,
    #[serde(default)]
    pub imap_host: Option<String>,
    #[serde(default)]
    pub imap_port: Option<u16>,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub imap_tls: Option<String>,
    #[serde(default)]
    pub smtp_tls: Option<String>,
}
