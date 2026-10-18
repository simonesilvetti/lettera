// Subsequence fuzzy match with a small score: consecutive letters and
// word starts count more. Returns the matched positions for highlighting.

export interface Match {
  score: number;
  positions: number[];
}

export function fuzzy(query: string, text: string): Match | null {
  const q = query.toLowerCase().replace(/\s+/g, '');
  if (!q) return { score: 0, positions: [] };
  const t = text.toLowerCase();
  const positions: number[] = [];
  let score = 0;
  let from = 0;
  for (const ch of q) {
    const i = t.indexOf(ch, from);
    if (i < 0) return null;
    const prev = positions[positions.length - 1];
    if (prev !== undefined && i === prev + 1) score += 5;
    if (i === 0 || /[\s\-_/.]/.test(t[i - 1])) score += 3;
    score -= Math.min(i - from, 3);
    positions.push(i);
    from = i + 1;
  }
  return { score: score - t.length * 0.01, positions };
}

/** Splits `text` into [chunk, matched] pairs for rendering. */
export function highlight(text: string, positions: number[]): [string, boolean][] {
  const set = new Set(positions);
  const out: [string, boolean][] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = set.has(i);
    const last = out[out.length - 1];
    if (last && last[1] === hit) last[0] += text[i];
    else out.push([text[i], hit]);
  }
  return out;
}
