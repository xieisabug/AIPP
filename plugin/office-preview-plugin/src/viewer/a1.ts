// A1 helpers shared by selection reporting and edit jobs (zero-based rows/cols).

export function columnName(col: number): string {
  let n = col + 1;
  let name = "";
  while (n > 0) {
    const rem = (n - 1) % 26;
    name = String.fromCharCode(65 + rem) + name;
    n = Math.floor((n - 1) / 26);
  }
  return name;
}

export function cellA1(row: number, col: number): string {
  return `${columnName(col)}${row + 1}`;
}

export function rangeA1(top: number, left: number, bottom: number, right: number): string {
  const start = cellA1(top, left);
  const end = cellA1(bottom, right);
  return start === end ? start : `${start}:${end}`;
}

export interface ParsedRange {
  top: number;
  left: number;
  bottom: number;
  right: number;
}

function parseCell(ref: string): { row: number; col: number } | null {
  const m = /^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/.exec(ref.trim());
  if (!m) return null;
  let col = 0;
  for (const ch of m[1].toUpperCase()) col = col * 26 + (ch.charCodeAt(0) - 64);
  const row = Number(m[2]);
  if (row < 1 || col < 1) return null;
  return { row: row - 1, col: col - 1 };
}

/** Parses `B2` or `B2:D9` (either corner order); null for anything else. */
export function parseRangeA1(a1: string): ParsedRange | null {
  const parts = a1.split(":");
  if (parts.length < 1 || parts.length > 2) return null;
  const a = parseCell(parts[0]);
  const b = parts.length === 2 ? parseCell(parts[1]) : a;
  if (!a || !b) return null;
  return {
    top: Math.min(a.row, b.row),
    left: Math.min(a.col, b.col),
    bottom: Math.max(a.row, b.row),
    right: Math.max(a.col, b.col),
  };
}
