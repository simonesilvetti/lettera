// Static check of the command registry: within one scope a key may run only
// one command, otherwise the first match silently shadows the others.
// (commands.ts imports Svelte runes, so it is read as text, not imported.)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const src = readFileSync(new URL('./commands.ts', import.meta.url), 'utf8');

interface Binding {
  id: string;
  scope: string;
  keys: string[];
}

function bindings(): Binding[] {
  const parts = src.split(/(?=\bid: ')/).slice(1);
  return parts.flatMap((p) => {
    const id = /^id: '([^']+)'/.exec(p)?.[1];
    const scope = /\bscope: '([^']+)'/.exec(p)?.[1];
    const keys = /\bkeys: \[([^\]]*)\]/.exec(p)?.[1];
    if (!id || !scope || !keys) return [];
    return [{ id, scope, keys: [...keys.matchAll(/'([^']+)'/g)].map((m) => m[1]) }];
  });
}

test('the registry is parsed', () => {
  const b = bindings();
  assert.ok(b.length > 40, `only ${b.length} bindings found`);
  assert.ok(b.some((x) => x.id === 'focus-thread' && x.keys.includes('l')));
});

// Toggles: two commands on one key whose `when` conditions exclude each other.
const TOGGLES = new Set(['thread v: html reading']);

test('no key runs two commands in the same scope', () => {
  const seen = new Map<string, string>();
  for (const b of bindings()) {
    for (const k of b.keys) {
      const slot = `${b.scope} ${k}`;
      const other = seen.get(slot);
      if (other && !TOGGLES.has(`${slot}: ${other} ${b.id}`)) {
        assert.fail(`"${k}" in ${b.scope}: ${other} and ${b.id}`);
      }
      seen.set(slot, b.id);
    }
  }
});

test('vim keys and arrows are paired', () => {
  const b = bindings();
  const pairs: [string, string][] = [['h', 'arrowleft'], ['j', 'arrowdown'], ['k', 'arrowup'], ['l', 'arrowright']];
  for (const [vim, arrow] of pairs) {
    for (const x of b.filter((x) => x.keys.includes(vim))) {
      assert.ok(x.keys.includes(arrow), `${x.id} has ${vim} but not ${arrow}`);
    }
  }
});
