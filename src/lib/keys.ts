// Keyboard: turns key events into combos and runs the matching command.

import { commands, type Command, type Scope } from './commands';
import { app } from './state.svelte';

const PREFIXES = new Set(['g']);
let pending: string | null = null;
let pendingTimer: ReturnType<typeof setTimeout> | undefined;

/** "j", "G", "#", "enter", "ctrl+enter", "ctrl+1", "shift+tab"… */
export function keyName(e: KeyboardEvent): string {
  let key = e.key === ' ' ? 'space' : e.key;
  if (key.length > 1) key = key.toLowerCase();
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push('ctrl');
  if (e.altKey) mods.push('alt');
  if (e.shiftKey && key.length > 1) mods.push('shift');
  if (mods.includes('ctrl') && key.length === 1) key = key.toLowerCase();
  if (mods.includes('ctrl') && /^digit\d$/i.test(e.code)) key = e.code.slice(5);
  return [...mods, key].join('+');
}

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

/** Scopes active right now, most specific first. */
export function activeScopes(): Scope[] {
  if (app.palette) return ['palette'];
  switch (app.screen) {
    case 'compose':
      return ['compose', 'global'];
    case 'setup':
      return ['global'];
    case 'inbox':
      if (app.focus === 'search') return ['search', 'global'];
      return app.focus === 'thread' ? ['thread', 'mail', 'global'] : ['list', 'mail', 'global'];
    default:
      return [];
  }
}

export function available(c: Command, scopes = activeScopes()): boolean {
  return scopes.includes(c.scope) && (c.when?.() ?? true);
}

export function handleKey(e: KeyboardEvent) {
  const key = keyName(e);
  if (['control', 'shift', 'alt', 'meta'].includes(key)) return;
  const inField = typing(e.target);
  if (inField && key !== 'escape' && !key.startsWith('ctrl+')) {
    // the search field still confirms with enter / arrow down
    if (!(app.focus === 'search' && (key === 'enter' || key === 'arrowdown'))) return;
  }
  let combo = key;
  if (pending) {
    combo = `${pending} ${key}`;
    pending = null;
    clearTimeout(pendingTimer);
  } else if (PREFIXES.has(key) && !inField && !app.palette && app.screen === 'inbox') {
    pending = key;
    pendingTimer = setTimeout(() => (pending = null), 1200);
    e.preventDefault();
    return;
  }
  const scopes = activeScopes();
  for (const scope of scopes) {
    const cmd = commands.find((c) => c.scope === scope && c.keys?.includes(combo) && (c.when?.() ?? true));
    if (cmd) {
      e.preventDefault();
      void cmd.run();
      return;
    }
  }
}
