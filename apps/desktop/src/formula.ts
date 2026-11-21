// Formula text helpers for the editor: reference spans (for colours and point mode),
// Excel's F4 cycle, and the function-name token under the caret (for autocomplete).
import { colName, type Pos } from "./grid/model";

export const MAX_ROWS = 1_048_576;
export const MAX_COLS = 16_384;

/** Excel-like colours for the references of the formula being edited. */
export const REF_COLORS = ["#1f6fd1", "#d13438", "#8e44ad", "#16a34a", "#c2410c", "#0891b2", "#b45309", "#db2777"];

export interface RefSpan {
  start: number;
  end: number;
  sheet: string | null;
  r0: number;
  c0: number;
  r1: number;
  c1: number;
  /** Index into REF_COLORS; the same reference text gets the same colour. */
  color: number;
}

interface Corner {
  col?: number;
  colAbs: boolean;
  row?: number;
  rowAbs: boolean;
}

function parseCorner(s: string): Corner | null {
  const m = /^(\$?)([A-Za-z]{0,3})(\$?)(\d{0,7})$/.exec(s);
  if (!m) return null;
  const [, ca, letters, ra, digits] = m;
  if (!letters && !digits) return null;
  if (ca && !letters) return null;
  if (ra && !digits) return null;
  let col: number | undefined;
  if (letters) {
    col = 0;
    for (const ch of letters.toUpperCase()) col = col * 26 + (ch.charCodeAt(0) - 64);
    if (col > MAX_COLS) return null;
  }
  let row: number | undefined;
  if (digits) {
    row = Number(digits);
    if (row < 1 || row > MAX_ROWS) return null;
  }
  return { col, colAbs: !!ca, row, rowAbs: !!ra };
}

function renderCorner(c: Corner): string {
  let s = "";
  if (c.col !== undefined) s += (c.colAbs ? "$" : "") + colName(c.col);
  if (c.row !== undefined) s += (c.rowAbs ? "$" : "") + String(c.row);
  return s;
}

const isNameChar = (ch: string) => /[\p{L}\p{N}_.]/u.test(ch);

interface Found {
  start: number;
  end: number;
  sheet: string | null;
  sheetText: string;
  a: Corner;
  b: Corner | null;
}

/** Finds A1 references outside strings and outside structured-reference brackets. */
function findRefs(f: string): Found[] {
  const out: Found[] = [];
  let i = 0;
  const n = f.length;
  while (i < n) {
    const ch = f[i];
    if (ch === '"') {
      i++;
      while (i < n) {
        if (f[i] === '"') {
          if (f[i + 1] === '"') {
            i += 2;
            continue;
          }
          break;
        }
        i++;
      }
      i++;
      continue;
    }
    if (ch === "[") {
      let depth = 0;
      while (i < n) {
        if (f[i] === "[") depth++;
        else if (f[i] === "]" && --depth === 0) break;
        i++;
      }
      i++;
      continue;
    }
    const prevOk = i === 0 || !(isNameChar(f[i - 1]) || f[i - 1] === "]");
    if (!prevOk || !/[\p{L}\p{N}$'_]/u.test(ch)) {
      i++;
      continue;
    }
    const start = i;
    let j = i;
    let sheet: string | null = null;
    if (ch === "'") {
      let k = i + 1;
      let name = "";
      let closed = false;
      while (k < n) {
        if (f[k] === "'") {
          if (f[k + 1] === "'") {
            name += "'";
            k += 2;
            continue;
          }
          closed = true;
          break;
        }
        name += f[k++];
      }
      if (closed && f[k + 1] === "!") {
        sheet = name;
        j = k + 2;
      } else {
        i++;
        continue;
      }
    } else {
      let k = i;
      while (k < n && (isNameChar(f[k]) || f[k] === "$")) k++;
      if (f[k] === "!") {
        sheet = f.slice(i, k);
        j = k + 1;
      }
    }
    let k = j;
    while (k < n && /[A-Za-z0-9$]/.test(f[k])) k++;
    const a = parseCorner(f.slice(j, k));
    if (!a) {
      let w = i + 1;
      while (w < n && isNameChar(f[w])) w++;
      i = w;
      continue;
    }
    if (f[k] === "(") {
      i = k;
      continue;
    }
    let end = k;
    let b: Corner | null = null;
    if (f[k] === ":") {
      let m = k + 1;
      while (m < n && /[A-Za-z0-9$]/.test(f[m])) m++;
      const c = parseCorner(f.slice(k + 1, m));
      if (c) {
        b = c;
        end = m;
      }
    }
    const whole = a.row === undefined || a.col === undefined;
    if (whole && !b) {
      i = Math.max(k, i + 1);
      continue;
    }
    if (end < n && (isNameChar(f[end]) || f[end] === "[")) {
      i = end;
      continue;
    }
    out.push({ start, end, sheet, sheetText: f.slice(start, j), a, b });
    i = end;
  }
  return out;
}

export function refSpans(formula: string): RefSpan[] {
  if (!formula.startsWith("=")) return [];
  const colors = new Map<string, number>();
  return findRefs(formula).map((r) => {
    const b = r.b ?? r.a;
    const c0 = r.a.col ?? 1;
    const c1 = b.col ?? MAX_COLS;
    const r0 = r.a.row ?? 1;
    const r1 = b.row ?? MAX_ROWS;
    const key = formula.slice(r.start, r.end).replace(/\$/g, "").toUpperCase();
    if (!colors.has(key)) colors.set(key, colors.size % REF_COLORS.length);
    return {
      start: r.start,
      end: r.end,
      sheet: r.sheet,
      r0: Math.min(r0, r1),
      c0: Math.min(c0, c1),
      r1: Math.max(r0, r1),
      c1: Math.max(c0, c1),
      color: colors.get(key)!,
    };
  });
}

/** Excel's F4: A1 → $A$1 → A$1 → $A1 → A1 for the reference at or just before the caret. */
export function cycleAbsolute(formula: string, caret: number): { text: string; caret: number } | null {
  const r = findRefs(formula).find((x) => caret >= x.start && caret <= x.end);
  if (!r) return null;
  const next = (c: Corner): Corner => {
    const state = `${c.colAbs || c.col === undefined ? 1 : 0}${c.rowAbs || c.row === undefined ? 1 : 0}`;
    const [ca, ra] = ({ "00": [true, true], "11": [false, true], "01": [true, false], "10": [false, false] } as const)[
      state as "00" | "11" | "01" | "10"
    ];
    return { ...c, colAbs: c.col !== undefined && ca, rowAbs: c.row !== undefined && ra };
  };
  const text = r.sheetText + renderCorner(next(r.a)) + (r.b ? ":" + renderCorner(next(r.b)) : "");
  const before = formula.slice(0, r.start) + text;
  return { text: before + formula.slice(r.end), caret: before.length };
}

/**
 * Whether typing a reference at the caret makes sense (Excel's "point mode"):
 * right after `=`, an operator, a comma or an opening parenthesis.
 */
export function canPointAt(text: string, caret: number): boolean {
  if (!text.startsWith("=")) return false;
  const before = text.slice(0, caret).replace(/\s+$/, "");
  if (before.length === 0) return false;
  // Inside an open string literal: no.
  const quotes = (before.match(/"/g) ?? []).length;
  if (quotes % 2 === 1) return false;
  return /[=(,+\-*/^&<>:;]$/.test(before) || before === "=";
}

export function refText(r0: number, c0: number, r1: number, c1: number, sheet?: string | null): string {
  const a = `${colName(c0)}${r0}`;
  const b = `${colName(c1)}${r1}`;
  const prefix = sheet ? `${quoteSheet(sheet)}!` : "";
  return prefix + (a === b ? a : `${a}:${b}`);
}

export function quoteSheet(name: string): string {
  return /^[\p{L}_][\p{L}\p{N}_]*$/u.test(name) ? name : `'${name.replace(/'/g, "''")}'`;
}

/** A point-mode reference inserted into the formula: replaced as the user moves. */
export interface PointRef {
  start: number;
  end: number;
  anchor: Pos;
  focus: Pos;
}

/** Inserts (or replaces, when `point` is active) a reference at the caret. */
export function insertRef(
  text: string,
  caret: number,
  point: PointRef | null,
  anchor: Pos,
  focus: Pos,
  sheet: string | null,
): { text: string; caret: number; point: PointRef } {
  const ref = refText(
    Math.min(anchor.r, focus.r),
    Math.min(anchor.c, focus.c),
    Math.max(anchor.r, focus.r),
    Math.max(anchor.c, focus.c),
    sheet,
  );
  const start = point ? point.start : caret;
  const end = point ? point.end : caret;
  const out = text.slice(0, start) + ref + text.slice(end);
  const newEnd = start + ref.length;
  return { text: out, caret: newEnd, point: { start, end: newEnd, anchor, focus } };
}

/** The function name being typed at the caret, e.g. `SU` in `=A1+SU`. */
export function functionPrefixAt(text: string, caret: number): { start: number; prefix: string } | null {
  if (!text.startsWith("=")) return null;
  const before = text.slice(0, caret);
  const quotes = (before.match(/"/g) ?? []).length;
  if (quotes % 2 === 1) return null;
  const m = /([A-Za-z_][A-Za-z0-9_.]*)$/.exec(before);
  if (!m) return null;
  const start = caret - m[1].length;
  const prev = before[start - 1];
  if (prev !== undefined && !/[=(,+\-*/^&<>:;\s]/.test(prev)) return null;
  // A complete cell reference (A1) is not a function prefix.
  if (/^[A-Za-z]{1,3}\d+$/.test(m[1])) return null;
  return { start, prefix: m[1] };
}

export function completeFunctions(all: string[], prefix: string, limit = 10): string[] {
  const p = prefix.toUpperCase();
  return all.filter((f) => f.startsWith(p)).slice(0, limit);
}

/** Splits the formula into plain and coloured pieces for the editor overlay. */
export function colourPieces(text: string): { text: string; color?: string }[] {
  const spans = refSpans(text);
  const out: { text: string; color?: string }[] = [];
  let last = 0;
  for (const s of spans) {
    if (s.start > last) out.push({ text: text.slice(last, s.start) });
    out.push({ text: text.slice(s.start, s.end), color: REF_COLORS[s.color] });
    last = s.end;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}
