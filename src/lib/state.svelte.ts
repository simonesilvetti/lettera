// UI state and the actions that change it. Domain logic lives in Rust:
// everything here either asks the backend or decides what to draw.

import { api, on } from './api';
import type {
  AccountInfo,
  AppInfo,
  AttachmentInfo,
  BodyView,
  DateGroup,
  Draft,
  DraftKind,
  FolderInfo,
  SyncStatus,
  ThemePayload,
  ThreadSummary,
  ThreadView,
} from './types';

export type Screen = 'loading' | 'setup' | 'inbox' | 'compose';
export type Focus = 'list' | 'thread' | 'search';

export interface PaletteItem {
  label: string;
  hint?: string;
  alias?: string;
  run: () => unknown;
}

export interface PaletteState {
  title: string;
  query: string;
  index: number;
  /** null = the command list */
  items: PaletteItem[] | null;
  info: boolean;
}

const PAGE = 100;

class AppState {
  screen = $state<Screen>('loading');
  accounts = $state<AccountInfo[]>([]);
  account = $state('');
  folders = $state<FolderInfo[]>([]);
  folderId = $state(0);

  query = $state('');
  total = $state(0);
  unread = $state(0);
  groups = $state<DateGroup[]>([]);
  items = $state<(ThreadSummary | undefined)[]>([]);
  selected = $state(0);
  focus = $state<Focus>('list');
  loaded = $state(false);

  thread = $state<ThreadView | null>(null);
  threadKey = $state('');
  cursor = $state(0);
  expanded = $state<Record<number, boolean>>({});
  bodies = $state<Record<number, BodyView>>({});
  htmlMode = $state<Record<number, boolean>>({});
  attachments = $state<Record<number, AttachmentInfo[]>>({});
  quotes = $state<Record<string, boolean>>({});

  palette = $state<PaletteState | null>(null);
  draft = $state<Draft | null>(null);
  draftDirty = $state(false);
  draftSavedAt = $state<number | null>(null);
  draftBcc = $state(false);
  draftQuote = $state(false);
  sending = $state(false);
  confirmDiscard = $state(false);

  status = $state<Record<string, SyncStatus>>({});
  flash = $state<{ text: string; error: boolean } | null>(null);
  info = $state<AppInfo | null>(null);
  setupOpen = $state(false);
}

export const app = new AppState();

let generation = 0;
const pagesLoading = new Set<number>();
let flashTimer: ReturnType<typeof setTimeout> | undefined;

export function flash(text: string, error = false) {
  app.flash = { text, error };
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => (app.flash = null), error ? 8000 : 3000);
}

function fail(e: unknown) {
  flash(String(e), true);
}

// ------------------------------------------------------------------ helpers

const ROLE_LABELS: Record<string, string> = {
  inbox: 'inbox',
  sent: 'sent',
  drafts: 'drafts',
  trash: 'trash',
  archive: 'archive',
  all: 'all mail',
  junk: 'spam',
};

export function folderLabel(f: FolderInfo | undefined): string {
  if (!f) return '';
  if (f.role && ROLE_LABELS[f.role]) return ROLE_LABELS[f.role];
  const parts = f.name.split('/');
  return parts[parts.length - 1].toLowerCase();
}

const DAYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
const MONTHS = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'];

function pad(n: number) {
  return String(n).padStart(2, '0');
}

/** "14:02" today, "thu 15" within the week, "14 oct" otherwise (plus year if not this one). */
export function shortDate(ts: number): string {
  const d = new Date(ts * 1000);
  const now = new Date();
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  if (d.getTime() >= midnight) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (d.getTime() >= midnight - 6 * 86400_000) return `${DAYS[d.getDay()]} ${d.getDate()}`;
  const base = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === now.getFullYear() ? base : `${base} ${d.getFullYear()}`;
}

export function longDate(ts: number): string {
  const d = new Date(ts * 1000);
  return `${DAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function clock(ts: number): string {
  const d = new Date(ts * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function currentFolder(): FolderInfo | undefined {
  return app.folders.find((f) => f.id === app.folderId);
}

export function currentSummary(): ThreadSummary | undefined {
  return app.items[app.selected];
}

export function isGoogle(): boolean {
  return app.accounts.find((a) => a.email === app.account)?.kind === 'google';
}

// ------------------------------------------------------------- loading data

export async function loadAccounts() {
  app.accounts = await api.listAccounts();
  if (app.accounts.length === 0) {
    app.screen = 'setup';
    app.account = '';
    return;
  }
  if (!app.accounts.some((a) => a.email === app.account)) {
    await selectAccount(app.accounts[0].email);
  }
  if (app.screen === 'loading' || (app.screen === 'setup' && !app.setupOpen)) app.screen = 'inbox';
}

export async function selectAccount(email: string) {
  app.account = email;
  app.folderId = 0;
  closeThread();
  await loadFolders();
}

export async function loadFolders() {
  if (!app.account) return;
  app.folders = await api.listFolders(app.account);
  if (!app.folders.some((f) => f.id === app.folderId)) {
    const inbox = app.folders.find((f) => f.role === 'inbox') ?? app.folders[0];
    if (inbox) await selectFolder(inbox.id);
    else resetList();
  }
}

function resetList() {
  generation++;
  pagesLoading.clear();
  app.items = [];
  app.total = 0;
  app.unread = 0;
  app.groups = [];
  app.selected = 0;
  app.loaded = false;
}

export async function selectFolder(id: number) {
  app.folderId = id;
  closeThread();
  resetList();
  await loadPage(0);
}

async function loadPage(page: number, force = false) {
  if (!app.folderId || (pagesLoading.has(page) && !force)) return;
  const gen = generation;
  pagesLoading.add(page);
  try {
    const res = await api.listThreads(app.folderId, app.query, page * PAGE, PAGE);
    if (gen !== generation) return;
    app.total = res.total;
    app.unread = res.unread;
    app.groups = res.groups;
    const items = app.items.slice(0, res.total);
    items.length = res.total;
    res.items.forEach((t, i) => (items[page * PAGE + i] = t));
    app.items = items;
    app.loaded = true;
    if (app.selected >= res.total) app.selected = Math.max(0, res.total - 1);
  } catch (e) {
    fail(e);
  } finally {
    if (gen === generation) pagesLoading.delete(page);
  }
}

/** Makes sure thread rows [start, end) are loaded. */
export function ensureLoaded(start: number, end: number) {
  for (let p = Math.floor(start / PAGE); p <= Math.floor(Math.max(start, end - 1) / PAGE); p++) {
    const first = p * PAGE;
    if (first >= app.total && app.loaded) break;
    if (app.items[first] === undefined || app.items[Math.min(first + PAGE, app.total) - 1] === undefined) {
      void loadPage(p);
    }
  }
}

/** Reloads the pages we already have (after a change on the backend). */
export async function refreshList() {
  const pages = new Set<number>([0]);
  app.items.forEach((t, i) => t && pages.add(Math.floor(i / PAGE)));
  await Promise.all([...pages].map((p) => loadPage(p, true)));
}

let searchTimer: ReturnType<typeof setTimeout> | undefined;
export function setQuery(q: string) {
  app.query = q;
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    resetList();
    void loadPage(0);
  }, 120);
}

// ------------------------------------------------------------------ threads

export function closeThread() {
  app.thread = null;
  app.threadKey = '';
  app.cursor = 0;
  if (app.focus === 'thread') app.focus = 'list';
}

/** Moves focus to the right pane: the selected thread, opened if it is not already. */
export async function focusThread() {
  const summary = app.items[app.selected];
  if (!summary) return;
  if (app.thread && app.threadKey === summary.thread_id) app.focus = 'thread';
  else await openThread();
}

export async function openThread(index = app.selected) {
  const summary = app.items[index];
  if (!summary) return;
  app.selected = index;
  try {
    const view = await api.getThread(summary.thread_id);
    app.thread = view;
    app.threadKey = summary.thread_id;
    app.focus = 'thread';
    const last = view.messages.length - 1;
    app.cursor = last;
    const expanded: Record<number, boolean> = {};
    view.messages.forEach((m, i) => {
      if (m.unread || i === last) expanded[m.id] = true;
    });
    app.expanded = expanded;
    for (const id of Object.keys(expanded)) void loadBody(Number(id));
    const unread = view.messages.filter((m) => m.unread).map((m) => m.id);
    if (unread.length) await api.setFlags(unread, ['\\Seen'], []);
  } catch (e) {
    fail(e);
  }
}

async function reloadThread() {
  if (!app.threadKey) return;
  try {
    const view = await api.getThread(app.threadKey);
    app.thread = view;
    app.cursor = Math.min(app.cursor, view.messages.length - 1);
  } catch {
    closeThread();
  }
}

export async function loadBody(id: number) {
  try {
    const body = await api.getBody(id, app.htmlMode[id] ? 'html' : 'text');
    app.bodies[id] = body;
    if (!body.ready) return;
    // Always ask: before the body was downloaded `has_attachments` was only a
    // guess from the headers (multipart/mixed), and it may have been wrong.
    const files = await api.listAttachments(id);
    app.attachments[id] = files;
    const has = files.length > 0;
    const msg = app.thread?.messages.find((m) => m.id === id);
    if (msg) msg.has_attachments = has;
    const row = app.items.find((t) => t?.ids.includes(id));
    if (row && has) row.has_attachments = true;
  } catch (e) {
    fail(e);
  }
}

export function cursorMessage() {
  return app.thread?.messages[app.cursor];
}

export function toggleExpanded(id: number) {
  app.expanded[id] = !app.expanded[id];
  if (app.expanded[id] && !app.bodies[id]?.ready) void loadBody(id);
}

export async function toggleHtml(id = cursorMessage()?.id) {
  if (id === undefined) return;
  app.htmlMode[id] = !app.htmlMode[id];
  app.expanded[id] = true;
  await loadBody(id);
}

export async function enableImages(id = cursorMessage()?.id) {
  if (id === undefined) return;
  await api.enableImages(id);
  app.htmlMode[id] = true;
  await loadBody(id);
  flash('images enabled for this message');
}

export function moveSelection(delta: number) {
  if (!app.total) return;
  app.selected = Math.max(0, Math.min(app.total - 1, app.selected + delta));
}

export function moveCursor(delta: number) {
  if (!app.thread) return;
  app.cursor = Math.max(0, Math.min(app.thread.messages.length - 1, app.cursor + delta));
}

export async function nextThread(delta: number) {
  const target = app.selected + delta;
  if (target < 0 || target >= app.total) return;
  ensureLoaded(target, target + 1);
  await openThread(target);
}

// ------------------------------------------------------------------ actions

/** Ids the action applies to: the open or selected thread, in this folder. */
function targetIds(): number[] {
  return currentSummary()?.ids ?? [];
}

async function removeAndAdvance(fn: (ids: number[]) => Promise<void>, done: string) {
  const ids = targetIds();
  if (!ids.length) return;
  const wasOpen = app.focus === 'thread';
  try {
    await fn(ids);
    flash(done);
    await refreshList();
    if (wasOpen && app.items[app.selected]) await openThread(app.selected);
    else closeThread();
  } catch (e) {
    fail(e);
  }
}

export const archive = () => removeAndAdvance(api.archive, 'archived');
export const remove = () => removeAndAdvance(api.delete, 'deleted');

export async function moveTo(folder: FolderInfo) {
  await removeAndAdvance((ids) => api.moveTo(ids, folder.id), `moved to ${folderLabel(folder)}`);
}

export async function markRead(read: boolean) {
  const ids = targetIds();
  if (!ids.length) return;
  try {
    await (read ? api.setFlags(ids, ['\\Seen'], []) : api.setFlags(ids, [], ['\\Seen']));
    if (!read) closeThread();
    flash(read ? 'marked as read' : 'marked as unread');
  } catch (e) {
    fail(e);
  }
}

export async function toggleFlag() {
  const s = currentSummary();
  if (!s) return;
  try {
    await (s.flagged ? api.setFlags(s.ids, [], ['\\Flagged']) : api.setFlags(s.ids, ['\\Flagged'], []));
    flash(s.flagged ? 'flag removed' : 'flagged');
  } catch (e) {
    fail(e);
  }
}

export function goToRole(role: string) {
  const f =
    app.folders.find((x) => x.role === role) ??
    (role === 'archive' ? app.folders.find((x) => x.role === 'all') : undefined);
  if (!f) {
    flash(`no ${ROLE_LABELS[role] ?? role} folder`, true);
    return;
  }
  void selectFolder(f.id);
}

export async function syncNow() {
  await api.syncNow(app.account || null);
}

export async function saveAttachment(a: AttachmentInfo) {
  try {
    flash(`downloading ${a.filename}…`);
    const path = await api.saveAttachment(a.id);
    if (path) flash(`saved to ${path}`);
  } catch (e) {
    fail(e);
  }
}

// ------------------------------------------------------------------ compose

let autosave: ReturnType<typeof setInterval> | undefined;

export async function compose(kind: DraftKind) {
  if (!app.account) return;
  const source = kind === 'new' ? null : (cursorMessage() ?? app.thread?.messages.at(-1))?.id ?? null;
  if (kind !== 'new' && source === null) {
    flash('open a thread first', true);
    return;
  }
  try {
    app.draft = await api.prepareDraft(app.account, kind, source);
    app.draftDirty = false;
    app.draftSavedAt = app.draft.saved_at;
    app.draftBcc = !!app.draft.bcc;
    app.draftQuote = false;
    app.confirmDiscard = false;
    app.screen = 'compose';
    clearInterval(autosave);
    autosave = setInterval(() => void saveDraft(), 5000);
  } catch (e) {
    fail(e);
  }
}

export function touchDraft() {
  app.draftDirty = true;
  app.confirmDiscard = false;
}

export async function saveDraft() {
  if (!app.draft || !app.draftDirty) return;
  try {
    const id = await api.saveDraft($state.snapshot(app.draft) as Draft);
    if (app.draft) app.draft.id = id;
    app.draftDirty = false;
    app.draftSavedAt = Math.floor(Date.now() / 1000);
  } catch (e) {
    fail(e);
  }
}

function leaveCompose() {
  clearInterval(autosave);
  app.draft = null;
  app.screen = 'inbox';
}

export async function cancelCompose() {
  if (!app.draft) return;
  if (app.draftDirty && !app.confirmDiscard) {
    app.confirmDiscard = true;
    return;
  }
  if (app.confirmDiscard && app.draft.id !== null) await api.discardDraft(app.draft.id);
  leaveCompose();
  if (app.confirmDiscard) flash('draft discarded');
}

export async function send() {
  if (!app.draft || app.sending) return;
  app.sending = true;
  flash('sending…');
  try {
    await saveDraftNow();
    await api.send($state.snapshot(app.draft) as Draft);
    leaveCompose();
    flash('sent');
  } catch (e) {
    fail(e);
  } finally {
    app.sending = false;
  }
}

async function saveDraftNow() {
  app.draftDirty = true;
  await saveDraft();
}

export async function attach() {
  if (!app.draft) return;
  const files = await api.pickFiles();
  if (files.length) {
    app.draft.attachments = [...app.draft.attachments, ...files];
    touchDraft();
  }
}

export function removeAttachment(i: number) {
  if (!app.draft) return;
  app.draft.attachments = app.draft.attachments.filter((_, k) => k !== i);
  touchDraft();
}

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

// ------------------------------------------------------------------ palette

export function openPalette(items: PaletteItem[] | null = null, title = '') {
  app.palette = { title, query: '', index: 0, items, info: false };
}

export function closePalette() {
  app.palette = null;
}

export async function showInfo() {
  app.info ??= await api.appInfo();
  app.palette = { title: '', query: '', index: 0, items: [], info: true };
}

// ------------------------------------------------------------------ theme

const TOKENS = ['bg', 'fg', 'muted', 'border', 'surface', 'accent', 'red', 'green'] as const;

export function applyTheme(t: ThemePayload) {
  const root = document.documentElement.style;
  for (const k of TOKENS) {
    if (t.tokens) root.setProperty(`--${k}`, t.tokens[k]);
    else root.removeProperty(`--${k}`);
  }
  root.setProperty('--font', `"${t.font}", ui-monospace, monospace`);
  root.setProperty('--fs', `${t.font_size}px`);
}

// ------------------------------------------------------------------ startup

export async function start() {
  applyTheme(await api.getTheme());
  await on('theme:changed', applyTheme);
  await on('sync:status', (s) => {
    app.status[s.account] = s;
    if (s.state === 'error' && (s.account === app.account || !s.account)) flash(s.message, true);
    if (s.state === 'idle' && s.account === app.account) void loadFolders();
  });
  await on('messages:changed', (e) => {
    if (e.account !== app.account) return;
    if (!app.folderId) {
      void loadFolders();
      return;
    }
    if (e.folder === app.folderId) {
      void refreshList();
      if (app.thread) void reloadThread();
    }
  });
  await on('body:ready', ({ id }) => {
    if (app.bodies[id] || app.thread?.messages.some((m) => m.id === id)) void loadBody(id);
  });
  await on('accounts:changed', () => void loadAccounts());
  await loadAccounts();
}
