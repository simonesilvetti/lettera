// Mirror of the Rust types in crates/core/src/model.rs (kept by hand).

export interface Addr {
  name: string;
  addr: string;
}

export interface AccountInfo {
  email: string;
  name: string;
  kind: 'imap' | 'google';
}

export interface FolderInfo {
  id: number;
  account: string;
  name: string;
  role: string | null;
  total: number;
  unread: number;
}

export interface ThreadSummary {
  thread_id: string;
  ids: number[];
  subject: string;
  from: string;
  date: number;
  count: number;
  unread: boolean;
  flagged: boolean;
  has_attachments: boolean;
  to_me: boolean;
}

export interface DateGroup {
  label: string;
  count: number;
}

export interface ThreadPage {
  total: number;
  unread: number;
  offset: number;
  groups: DateGroup[];
  items: ThreadSummary[];
}

export interface MessageView {
  id: number;
  folder_id: number;
  from: Addr;
  to: Addr[];
  cc: Addr[];
  subject: string;
  date: number;
  unread: boolean;
  flagged: boolean;
  snippet: string;
  has_attachments: boolean;
}

export interface ThreadView {
  thread_id: string;
  account: string;
  subject: string;
  participants: string[];
  messages: MessageView[];
}

export interface Block {
  kind: 'text' | 'quote';
  text: string;
  lines: number;
}

export interface BodyView {
  id: number;
  ready: boolean;
  blocks: Block[];
  html: string | null;
  has_html: boolean;
}

export interface AttachmentInfo {
  id: number;
  message_id: number;
  filename: string;
  mime: string;
  size: number;
}

export type DraftKind = 'new' | 'reply' | 'reply_all' | 'forward';

export interface Draft {
  id: number | null;
  account: string;
  kind: DraftKind;
  source: number | null;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  quote: string;
  attachments: string[];
  saved_at: number | null;
  /** Message-ID of the copy in the server's Drafts folder. */
  message_id: string | null;
  in_reply_to: string | null;
  references: string[];
}

export interface NewAccount {
  name: string;
  kind: 'imap' | 'google';
  email: string;
  imap_host?: string;
  imap_port?: number;
  smtp_host?: string;
  smtp_port?: number;
  username?: string;
  password?: string;
  client_id?: string;
  client_secret?: string;
  imap_tls?: 'implicit' | 'starttls';
  smtp_tls?: 'implicit' | 'starttls';
}

export interface Tokens {
  bg: string;
  fg: string;
  muted: string;
  border: string;
  surface: string;
  accent: string;
  red: string;
  green: string;
}

export interface ThemePayload {
  tokens: Tokens | null;
  font: string;
  font_size: number;
}

export interface SyncStatus {
  account: string;
  state: 'idle' | 'syncing' | 'error';
  message: string;
  at: number;
}

export interface AppInfo {
  name: string;
  version: string;
  license: string;
  author: string;
  homepage: string;
  config: string;
}
