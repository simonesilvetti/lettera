import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fuzzy, highlight } from './fuzzy.ts';

test('subsequence matches with positions', () => {
  const m = fuzzy('arc', 'archivia');
  assert.ok(m);
  assert.deepEqual(m.positions, [0, 1, 2]);
  assert.equal(fuzzy('xyz', 'archivia'), null);
  assert.deepEqual(fuzzy('ac', 'apri config')?.positions, [0, 5]);
});

test('empty query matches everything', () => {
  assert.deepEqual(fuzzy('', 'qualsiasi'), { score: 0, positions: [] });
  assert.deepEqual(fuzzy('   ', 'qualsiasi'), { score: 0, positions: [] });
});

test('case-insensitive and spaces ignored', () => {
  assert.ok(fuzzy('VAI CART', 'vai a cartella…'));
});

test('consecutive and word-start matches rank higher', () => {
  const words = ['sposta in…', 'segna come letto', 'sincronizza ora'];
  const ranked = words
    .map((w) => ({ w, m: fuzzy('si', w) }))
    .filter((x) => x.m)
    .sort((a, b) => b.m!.score - a.m!.score);
  assert.equal(ranked[0].w, 'sincronizza ora');
  const a = fuzzy('arch', 'archivia')!.score;
  const b = fuzzy('arch', 'vai a archivio')!.score;
  assert.ok(a > b);
});

test('highlight splits into runs', () => {
  assert.deepEqual(highlight('archivia', [0, 1]), [
    ['ar', true],
    ['chivia', false],
  ]);
  assert.deepEqual(highlight('', []), []);
});
