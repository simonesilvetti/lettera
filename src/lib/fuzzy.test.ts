import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fuzzy, highlight } from './fuzzy.ts';

test('subsequence matches with positions', () => {
  const m = fuzzy('arc', 'archive');
  assert.ok(m);
  assert.deepEqual(m.positions, [0, 1, 2]);
  assert.equal(fuzzy('xyz', 'archive'), null);
  assert.deepEqual(fuzzy('oc', 'open config')?.positions, [0, 5]);
});

test('empty query matches everything', () => {
  assert.deepEqual(fuzzy('', 'anything'), { score: 0, positions: [] });
  assert.deepEqual(fuzzy('   ', 'anything'), { score: 0, positions: [] });
});

test('case-insensitive and spaces ignored', () => {
  assert.ok(fuzzy('GO FOLD', 'go to folder…'));
});

test('consecutive and word-start matches rank higher', () => {
  const words = ['save attachment…', 'mark as read', 'search'];
  const ranked = words
    .map((w) => ({ w, m: fuzzy('se', w) }))
    .filter((x) => x.m)
    .sort((a, b) => b.m!.score - a.m!.score);
  assert.equal(ranked[0].w, 'search');
  const a = fuzzy('arch', 'archive')!.score;
  const b = fuzzy('arch', 'go to archive')!.score;
  assert.ok(a > b);
});

test('highlight splits into runs', () => {
  assert.deepEqual(highlight('archive', [0, 1]), [
    ['ar', true],
    ['chive', false],
  ]);
  assert.deepEqual(highlight('', []), []);
});
