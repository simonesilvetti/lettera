import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rowAt, rowCount, rowOf, scrollInto, visibleWindow } from './virtual.ts';

const groups = [
  { label: 'oggi', count: 2 },
  { label: 'ieri', count: 0 },
  { label: 'prima', count: 3 },
];

test('rows interleave group headers and threads', () => {
  assert.equal(rowCount(groups), 7);
  assert.deepEqual(rowAt(groups, 0), { kind: 'header', label: 'oggi' });
  assert.deepEqual(rowAt(groups, 2), { kind: 'thread', index: 1 });
  assert.deepEqual(rowAt(groups, 3), { kind: 'header', label: 'prima' });
  assert.deepEqual(rowAt(groups, 6), { kind: 'thread', index: 4 });
  assert.equal(rowAt(groups, 7), null);
});

test('rowOf is the inverse of rowAt for threads', () => {
  for (let i = 0; i < 5; i++) {
    const row = rowAt(groups, rowOf(groups, i));
    assert.deepEqual(row, { kind: 'thread', index: i });
  }
});

test('visible window with overscan', () => {
  const w = visibleWindow(380, 760, 38, 1000, 5);
  assert.equal(w.start, 5);
  assert.equal(w.end, 36);
  assert.equal(w.top, 5 * 38);
  assert.equal(w.height, 38000);
  const end = visibleWindow(38000, 760, 38, 1000);
  assert.equal(end.end, 1000);
});

test('scrollInto only moves when needed', () => {
  assert.equal(scrollInto(5, 0, 380, 38), null);
  assert.equal(scrollInto(20, 0, 380, 38), 21 * 38 - 380);
  assert.equal(scrollInto(2, 200, 380, 38), 76);
});
