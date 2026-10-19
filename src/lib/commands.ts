// The command registry. Every action of the app is a command: the palette
// lists them and keys.ts binds their shortcuts, so the two cannot diverge.

import { api } from './api';
import {
  app,
  archive,
  attach,
  cancelCompose,
  closePalette,
  closeThread,
  compose,
  cursorMessage,
  enableImages,
  fileName,
  flash,
  folderLabel,
  goToRole,
  markRead,
  moveCursor,
  moveSelection,
  moveTo,
  nextThread,
  openPalette,
  openThread,
  remove,
  removeAttachment,
  saveAttachment,
  selectAccount,
  selectFolder,
  send,
  setQuery,
  showInfo,
  syncNow,
  toggleExpanded,
  toggleFlag,
  toggleHtml,
} from './state.svelte';

/** Where a command is available. */
export type Scope = 'list' | 'thread' | 'mail' | 'compose' | 'palette' | 'search' | 'global';

export interface Command {
  id: string;
  label: string;
  /** Shown in the palette and the status line. */
  shortcut?: string;
  /** Key combos as produced by keys.ts (e.g. "j", "g i", "ctrl+enter"). */
  keys?: string[];
  scope: Scope;
  /** Not listed in the palette (navigation keys and such). */
  hidden?: boolean;
  /** Extra words for the palette's fuzzy match. */
  alias?: string;
  when?: () => boolean;
  run: () => unknown;
}

const hasThread = () => !!app.items[app.selected];
const inThread = () => !!app.thread;

function folderItems(run: (id: number) => unknown, exclude?: number) {
  return app.folders
    .filter((f) => f.id !== exclude)
    .map((f) => ({
      label: folderLabel(f),
      hint: f.unread ? `${f.unread}` : '',
      alias: f.name,
      run: () => run(f.id),
    }));
}

export const commands: Command[] = [
  // ------------------------------------------------------------ list / thread
  { id: 'down', label: 'giù', keys: ['j', 'arrowdown'], scope: 'list', hidden: true, run: () => moveSelection(1) },
  { id: 'up', label: 'su', keys: ['k', 'arrowup'], scope: 'list', hidden: true, run: () => moveSelection(-1) },
  { id: 'top', label: 'inizio lista', keys: ['g g', 'home'], scope: 'list', hidden: true, run: () => (app.selected = 0) },
  {
    id: 'bottom',
    label: 'fine lista',
    keys: ['G', 'end'],
    scope: 'list',
    hidden: true,
    run: () => (app.selected = Math.max(0, app.total - 1)),
  },
  { id: 'open', label: 'apri', shortcut: '⏎', keys: ['enter'], scope: 'list', hidden: true, when: hasThread, run: () => openThread() },
  { id: 'msg-down', label: 'messaggio successivo', keys: ['j', 'arrowdown'], scope: 'thread', hidden: true, run: () => moveCursor(1) },
  { id: 'msg-up', label: 'messaggio precedente', keys: ['k', 'arrowup'], scope: 'thread', hidden: true, run: () => moveCursor(-1) },
  {
    id: 'expand',
    label: 'espandi messaggio',
    keys: ['enter'],
    scope: 'thread',
    hidden: true,
    run: () => {
      const m = cursorMessage();
      if (m) toggleExpanded(m.id);
    },
  },
  { id: 'close', label: 'chiudi thread', keys: ['escape'], scope: 'thread', hidden: true, run: closeThread },
  { id: 'next-thread', label: 'thread successivo', shortcut: 'n', keys: ['n'], scope: 'thread', run: () => nextThread(1) },
  { id: 'prev-thread', label: 'thread precedente', shortcut: 'p', keys: ['p'], scope: 'thread', run: () => nextThread(-1) },

  // ------------------------------------------------------------------ actions
  { id: 'archive', label: 'archivia', shortcut: 'e', keys: ['e'], scope: 'mail', when: hasThread, run: archive },
  { id: 'delete', label: 'elimina', shortcut: '#', keys: ['#', 'delete'], scope: 'mail', when: hasThread, run: remove },
  { id: 'read', label: 'segna come letto', scope: 'mail', when: hasThread, run: () => markRead(true) },
  { id: 'unread', label: 'segna come non letto', shortcut: 'u', keys: ['u'], scope: 'mail', when: hasThread, run: () => markRead(false) },
  { id: 'flag', label: 'segna/togli flag', shortcut: 's', keys: ['s'], scope: 'mail', when: hasThread, run: toggleFlag },
  {
    id: 'move',
    label: 'sposta in…',
    scope: 'mail',
    when: hasThread,
    run: () =>
      openPalette(
        folderItems((id) => {
          const f = app.folders.find((x) => x.id === id);
          if (f) void moveTo(f);
        }, app.folderId),
        'sposta in',
      ),
  },
  { id: 'reply', label: 'rispondi', shortcut: 'r', keys: ['r'], scope: 'mail', when: inThread, run: () => compose('reply') },
  { id: 'reply-all', label: 'rispondi a tutti', shortcut: 'a', keys: ['a'], scope: 'mail', when: inThread, run: () => compose('reply_all') },
  { id: 'forward', label: 'inoltra', shortcut: 'f', keys: ['f'], scope: 'mail', when: inThread, run: () => compose('forward') },
  { id: 'compose', label: 'scrivi', shortcut: 'c', keys: ['c'], scope: 'mail', run: () => compose('new') },
  {
    id: 'html',
    label: 'originale html',
    shortcut: 'v',
    keys: ['v'],
    scope: 'thread',
    when: () => !!cursorMessage() && !app.htmlMode[cursorMessage()!.id],
    run: () => toggleHtml(),
  },
  {
    id: 'reading',
    label: 'modalità lettura',
    shortcut: 'v',
    keys: ['v'],
    scope: 'thread',
    when: () => !!cursorMessage() && !!app.htmlMode[cursorMessage()!.id],
    run: () => toggleHtml(),
  },
  { id: 'images', label: 'abilita immagini', alias: 'immagini', scope: 'thread', when: () => !!cursorMessage(), run: () => enableImages() },
  {
    id: 'save-attachment',
    label: 'salva allegato…',
    shortcut: 'o',
    keys: ['o'],
    scope: 'thread',
    when: () => !!app.attachments[cursorMessage()?.id ?? -1]?.length,
    run: () => {
      const list = app.attachments[cursorMessage()?.id ?? -1] ?? [];
      if (list.length === 1) void saveAttachment(list[0]);
      else openPalette(list.map((a) => ({ label: a.filename, run: () => saveAttachment(a) })), 'salva allegato');
    },
  },

  // --------------------------------------------------------------- navigation
  { id: 'search', label: 'cerca', shortcut: '/', keys: ['/'], scope: 'mail', run: () => (app.focus = 'search') },
  { id: 'go-inbox', label: 'vai a inbox', shortcut: 'g i', keys: ['g i'], scope: 'mail', run: () => goToRole('inbox') },
  { id: 'go-archive', label: 'vai a archivio', shortcut: 'g a', keys: ['g a'], scope: 'mail', run: () => goToRole('archive') },
  { id: 'go-sent', label: 'vai a inviati', shortcut: 'g s', keys: ['g s'], scope: 'mail', run: () => goToRole('sent') },
  { id: 'go-trash', label: 'vai a cestino', shortcut: 'g t', keys: ['g t'], scope: 'mail', run: () => goToRole('trash') },
  { id: 'go-drafts', label: 'vai a bozze', shortcut: 'g d', keys: ['g d'], scope: 'mail', run: () => goToRole('drafts') },
  {
    id: 'go-folder',
    label: 'vai a cartella…',
    scope: 'mail',
    run: () => openPalette(folderItems((id) => selectFolder(id)), 'vai a cartella'),
  },
  {
    id: 'account',
    label: 'cambia account…',
    shortcut: '⌃1…9',
    scope: 'global',
    when: () => app.accounts.length > 0,
    run: () =>
      openPalette(
        app.accounts.map((a, i) => ({
          label: `${a.name} · ${a.email}`,
          hint: `⌃${i + 1}`,
          run: () => selectAccount(a.email),
        })),
        'cambia account',
      ),
  },
  ...Array.from({ length: 9 }, (_, i): Command => ({
    id: `account-${i + 1}`,
    label: `account ${i + 1}`,
    keys: [`ctrl+${i + 1}`],
    scope: 'global',
    hidden: true,
    when: () => !!app.accounts[i] && app.screen === 'inbox',
    run: () => selectAccount(app.accounts[i].email),
  })),
  { id: 'sync', label: 'sincronizza ora', shortcut: 'R', keys: ['R'], scope: 'mail', run: syncNow },
  {
    id: 'add-account',
    label: 'aggiungi account',
    scope: 'global',
    run: () => {
      app.setupOpen = true;
      app.screen = 'setup';
    },
  },
  {
    id: 'config',
    label: 'apri config',
    alias: 'config',
    scope: 'global',
    run: async () => {
      try {
        flash(`apro ${await api.openConfig()}`);
      } catch (e) {
        flash(String(e), true);
      }
    },
  },
  { id: 'info', label: 'info', scope: 'global', run: showInfo },
  { id: 'palette', label: 'comandi', shortcut: ':', keys: [':'], scope: 'mail', hidden: true, run: () => openPalette() },

  // ------------------------------------------------------------------ search
  {
    id: 'search-done',
    label: 'conferma ricerca',
    keys: ['enter', 'arrowdown'],
    scope: 'search',
    hidden: true,
    run: () => (app.focus = 'list'),
  },
  {
    id: 'search-cancel',
    label: 'annulla ricerca',
    keys: ['escape'],
    scope: 'search',
    hidden: true,
    run: () => {
      app.focus = 'list';
      setQuery('');
    },
  },
  {
    id: 'clear-search',
    label: 'annulla ricerca',
    keys: ['escape'],
    scope: 'list',
    hidden: true,
    when: () => !!app.query,
    run: () => setQuery(''),
  },

  // ------------------------------------------------------------------ compose
  { id: 'send', label: 'invia', shortcut: '⌃⏎', keys: ['ctrl+enter'], scope: 'compose', run: send },
  { id: 'attach', label: 'allega', shortcut: '⌃a', keys: ['ctrl+a'], scope: 'compose', run: attach },
  {
    id: 'remove-attachment',
    label: 'rimuovi allegato…',
    alias: 'togli elimina allegato',
    shortcut: '⌃d',
    keys: ['ctrl+d'],
    scope: 'compose',
    when: () => !!app.draft?.attachments.length,
    run: () =>
      openPalette(
        (app.draft?.attachments ?? []).map((path, i) => ({
          label: fileName(path),
          hint: path,
          run: () => removeAttachment(i),
        })),
        'rimuovi allegato',
      ),
  },
  {
    id: 'bcc',
    label: 'ccn',
    shortcut: '⌃b',
    keys: ['ctrl+b'],
    scope: 'compose',
    run: () => (app.draftBcc = !app.draftBcc),
  },
  {
    id: 'quote',
    label: 'mostra citazione',
    shortcut: '⌃q',
    keys: ['ctrl+q'],
    scope: 'compose',
    when: () => !!app.draft?.quote,
    run: () => (app.draftQuote = !app.draftQuote),
  },
  { id: 'cancel', label: 'annulla', shortcut: 'esc', keys: ['escape'], scope: 'compose', run: cancelCompose },

  // ------------------------------------------------------------------ palette
  { id: 'palette-close', label: 'chiudi', keys: ['escape'], scope: 'palette', hidden: true, run: closePalette },
];

export function byId(id: string): Command | undefined {
  return commands.find((c) => c.id === id);
}

/** Runs a command from a click, if its context allows it right now. */
export function runCommand(id: string) {
  const c = byId(id);
  if (c && (c.when?.() ?? true)) void c.run();
}
