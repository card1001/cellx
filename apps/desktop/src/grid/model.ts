import { getActiveLocale } from "../i18n";
// Pure helpers shared by the grid and the app. Kept free of React so they are easy to test.

export const MAX_ROWS = 1_048_576;
export const MAX_COLS = 16_384;

export interface Pos {
  r: number;
  c: number;
}

export interface Range {
  r0: number;
  c0: number;
  r1: number;
  c1: number;
}

/** 1 → A, 27 → AA, 16384 → XFD */
export function colName(c: number): string {
  let s = "";
  let n = c;
  while (n > 0) {
    const m = (n - 1) % 26;
    s = String.fromCharCode(65 + m) + s;
    n = Math.floor((n - 1) / 26);
  }
  return s;
}

export function addr(p: Pos): string {
  return `${colName(p.c)}${p.r}`;
}

/** "B12" → {r: 12, c: 2}; null when invalid. */
export function parseAddr(text: string): Pos | null {
  const m = /^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/.exec(text.trim());
  if (!m) return null;
  let c = 0;
  for (const ch of m[1].toUpperCase()) c = c * 26 + (ch.charCodeAt(0) - 64);
  const r = Number(m[2]);
  if (r < 1 || r > MAX_ROWS || c > MAX_COLS) return null;
  return { r, c };
}

export function normalize(a: Pos, b: Pos): Range {
  return {
    r0: Math.min(a.r, b.r),
    c0: Math.min(a.c, b.c),
    r1: Math.max(a.r, b.r),
    c1: Math.max(a.c, b.c),
  };
}

export function rangeLabel(a: Pos, b: Pos): string {
  if (a.r === b.r && a.c === b.c) return addr(a);
  const r = normalize(a, b);
  return `${addr({ r: r.r0, c: r.c0 })}:${addr({ r: r.r1, c: r.c1 })}`;
}

/** Parses clipboard text the way Excel writes it: tabs between cells, newlines between rows. */
export function parseTsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i++;
        } else {
          quoted = false;
        }
      } else {
        cell += ch;
      }
    } else if (ch === '"' && cell === "") {
      quoted = true;
    } else if (ch === "\t") {
      row.push(cell);
      cell = "";
    } else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && text[i + 1] === "\n") i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
    } else {
      cell += ch;
    }
  }
  if (cell !== "" || row.length > 0) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

export function toTsv(rows: string[][]): string {
  const esc = (s: string) => (/[\t\n\r"]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
  return rows.map((r) => r.map(esc).join("\t")).join("\n");
}

/**
 * Maps a scroll offset to a fractional row index. Browsers cap element sizes
 * (WebKit at about 2^24 px), so very tall grids use a compressed scroll area.
 */
export const MAX_SCROLL_PX = 8_000_000;

export function scrollHeightFor(rows: number, rowHeight: number): number {
  return Math.min(rows * rowHeight, MAX_SCROLL_PX);
}

export function firstRowAt(
  scrollTop: number,
  rows: number,
  rowHeight: number,
  viewport: number,
): number {
  const total = rows * rowHeight;
  const scrollable = scrollHeightFor(rows, rowHeight);
  if (total <= scrollable || scrollable <= viewport) return scrollTop / rowHeight;
  const ratio = (total - viewport) / (scrollable - viewport);
  return (scrollTop * ratio) / rowHeight;
}

export function scrollTopFor(
  rowFloat: number,
  rows: number,
  rowHeight: number,
  viewport: number,
): number {
  const total = rows * rowHeight;
  const scrollable = scrollHeightFor(rows, rowHeight);
  if (total <= scrollable || scrollable <= viewport) return rowFloat * rowHeight;
  const ratio = (total - viewport) / (scrollable - viewport);
  return (rowFloat * rowHeight) / ratio;
}

/** Whether a key press should start editing the focused cell (Excel: any printable key). */
export function startsEdit(e: { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean }): boolean {
  return e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey;
}

export function formatCount(n: number): string {
  return new Intl.NumberFormat(getActiveLocale()).format(n);
}
