// Fixed-height list virtualization: which rows to render for a scroll position.

export interface Window {
  start: number;
  end: number;
  top: number;
  height: number;
}

export function visibleWindow(
  scrollTop: number,
  viewport: number,
  rowHeight: number,
  count: number,
  overscan = 8,
): Window {
  const first = Math.floor(scrollTop / rowHeight);
  const visible = Math.ceil(viewport / rowHeight) + 1;
  const start = Math.max(0, first - overscan);
  const end = Math.min(count, first + visible + overscan);
  return { start, end, top: start * rowHeight, height: count * rowHeight };
}

/** Scroll offset that keeps row `index` visible, or null when it already is. */
export function scrollInto(
  index: number,
  scrollTop: number,
  viewport: number,
  rowHeight: number,
): number | null {
  const top = index * rowHeight;
  if (top < scrollTop) return top;
  if (top + rowHeight > scrollTop + viewport) return top + rowHeight - viewport;
  return null;
}

export type Row =
  | { kind: 'header'; label: string }
  | { kind: 'thread'; index: number };

/** Date-group headers interleaved with thread rows: row index -> what to draw. */
export function rowAt(groups: { label: string; count: number }[], row: number): Row | null {
  let base = 0;
  let threads = 0;
  for (const g of groups) {
    if (g.count === 0) continue;
    if (row === base) return { kind: 'header', label: g.label };
    if (row <= base + g.count) return { kind: 'thread', index: threads + (row - base - 1) };
    base += g.count + 1;
    threads += g.count;
  }
  return null;
}

/** Row index for thread `index` (inverse of rowAt). */
export function rowOf(groups: { label: string; count: number }[], index: number): number {
  let row = 0;
  let threads = 0;
  for (const g of groups) {
    if (g.count === 0) continue;
    if (index < threads + g.count) return row + 1 + (index - threads);
    row += g.count + 1;
    threads += g.count;
  }
  return row;
}

export function rowCount(groups: { label: string; count: number }[]): number {
  return groups.reduce((n, g) => (g.count ? n + g.count + 1 : n), 0);
}
