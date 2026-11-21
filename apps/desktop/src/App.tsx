import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type BookInfo,
  type CellDetail,
  type Direction,
  errorText,
  isTauri,
  type RecalcStats,
  type SelectionStats,
  type TableInfo,
} from "./api";
import { FindDialog, type FormatResult, FormatCellsDialog, PromptDialog } from "./components/Dialogs";
import { FormulaText } from "./components/FormulaText";
import { ContextMenu, type MenuItem } from "./components/Menu";
import { Ribbon, type RibbonActions } from "./components/Ribbon";
import { decreaseDecimals, increaseDecimals } from "./formats";
import { canPointAt, completeFunctions, cycleAbsolute, functionPrefixAt, insertRef, type PointRef, refSpans } from "./formula";
import { fontCss, Grid, type Hit, type RefBox, type Selection } from "./grid/Grid";
import { formatCount, normalize, parseAddr, type Pos, type Range, rangeLabel, toTsv } from "./grid/model";
import { DataSource, SheetSource } from "./grid/sources";

type Tab = { kind: "sheet"; index: number } | { kind: "data"; name: string };
const tabKey = (t: Tab) => (t.kind === "sheet" ? `s:${t.index}` : `d:${t.name}`);
const START: Selection = { anchor: { r: 1, c: 1 }, focus: { r: 1, c: 1 } };

interface Session {
  tab: Tab;
  pos: Pos;
  text: string;
  /** Text before editing, to skip writes that change nothing. */
  original: string;
  caret: number;
  /** enter: arrows leave the cell (or point); edit (F2): arrows move the caret. */
  mode: "enter" | "edit";
  origin: "cell" | "bar";
  point: PointRef | null;
}

type Dialog =
  | { kind: "prompt"; title: string; label: string; value: string; resolve(v: string | null): void }
  | { kind: "format" }
  | null;

const ARROWS: Record<string, Direction> = { ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
const DELTA: Record<Direction, [number, number]> = { Up: [-1, 0], Down: [1, 0], Left: [0, -1], Right: [0, 1] };
const FONT_SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

const measureCtx = typeof document !== "undefined" ? document.createElement("canvas").getContext("2d") : null;

/** The cell editor grows to the right with its text, as in Excel. */
function editorWidth(text: string, font: string, min: number): number {
  if (!measureCtx) return min;
  measureCtx.font = font;
  const longest = Math.max(...text.split("\n").map((l) => measureCtx.measureText(l).width));
  return Math.max(min, Math.ceil(longest + 14));
}

function formatNumber(n: number): string {
  // Excel's status bar shows up to 10 significant digits.
  return Number.isInteger(n) ? n.toLocaleString("ko-KR") : n.toLocaleString("ko-KR", { maximumSignificantDigits: 10 });
}

export default function App() {
  const [info, setInfo] = useState<BookInfo | null>(null);
  const [functions, setFunctions] = useState<string[]>([]);
  const [tab, setTabState] = useState<Tab>({ kind: "sheet", index: 0 });
  const [selection, setSelectionState] = useState<Selection>(START);
  const [session, setSessionState] = useState<Session | null>(null);
  const [detail, setDetail] = useState<CellDetail | null>(null);
  const [stats, setStats] = useState<RecalcStats | null>(null);
  const [selStats, setSelStats] = useState<SelectionStats | null>(null);
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [showSql, setShowSql] = useState(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findMsg, setFindMsg] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [copyMark, setCopyMark] = useState<{ tab: string; range: Range; cut: boolean } | null>(null);
  const [acIndex, setAcIndex] = useState(0);
  const [acDismissed, setAcDismissed] = useState(false);
  const [source, setSource] = useState<SheetSource | DataSource | null>(null);
  const [, force] = useState(0);

  const sources = useRef(new Map<string, SheetSource | DataSource>());
  const selections = useRef(new Map<string, Selection>());
  const cellInput = useRef<HTMLTextAreaElement>(null);
  const barInput = useRef<HTMLTextAreaElement>(null);
  const nameBox = useRef<HTMLInputElement>(null);
  const sessionRef = useRef<Session | null>(null);
  const selectionRef = useRef(selection);
  const tabRef = useRef(tab);
  const detailRef = useRef(detail);
  const sourceRef = useRef(source);
  const drag = useRef<null | "cell" | "row" | "col" | "point">(null);
  const pageRows = useRef(20);
  const lastCopy = useRef<{ partial: string; full: string | null }>({ partial: "", full: null });
  const tabStartCol = useRef<number | null>(null);
  detailRef.current = detail;
  sourceRef.current = source;

  const setSession = (s: Session | null) => {
    sessionRef.current = s;
    setSessionState(s);
    setAcDismissed(false);
  };
  const setSelection = (s: Selection) => {
    selectionRef.current = s;
    selections.current.set(tabKey(tabRef.current), s);
    setSelectionState(s);
  };

  const say = (text: string, error = false) => setMessage({ text, error });
  const fail = (e: unknown) => say(errorText(e), true);
  const focusGrid = () => requestAnimationFrame(() => cellInput.current?.focus({ preventScroll: true }));

  const ask = (title: string, label: string, value = "") =>
    new Promise<string | null>((resolve) => setDialog({ kind: "prompt", title, label, value, resolve }));

  const run = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(label);
    try {
      return await fn();
    } catch (e) {
      fail(e);
      return undefined;
    } finally {
      setBusy(null);
    }
  };

  // ---------------------------------------------------------------- sources
  const sheetIndex = tab.kind === "sheet" ? tab.index : -1;
  const sheetName = (i: number) => info?.sheets[i] ?? "";

  const sourceFor = useCallback(
    async (t: Tab): Promise<SheetSource | DataSource | null> => {
      const k = tabKey(t);
      const existing = sources.current.get(k);
      if (existing) return existing;
      const font = info?.font ?? (["맑은 고딕", 11] as [string, number]);
      if (t.kind === "sheet") {
        const s = new SheetSource(t.index, await api.layout(t.index), font);
        sources.current.set(k, s);
        return s;
      }
      const table = info?.tables.find((x) => x.name === t.name);
      if (!table) return null;
      const s = new DataSource(table, font);
      sources.current.set(k, s);
      return s;
    },
    [info],
  );

  useEffect(() => {
    let live = true;
    void sourceFor(tab).then((s) => live && setSource(s));
    return () => {
      live = false;
    };
  }, [tab, sourceFor]);

  useEffect(() => {
    if (!source) return;
    return source.subscribe(() => force((x) => x + 1));
  }, [source]);

  const invalidateSheets = async () => {
    await Promise.all([...sources.current.values()].map((s) => (s instanceof SheetSource ? s.invalidate() : null)));
  };

  // ---------------------------------------------------------------- book
  const refreshInfo = async () => setInfo(await api.bookInfo());

  useEffect(() => {
    if (!isTauri()) return;
    void refreshInfo().catch(fail);
    void api.functionNames().then(setFunctions).catch(() => {});
    focusGrid();
  }, []);

  /** Active cell details: formula bar text and ribbon state. */
  const loadDetail = useCallback(async () => {
    const f = selectionRef.current.focus;
    const t = tabRef.current;
    if (t.kind !== "sheet") {
      setDetail(null);
      return;
    }
    try {
      const d = await api.cellDetail(t.index, f.r, f.c);
      if (selectionRef.current.focus === f && tabRef.current === t) setDetail(d);
    } catch (e) {
      fail(e);
    }
  }, []);

  useEffect(() => {
    void loadDetail();
  }, [selection.focus, tab, loadDetail]);

  // Status bar: sum, average and count of the selection (sheets only).
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setSelStats(null);
      return;
    }
    const rg = normalize(selection.anchor, selection.focus);
    if (rg.r0 === rg.r1 && rg.c0 === rg.c1) {
      setSelStats(null);
      return;
    }
    const id = setTimeout(() => {
      api.selectionStats(tab.index, rg).then(setSelStats).catch(() => setSelStats(null));
    }, 120);
    return () => clearTimeout(id);
  }, [selection, tab, stats]);

  const afterChange = async (s?: RecalcStats) => {
    if (s) setStats(s);
    await invalidateSheets();
    void loadDetail();
    void refreshInfo();
  };

  const switchTab = (t: Tab, keepSession = false) => {
    if (!keepSession && sessionRef.current) void commit("none");
    selections.current.set(tabKey(tabRef.current), selectionRef.current);
    tabRef.current = t;
    setTabState(t);
    const s = selections.current.get(tabKey(t)) ?? START;
    selectionRef.current = s;
    setSelectionState(s);
    focusGrid();
  };

  // ---------------------------------------------------------------- selection helpers
  const range = () => normalize(selectionRef.current.anchor, selectionRef.current.focus);
  const rowCount = () => sourceRef.current?.rows.count ?? 1;
  const colCount = () => sourceRef.current?.cols.count ?? 1;
  const clamp = (p: Pos): Pos => ({
    r: Math.min(Math.max(1, p.r), rowCount()),
    c: Math.min(Math.max(1, p.c), colCount()),
  });
  const moveTo = (p: Pos, extend = false) => {
    const q = clamp(p);
    setSelection({ anchor: extend ? selectionRef.current.anchor : q, focus: q });
  };

  /** What the formula bar shows for the active cell (synchronously, from loaded data). */
  const shownInput = (): string => {
    const t = tabRef.current;
    const f = selectionRef.current.focus;
    if (t.kind === "sheet") return detailRef.current?.input ?? "";
    const src = sourceRef.current;
    return src instanceof DataSource ? (src.cell(f.r, f.c)?.text ?? "").replace(/,/g, "") : "";
  };

  const currentInput = async (): Promise<string> => {
    const t = tabRef.current;
    const f = selectionRef.current.focus;
    if (t.kind === "sheet") return (await api.cellDetail(t.index, f.r, f.c)).input;
    return shownInput();
  };

  // ---------------------------------------------------------------- editing
  const newSession = (text: string, mode: Session["mode"], origin: Session["origin"], original: string): Session | null => {
    const pos = selectionRef.current.focus;
    const src = sourceRef.current;
    if (tabRef.current.kind === "data" && !(src instanceof DataSource && src.rowId(pos.r) !== undefined)) return null;
    return { tab: tabRef.current, pos, text, original, caret: text.length, mode, origin, point: null };
  };

  const startEdit = (text: string, mode: Session["mode"], origin: Session["origin"], original?: string) => {
    const s = newSession(text, mode, origin, original ?? text);
    if (!s) return;
    setSession(s);
    if (origin === "cell") {
      requestAnimationFrame(() => {
        const el = cellInput.current;
        if (el) {
          el.focus({ preventScroll: true });
          el.setSelectionRange(text.length, text.length);
        }
      });
    }
  };

  const commit = async (move: "down" | "up" | "right" | "left" | "none", fill = false) => {
    const s = sessionRef.current;
    if (!s) return;
    setSession(null);
    // Back to the sheet being edited if the user pointed into another one.
    if (tabKey(tabRef.current) !== tabKey(s.tab)) switchTab(s.tab, true);
    if (move !== "none" && !fill) moveAfterEntry(s.pos, move);
    focusGrid();
    if (s.text === s.original && !fill) return;
    if (s.tab.kind === "sheet") {
      const sheet = s.tab.index;
      const st = await run("계산 중", () => (fill ? api.fillRange(sheet, range(), s.text) : api.setCell(sheet, s.pos.r, s.pos.c, s.text)));
      if (st) await afterChange(st);
    } else {
      const src = sources.current.get(tabKey(s.tab));
      if (!(src instanceof DataSource)) return;
      const rid = src.rowId(s.pos.r);
      const col = src.column(s.pos.c);
      if (rid === undefined || !col) return;
      const st = await run("저장 중", () => api.updateDataCell(src.info.name, rid, col.name, s.text));
      if (st) {
        src.reload();
        await afterChange(st);
      }
    }
  };

  /** Where the cursor goes after Enter/Tab, including inside a selected range (Excel). */
  const moveAfterEntry = (from: Pos, move: "down" | "up" | "right" | "left") => {
    const [dr, dc] = { down: [1, 0], up: [-1, 0], right: [0, 1], left: [0, -1] }[move];
    const sel = range();
    const multi = sel.r0 !== sel.r1 || sel.c0 !== sel.c1;
    if (multi) {
      let { r, c } = from;
      if (dr) {
        r += dr;
        if (r > sel.r1) {
          r = sel.r0;
          c = c + 1 > sel.c1 ? sel.c0 : c + 1;
        } else if (r < sel.r0) {
          r = sel.r1;
          c = c - 1 < sel.c0 ? sel.c1 : c - 1;
        }
      } else {
        c += dc;
        if (c > sel.c1) {
          c = sel.c0;
          r = r + 1 > sel.r1 ? sel.r0 : r + 1;
        } else if (c < sel.c0) {
          c = sel.c1;
          r = r - 1 < sel.r0 ? sel.r1 : r - 1;
        }
      }
      setSelection({ anchor: selectionRef.current.anchor, focus: { r, c } });
      return;
    }
    if (move === "down" && tabStartCol.current !== null) {
      moveTo({ r: from.r + 1, c: tabStartCol.current });
      tabStartCol.current = null;
      return;
    }
    if (move === "right" && tabStartCol.current === null) tabStartCol.current = from.c;
    if (move === "up" || move === "left") tabStartCol.current = null;
    moveTo({ r: from.r + dr, c: from.c + dc });
  };

  const cancelEdit = () => {
    const s = sessionRef.current;
    setSession(null);
    if (s && tabKey(tabRef.current) !== tabKey(s.tab)) switchTab(s.tab, true);
    focusGrid();
  };

  const pointable = (s: Session) => s.text.startsWith("=") && (s.point !== null || canPointAt(s.text, s.caret));

  /** Sheet prefix for a reference made while viewing another sheet. */
  const pointSheet = (s: Session): string | null => {
    const t = tabRef.current;
    if (t.kind === "sheet" && s.tab.kind === "sheet" && t.index !== s.tab.index) return sheetName(t.index);
    return null;
  };

  /** Places the caret after React has rendered the new text; uses the latest session so quick drags don't race. */
  const setCaretSoon = (s: Session, caret: number) =>
    requestAnimationFrame(() => {
      const cur = sessionRef.current;
      const at = cur ? cur.caret : caret;
      const el = (cur ?? s).origin === "bar" ? barInput.current : cellInput.current;
      if (el && (el.selectionStart !== at || el.selectionEnd !== at)) el.setSelectionRange(at, at);
    });

  const pointAt = (anchor: Pos, focus: Pos) => {
    const s = sessionRef.current;
    if (!s) return;
    const r = insertRef(s.text, s.caret, s.point, anchor, focus, pointSheet(s));
    setSession({ ...s, text: r.text, caret: r.caret, point: r.point });
    setCaretSoon(s, r.caret);
  };

  /** Inserts `table[column]` while pointing at a data sheet. */
  const pointTableColumn = (c: number) => {
    const s = sessionRef.current;
    const src = sourceRef.current;
    if (!s || !(src instanceof DataSource)) return;
    const col = src.column(c);
    if (!col) return;
    const text = `${src.info.name}[${col.name}]`;
    const start = s.point ? s.point.start : s.caret;
    const end = s.point ? s.point.end : s.caret;
    const out = s.text.slice(0, start) + text + s.text.slice(end);
    const caret = start + text.length;
    setSession({ ...s, text: out, caret, point: { start, end: caret, anchor: { r: 1, c }, focus: { r: 1, c } } });
    setCaretSoon(s, caret);
  };

  // ---------------------------------------------------------------- autocomplete
  const ac = useMemo(() => {
    if (!session || acDismissed) return null;
    const p = functionPrefixAt(session.text, session.caret);
    if (!p) return null;
    const items = completeFunctions(functions, p.prefix, 10);
    if (items.length === 0 || (items.length === 1 && items[0] === p.prefix.toUpperCase())) return null;
    return { ...p, items };
  }, [session, functions, acDismissed]);

  const acceptCompletion = (name: string) => {
    const s = sessionRef.current;
    if (!s || !ac) return;
    const text = s.text.slice(0, ac.start) + name + "(" + s.text.slice(s.caret);
    const caret = ac.start + name.length + 1;
    setSession({ ...s, text, caret, point: null });
    setAcIndex(0);
    setCaretSoon(s, caret);
  };

  // ---------------------------------------------------------------- styles
  const applyStyle = async (path: string, value: string) => {
    if (tabRef.current.kind !== "sheet") return say("데이터 시트에는 서식을 적용할 수 없습니다", true);
    const sheet = tabRef.current.index;
    const st = await run("서식", () => api.setStyle(sheet, range(), path, value));
    if (st) await afterChange(st);
  };

  const toggleStyle = (path: "font.b" | "font.i" | "font.u" | "font.strike") => {
    const key = { "font.b": "bold", "font.i": "italic", "font.u": "underline", "font.strike": "strike" } as const;
    const cur = detailRef.current?.style[key[path]] ?? false;
    void applyStyle(path, cur ? "false" : "true");
  };

  const applyFormat = async (r: FormatResult) => {
    if (tabRef.current.kind !== "sheet") return;
    const sheet = tabRef.current.index;
    const ops: [string, string][] = [];
    if (r.numFmt !== undefined) ops.push(["num_fmt", r.numFmt]);
    if (r.align !== undefined) ops.push(["alignment.horizontal", r.align]);
    if (r.valign !== undefined) ops.push(["alignment.vertical", r.valign]);
    if (r.wrap !== undefined) ops.push(["alignment.wrap_text", String(r.wrap)]);
    if (r.font !== undefined) ops.push(["font.name", r.font]);
    if (r.size !== undefined) ops.push(["font.size", String(r.size)]);
    if (r.bold !== undefined) ops.push(["font.b", String(r.bold)]);
    if (r.italic !== undefined) ops.push(["font.i", String(r.italic)]);
    if (r.underline !== undefined) ops.push(["font.u", String(r.underline)]);
    if (r.strike !== undefined) ops.push(["font.strike", String(r.strike)]);
    if (r.color !== undefined) ops.push(["font.color", r.color ?? "#000000"]);
    if (r.fill !== undefined) ops.push(["fill.fg_color", r.fill ?? ""]);
    for (const [p, v] of ops) await api.setStyle(sheet, range(), p, v).catch(fail);
    if (r.border) await api.setBorder(sheet, range(), r.border, "thin", "#000000").catch(fail);
    await afterChange();
  };

  // ---------------------------------------------------------------- clipboard
  const copyFromCache = (rg: Range) => {
    const src = sourceRef.current;
    if (!src) return "";
    const rows: string[][] = [];
    for (let r = rg.r0; r <= Math.min(rg.r1, rg.r0 + 20000); r++) {
      const row: string[] = [];
      for (let c = rg.c0; c <= Math.min(rg.c1, rg.c0 + 500); c++) row.push(src.cell(r, c)?.text ?? "");
      rows.push(row);
    }
    return toTsv(rows);
  };

  const doCopy = (cut: boolean, e?: React.ClipboardEvent) => {
    const rg = range();
    const text = copyFromCache(rg);
    if (e) {
      e.preventDefault();
      e.clipboardData.setData("text/plain", text);
    }
    lastCopy.current = { partial: text, full: null };
    setCopyMark({ tab: tabKey(tabRef.current), range: rg, cut });
    const t = tabRef.current;
    if (t.kind === "sheet") {
      void api.copy(t.index, rg, cut).then((full) => {
        lastCopy.current.full = full;
        if (!e || full.trim() !== text.trim()) navigator.clipboard?.writeText(full).catch(() => {});
      });
    } else if (!e) navigator.clipboard?.writeText(text).catch(() => {});
  };

  const doPaste = async (text: string) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("데이터 시트에는 붙여넣을 수 없습니다", true);
    const norm = (s: string) => s.replace(/\r\n/g, "\n").replace(/\n+$/, "");
    const internal =
      copyMark !== null &&
      (norm(text) === norm(lastCopy.current.partial) || (lastCopy.current.full !== null && norm(text) === norm(lastCopy.current.full)));
    const at = range();
    const st = await run("붙여넣는 중", () => api.paste(t.index, at.r0, at.c0, text, internal));
    if (st) {
      if (copyMark?.cut) setCopyMark(null);
      await afterChange(st);
    }
  };

  // ---------------------------------------------------------------- shared actions
  const onSheetOnly = <T,>(label: string, fn: (sheet: number) => Promise<T>) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return Promise.resolve(undefined);
    return run(label, () => fn(t.index));
  };

  const insertRows = async () => {
    const rg = range();
    const st = await onSheetOnly("행 삽입", (s) => api.insertRows(s, rg.r0, rg.r1 - rg.r0 + 1));
    if (st) await afterChange(st);
  };
  const insertColumns = async () => {
    const rg = range();
    const st = await onSheetOnly("열 삽입", (s) => api.insertColumns(s, rg.c0, rg.c1 - rg.c0 + 1));
    if (st) await afterChange(st);
  };
  const deleteRows = async () => {
    const rg = range();
    const st = await onSheetOnly("행 삭제", (s) => api.deleteRows(s, rg.r0, rg.r1 - rg.r0 + 1));
    if (st) await afterChange(st);
  };
  const deleteColumns = async () => {
    const rg = range();
    const st = await onSheetOnly("열 삭제", (s) => api.deleteColumns(s, rg.c0, rg.c1 - rg.c0 + 1));
    if (st) await afterChange(st);
  };
  const wholeCols = () => {
    const rg = range();
    return rg.r0 === 1 && rg.r1 >= rowCount();
  };

  const clearRange = async (what: "contents" | "formats" | "all") => {
    const st = await onSheetOnly("지우는 중", (s) => api.clear(s, range(), what));
    if (st) await afterChange(st);
  };

  const autosum = async (fn = "SUM") => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const f = selectionRef.current.focus;
    const formula = (await api.autosumFormula(t.index, f.r, f.c)).replace("=SUM(", `=${fn}(`);
    // Like Excel: the proposal goes into the editor with its range selected for review.
    startEdit(formula, "enter", "cell", detailRef.current?.input ?? "");
    requestAnimationFrame(() => {
      const open = formula.indexOf("(") + 1;
      cellInput.current?.setSelectionRange(open, formula.length - 1);
    });
  };

  const fillDir = async (dir: "down" | "right") => {
    const rg = range();
    let src: Range;
    let target: Range = rg;
    if ((dir === "down" && rg.r0 === rg.r1) || (dir === "right" && rg.c0 === rg.c1)) {
      // One row/column selected: copy from the cell above / to the left (Excel).
      src = dir === "down" ? { ...rg, r0: rg.r0 - 1, r1: rg.r0 - 1 } : { ...rg, c0: rg.c0 - 1, c1: rg.c0 - 1 };
      if (src.r0 < 1 || src.c0 < 1) return;
      target = dir === "down" ? { ...rg, r0: src.r0 } : { ...rg, c0: src.c0 };
    } else {
      src = dir === "down" ? { ...rg, r1: rg.r0 } : { ...rg, c1: rg.c0 };
    }
    const st = await onSheetOnly("채우기", (s) => api.autoFill(s, src, target));
    if (st) await afterChange(st);
  };

  const setSize = async (rows: boolean) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const rg = range();
    const cur = rows ? sourceRef.current?.rows.size(rg.r0) : sourceRef.current?.cols.size(rg.c0);
    const v = await ask(rows ? "행 높이" : "열 너비", rows ? "행 높이 (픽셀)" : "열 너비 (픽셀)", String(Math.round(cur ?? 0)));
    const n = Number(v);
    if (!v || !Number.isFinite(n) || n < 0) return;
    await (rows ? api.setRowHeight(t.index, rg.r0, rg.r1, n) : api.setColumnWidth(t.index, rg.c0, rg.c1, n)).catch(fail);
    await afterChange();
  };

  const autofit = async (c0: number, c1: number) => {
    const src = sourceRef.current;
    const ctx = document.createElement("canvas").getContext("2d")!;
    if (src instanceof DataSource) {
      for (let c = c0; c <= c1; c++) {
        ctx.font = fontCss(undefined, src.font, 1);
        let w = ctx.measureText(`${src.colHeader(c)}  ${src.colSubHeader(c)}`).width + 24;
        for (let r = 1; r <= Math.min(src.total, 300); r++) w = Math.max(w, ctx.measureText(src.cell(r, c)?.text ?? "").width + 12);
        src.cols.set(c, Math.ceil(w));
      }
      force((x) => x + 1);
      return;
    }
    if (!(src instanceof SheetSource)) return;
    for (let c = c0; c <= c1; c++) {
      const vp = await api.viewport(src.sheet, 1, c, Math.min(Math.max(1, src.lastRow), 20000), c);
      let w = 0;
      for (const cell of vp.cells) {
        if (!cell.text || src.mergeAt(cell.row, cell.col)) continue;
        ctx.font = fontCss(cell.style, src.font, 1);
        w = Math.max(w, ctx.measureText(cell.text).width + 10);
      }
      if (w > 0) await api.setColumnWidth(src.sheet, c, c, Math.ceil(w)).catch(fail);
    }
    await afterChange();
  };

  const hide = async (rows: boolean, hidden: boolean) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const rg = range();
    const from = rows ? rg.r0 : rg.c0;
    const to = rows ? rg.r1 : rg.c1;
    // Unhiding works on the selection and its neighbours, as in Excel.
    await api.setHidden(t.index, rows, hidden ? from : Math.max(1, from - 1), hidden ? to : to + 1, hidden).catch(fail);
    await afterChange();
  };

  const freeze = async (kind: "panes" | "row" | "col" | "none") => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const f = selectionRef.current.focus;
    const [r, c] = kind === "panes" ? [f.r - 1, f.c - 1] : kind === "row" ? [1, 0] : kind === "col" ? [0, 1] : [0, 0];
    await api.setFrozen(t.index, r, c).catch(fail);
    await afterChange();
  };

  /** Ctrl+A: the current region first, then the whole sheet. */
  const selectRegionOrAll = async () => {
    const t = tabRef.current;
    if (t.kind === "sheet") {
      const cur = range();
      const f = selectionRef.current.focus;
      const reg = await api.currentRegion(t.index, f.r, f.c);
      const same = reg.r0 === cur.r0 && reg.r1 === cur.r1 && reg.c0 === cur.c0 && reg.c1 === cur.c1;
      const single = reg.r0 === reg.r1 && reg.c0 === reg.c1;
      if (!same && !single) {
        setSelection({ anchor: { r: reg.r1, c: reg.c1 }, focus: { r: reg.r0, c: reg.c0 } });
        return;
      }
    }
    setSelection({ anchor: { r: rowCount(), c: colCount() }, focus: { r: 1, c: 1 } });
  };

  // ---------------------------------------------------------------- files
  const resetBook = (i: BookInfo) => {
    sources.current.clear();
    selections.current.clear();
    setCopyMark(null);
    setInfo(i);
    switchTab({ kind: "sheet", index: 0 });
    setSelection(START);
  };

  const newBook = async () => {
    const i = await run("새 문서", api.newBook);
    if (i) resetBook(i);
  };

  const openFile = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [
        { name: "Excel 통합 문서 / CellX 프로젝트", extensions: ["xlsx", "cellx"] },
        { name: "모든 파일", extensions: ["*"] },
      ],
    });
    if (!path || Array.isArray(path)) return;
    const t = performance.now();
    const i = await run("여는 중", () => api.openFile(path));
    if (!i) return;
    resetBook(i);
    say(`${path} (${Math.round(performance.now() - t)} ms)`);
  };

  const saveAs = async () => {
    const path = await saveDialog({
      defaultPath: info?.path ?? "통합 문서1.xlsx",
      filters: [
        { name: "Excel 통합 문서", extensions: ["xlsx"] },
        { name: "CellX 프로젝트 (데이터 시트 포함)", extensions: ["cellx"] },
      ],
    });
    if (!path) return;
    const i = await run("저장 중", () => api.saveFile(path));
    if (i) {
      setInfo(i);
      say(
        path.toLowerCase().endsWith(".xlsx") && i.tables.length > 0
          ? "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다"
          : `저장했습니다: ${path}`,
      );
    }
  };

  const save = async () => {
    if (!info?.path) return saveAs();
    const path = info.path;
    const i = await run("저장 중", () => api.saveFile(path));
    if (i) {
      setInfo(i);
      say(`저장했습니다: ${path}`);
    }
  };

  const importData = async () => {
    const path = await openDialog({ multiple: false, filters: [{ name: "데이터 파일", extensions: ["csv", "tsv", "txt", "parquet"] }] });
    if (!path || Array.isArray(path)) return;
    const base = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "data";
    const suggested = base.replace(/[^\p{L}\p{N}_]/gu, "_").replace(/^(\d)/, "_$1");
    const name = await ask("데이터 시트 가져오기", "데이터 시트 이름 (수식에서 이름[열]로 참조합니다)", suggested);
    if (!name) return;
    const t = performance.now();
    const table = await run("가져오는 중", () => api.importData(path, name));
    if (!table) return;
    await refreshInfo();
    sources.current.delete(tabKey({ kind: "data", name: table.name }));
    switchTab({ kind: "data", name: table.name });
    await invalidateSheets();
    say(`${formatCount(table.rows)}행을 가져왔습니다 (${((performance.now() - t) / 1000).toFixed(1)}초)`);
  };

  const exportTable = async () => {
    const src = sourceRef.current;
    if (!(src instanceof DataSource)) return;
    const path = await saveDialog({
      defaultPath: `${src.info.name}.csv`,
      filters: [
        { name: "CSV", extensions: ["csv"] },
        { name: "Parquet", extensions: ["parquet"] },
      ],
    });
    if (!path) return;
    const ok = await run("내보내는 중", () => api.exportTable(src.info.name, path));
    if (ok !== undefined) say(`내보냈습니다: ${path}`);
  };

  const insertSheet = async () => {
    const i = await run("시트 삽입", api.addSheet);
    if (i) {
      setInfo(i);
      switchTab({ kind: "sheet", index: i.sheets.length - 1 });
    }
  };

  const deleteSheet = async (index: number) => {
    if (index < 0) return;
    const i = await run("시트 삭제", () => api.deleteSheet(index));
    if (i) {
      sources.current.clear();
      selections.current.clear();
      setInfo(i);
      switchTab({ kind: "sheet", index: Math.max(0, index - 1) });
    }
  };

  const renameSheet = async (index: number) => {
    const name = await ask("시트 이름 바꾸기", "새 이름", info?.sheets[index] ?? "");
    if (!name) return;
    const i = await run("이름 바꾸기", () => api.renameSheet(index, name));
    if (i) setInfo(i);
  };

  const undo = async () => {
    if (sessionRef.current) return;
    const st = await run("실행 취소", api.undo);
    if (st) await afterChange(st);
  };
  const redo = async () => {
    if (sessionRef.current) return;
    const st = await run("다시 실행", api.redo);
    if (st) await afterChange(st);
  };

  const find = async (q: string, matchCase: boolean) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const f = selectionRef.current.focus;
    const at = await api.find(t.index, q, f.r, f.c, matchCase).catch(fail);
    if (at) {
      moveTo({ r: at[0], c: at[1] });
      setFindMsg(null);
    } else setFindMsg("찾는 내용이 없습니다.");
  };

  // ---------------------------------------------------------------- keys on the grid (not editing)
  const gridKey = async (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const mod = e.ctrlKey || e.metaKey;
    const f = selectionRef.current.focus;
    const t = tabRef.current;
    const dir = ARROWS[e.key];
    if (dir) {
      e.preventDefault();
      tabStartCol.current = null;
      const [dr, dc] = DELTA[dir];
      if (mod) {
        if (t.kind === "sheet") {
          // Ctrl+Arrow jumps from the moving end of the selection.
          const [r, c] = await api.navigateEdge(t.index, f.r, f.c, dir);
          moveTo({ r, c }, e.shiftKey);
        } else {
          moveTo({ r: dr > 0 ? rowCount() : dr < 0 ? 1 : f.r, c: dc > 0 ? colCount() : dc < 0 ? 1 : f.c }, e.shiftKey);
        }
      } else moveTo({ r: f.r + dr, c: f.c + dc }, e.shiftKey);
      return;
    }
    const k = e.key;
    const code = e.code;
    const handled = () => e.preventDefault();
    if (k === "Enter") {
      handled();
      if (copyMark && !mod && !e.shiftKey) {
        // Enter after copying pastes once (Excel), then the marquee goes away.
        await doPaste(lastCopy.current.full ?? lastCopy.current.partial);
        setCopyMark(null);
        return;
      }
      moveAfterEntry(f, e.shiftKey ? "up" : "down");
    } else if (k === "Tab") {
      handled();
      moveAfterEntry(f, e.shiftKey ? "left" : "right");
    } else if (k === "Home") {
      handled();
      moveTo({ r: mod ? 1 : f.r, c: 1 }, e.shiftKey);
    } else if (k === "End" && mod) {
      handled();
      const src = sourceRef.current;
      if (src instanceof SheetSource) moveTo({ r: Math.max(1, src.lastRow), c: Math.max(1, src.lastCol) }, e.shiftKey);
      else moveTo({ r: rowCount(), c: colCount() }, e.shiftKey);
    } else if (k === "PageDown" || k === "PageUp") {
      handled();
      const down = k === "PageDown";
      if (mod) {
        // Ctrl+PageUp/PageDown: previous/next sheet.
        const tabs: Tab[] = [
          ...(info?.sheets ?? []).map((_, i) => ({ kind: "sheet", index: i }) as Tab),
          ...(info?.tables ?? []).map((x) => ({ kind: "data", name: x.name }) as Tab),
        ];
        const i = tabs.findIndex((x) => tabKey(x) === tabKey(t));
        const next = tabs[i + (down ? 1 : -1)];
        if (next) switchTab(next);
        return;
      }
      if (e.altKey) moveTo({ r: f.r, c: f.c + (down ? 10 : -10) }, e.shiftKey);
      else moveTo({ r: f.r + (down ? 1 : -1) * pageRows.current, c: f.c }, e.shiftKey);
    } else if (k === "F2") {
      handled();
      const input = await currentInput();
      startEdit(input, "edit", "cell", input);
    } else if (k === "Delete") {
      handled();
      void clearRange("contents");
    } else if (k === "Backspace") {
      handled();
      startEdit("", "enter", "cell", shownInput());
    } else if (k === "Escape") {
      handled();
      if (copyMark) {
        setCopyMark(null);
        void api.clearClipboard();
      }
    } else if (k === "F11" && e.shiftKey) {
      handled();
      void insertSheet();
    } else if (k === "F5") {
      handled();
      nameBox.current?.focus();
    } else if (k === " " && e.shiftKey && !mod) {
      handled();
      setSelection({ anchor: { r: range().r0, c: colCount() }, focus: { r: f.r, c: 1 } });
    } else if (k === " " && mod) {
      handled();
      setSelection({ anchor: { r: rowCount(), c: range().c0 }, focus: { r: 1, c: f.c } });
    } else if (e.altKey && (k === "=" || code === "Equal")) {
      handled();
      void autosum("SUM");
    } else if (mod) {
      const lower = k.toLowerCase();
      if (lower === "z") {
        handled();
        void undo();
      } else if (lower === "y") {
        handled();
        void redo();
      } else if (lower === "a") {
        handled();
        void selectRegionOrAll();
      } else if (lower === "b") {
        handled();
        toggleStyle("font.b");
      } else if (lower === "i") {
        handled();
        toggleStyle("font.i");
      } else if (lower === "u") {
        handled();
        toggleStyle("font.u");
      } else if (code === "Digit5" && !e.shiftKey) {
        handled();
        toggleStyle("font.strike");
      } else if (code === "Digit1" && !e.shiftKey) {
        handled();
        if (t.kind === "sheet") setDialog({ kind: "format" });
      } else if (lower === "d") {
        handled();
        void fillDir("down");
      } else if (lower === "r") {
        handled();
        void fillDir("right");
      } else if (code === "Semicolon") {
        handled();
        const now = new Date();
        const p2 = (n: number) => String(n).padStart(2, "0");
        const text = e.shiftKey ? `${now.getHours()}:${p2(now.getMinutes())}` : `${now.getFullYear()}-${p2(now.getMonth() + 1)}-${p2(now.getDate())}`;
        startEdit(text, "enter", "cell", shownInput());
      } else if (e.shiftKey && code === "Digit1") {
        handled();
        void applyStyle("num_fmt", "#,##0.00");
      } else if (e.shiftKey && code === "Digit3") {
        handled();
        void applyStyle("num_fmt", "yyyy-mm-dd");
      } else if (e.shiftKey && code === "Digit4") {
        handled();
        void applyStyle("num_fmt", "₩#,##0");
      } else if (e.shiftKey && code === "Digit5") {
        handled();
        void applyStyle("num_fmt", "0%");
      } else if (e.shiftKey && code === "Backquote") {
        handled();
        void applyStyle("num_fmt", "general");
      } else if (e.shiftKey && (code === "Period" || code === "Comma")) {
        handled();
        const cur = detailRef.current?.style.size ?? 11;
        const next = code === "Period" ? FONT_SIZES.find((s) => s > cur) ?? cur : [...FONT_SIZES].reverse().find((s) => s < cur) ?? cur;
        void applyStyle("font.size", String(next));
      } else if (code === "Equal" || k === "+" || code === "NumpadAdd") {
        handled();
        if (wholeCols()) void insertColumns();
        else void insertRows();
      } else if (code === "Minus" || code === "NumpadSubtract") {
        handled();
        if (wholeCols()) void deleteColumns();
        else void deleteRows();
      } else if (code === "Digit9") {
        handled();
        void hide(true, !e.shiftKey);
      } else if (code === "Digit0") {
        handled();
        void hide(false, !e.shiftKey);
      } else if (lower === "f" || lower === "h") {
        handled();
        setFindOpen(true);
      } else if (lower === "g") {
        handled();
        nameBox.current?.focus();
      }
      // Ctrl+C / Ctrl+X / Ctrl+V reach the copy, cut and paste events.
    }
  };

  // ---------------------------------------------------------------- keys while editing
  const editKey = (e: React.KeyboardEvent<HTMLTextAreaElement>, s: Session) => {
    const mod = e.ctrlKey || e.metaKey;
    if (ac) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setAcIndex((i) => (i + (e.key === "ArrowDown" ? 1 : ac.items.length - 1)) % ac.items.length);
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        acceptCompletion(ac.items[acIndex % ac.items.length]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setAcDismissed(true);
        return;
      }
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.altKey) {
        // Alt+Enter: a line break inside the cell.
        const ta = e.currentTarget;
        const caret = ta.selectionStart ?? s.text.length;
        const text = s.text.slice(0, caret) + "\n" + s.text.slice(ta.selectionEnd ?? caret);
        setSession({ ...s, text, caret: caret + 1, point: null });
        requestAnimationFrame(() => ta.setSelectionRange(caret + 1, caret + 1));
        return;
      }
      void commit(mod ? "none" : e.shiftKey ? "up" : "down", mod);
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      void commit(e.shiftKey ? "left" : "right");
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
      return;
    }
    if (e.key === "F2") {
      e.preventDefault();
      setSession({ ...s, mode: s.mode === "enter" ? "edit" : "enter", point: null });
      return;
    }
    if (e.key === "F4") {
      e.preventDefault();
      const caret = e.currentTarget.selectionStart ?? s.caret;
      const r = cycleAbsolute(s.text, caret);
      if (r) {
        setSession({ ...s, text: r.text, caret: r.caret, point: null });
        setCaretSoon(s, r.caret);
      }
      return;
    }
    const dir = ARROWS[e.key];
    if (dir && s.mode === "enter" && s.origin === "cell") {
      e.preventDefault();
      if (pointable(s) && tabRef.current.kind === "sheet") {
        // Point mode: arrows move a reference instead of leaving the cell.
        const here = tabKey(tabRef.current) === tabKey(s.tab);
        const base = s.point ? s.point.focus : here ? s.pos : selectionRef.current.focus;
        const [dr, dc] = DELTA[dir];
        const next = clamp({ r: base.r + dr, c: base.c + dc });
        const anchor = e.shiftKey && s.point ? s.point.anchor : next;
        pointAt(anchor, next);
        if (!here) setSelection({ anchor: next, focus: next });
        return;
      }
      void commit(({ Up: "up", Down: "down", Left: "left", Right: "right" } as const)[dir]);
    }
  };

  const onCellKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing || e.key === "Process") return;
    const s = sessionRef.current;
    if (!s) void gridKey(e);
    else editKey(e, s);
  };

  // ---------------------------------------------------------------- mouse
  const onCellDown = (h: Hit, e: React.MouseEvent) => {
    setMenu(null);
    const s = sessionRef.current;
    const src = sourceRef.current;
    if (s && pointable(s)) {
      if (src instanceof DataSource) {
        if (h.area === "colHeader" || h.area === "cell") pointTableColumn(h.c);
        drag.current = null;
        return;
      }
      if (h.area === "cell") {
        const anchor = e.shiftKey && s.point ? s.point.anchor : { r: h.r, c: h.c };
        pointAt(anchor, { r: h.r, c: h.c });
        drag.current = "point";
        if (tabKey(tabRef.current) !== tabKey(s.tab)) setSelection({ anchor: { r: h.r, c: h.c }, focus: { r: h.r, c: h.c } });
        return;
      }
    }
    if (s) void commit("none");
    tabStartCol.current = null;
    if (h.area === "corner") {
      setSelection({ anchor: { r: rowCount(), c: colCount() }, focus: { r: 1, c: 1 } });
    } else if (h.area === "colHeader") {
      if (src instanceof DataSource) {
        const col = src.column(h.c);
        if (col) {
          const cur = src.sort;
          const next = cur?.column !== col.name ? { column: col.name, descending: false } : cur.descending ? null : { column: col.name, descending: true };
          src.setView(next, src.filters);
        }
        return;
      }
      drag.current = "col";
      const ac0 = e.shiftKey ? selectionRef.current.anchor.c : h.c;
      setSelection({ anchor: { r: rowCount(), c: ac0 }, focus: { r: 1, c: h.c } });
    } else if (h.area === "rowHeader") {
      drag.current = "row";
      const ar = e.shiftKey ? selectionRef.current.anchor.r : h.r;
      setSelection({ anchor: { r: ar, c: colCount() }, focus: { r: h.r, c: 1 } });
    } else {
      drag.current = "cell";
      moveTo({ r: h.r, c: h.c }, e.shiftKey);
    }
    focusGrid();
  };

  const onCellDrag = (h: Hit) => {
    const d = drag.current;
    const s = sessionRef.current;
    if (d === "point" && s?.point) pointAt(s.point.anchor, { r: h.r, c: h.c });
    else if (d === "cell") setSelection({ anchor: selectionRef.current.anchor, focus: { r: h.r, c: h.c } });
    else if (d === "col") setSelection({ anchor: selectionRef.current.anchor, focus: { r: 1, c: h.c } });
    else if (d === "row") setSelection({ anchor: selectionRef.current.anchor, focus: { r: h.r, c: 1 } });
  };

  const onContextMenu = (h: Hit, x: number, y: number) => {
    if (sessionRef.current) void commit("none");
    const rg = range();
    const inside = h.r >= rg.r0 && h.r <= rg.r1 && h.c >= rg.c0 && h.c <= rg.c1;
    if (!inside) {
      if (h.area === "rowHeader") setSelection({ anchor: { r: h.r, c: colCount() }, focus: { r: h.r, c: 1 } });
      else if (h.area === "colHeader") setSelection({ anchor: { r: rowCount(), c: h.c }, focus: { r: 1, c: h.c } });
      else moveTo({ r: h.r, c: h.c });
    }
    const onSheet = tabRef.current.kind === "sheet";
    const clip: MenuItem[] = [
      { label: "잘라내기", shortcut: "Ctrl+X", disabled: !onSheet, onClick: () => doCopy(true) },
      { label: "복사", shortcut: "Ctrl+C", onClick: () => doCopy(false) },
      { label: "붙여넣기", shortcut: "Ctrl+V", disabled: !onSheet, onClick: () => actions.paste() },
      { separator: true },
    ];
    const format: MenuItem = { label: "셀 서식...", shortcut: "Ctrl+1", onClick: () => setDialog({ kind: "format" }) };
    let items: MenuItem[];
    if (!onSheet) items = [clip[1]];
    else if (h.area === "rowHeader")
      items = [
        ...clip,
        { label: "삽입", onClick: insertRows },
        { label: "삭제", onClick: deleteRows },
        { label: "내용 지우기", onClick: () => clearRange("contents") },
        { separator: true },
        format,
        { label: "행 높이...", onClick: () => setSize(true) },
        { label: "숨기기", onClick: () => hide(true, true) },
        { label: "숨기기 취소", onClick: () => hide(true, false) },
      ];
    else if (h.area === "colHeader")
      items = [
        ...clip,
        { label: "삽입", onClick: insertColumns },
        { label: "삭제", onClick: deleteColumns },
        { label: "내용 지우기", onClick: () => clearRange("contents") },
        { separator: true },
        format,
        { label: "열 너비...", onClick: () => setSize(false) },
        { label: "숨기기", onClick: () => hide(false, true) },
        { label: "숨기기 취소", onClick: () => hide(false, false) },
      ];
    else
      items = [
        ...clip,
        { label: "행 삽입", onClick: insertRows },
        { label: "열 삽입", onClick: insertColumns },
        { label: "행 삭제", onClick: deleteRows },
        { label: "열 삭제", onClick: deleteColumns },
        { label: "내용 지우기", shortcut: "Delete", onClick: () => clearRange("contents") },
        { separator: true },
        format,
      ];
    setMenu({ x, y, items });
  };

  // ---------------------------------------------------------------- global shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      if (mod && k === "s") {
        e.preventDefault();
        void (e.shiftKey ? saveAs() : save());
      } else if (mod && k === "o") {
        e.preventDefault();
        void openFile();
      } else if (mod && k === "n") {
        e.preventDefault();
        void newBook();
      } else if (e.key === "F12") {
        e.preventDefault();
        void saveAs();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ---------------------------------------------------------------- ribbon actions
  const actions: RibbonActions = {
    newBook,
    open: openFile,
    save,
    saveAs,
    undo,
    redo,
    cut: () => doCopy(true),
    copy: () => doCopy(false),
    paste: async () => {
      let text: string | null = null;
      try {
        text = (await navigator.clipboard?.readText()) ?? null;
      } catch {
        text = null;
      }
      if (!text && copyMark) text = lastCopy.current.full ?? lastCopy.current.partial;
      if (text) await doPaste(text);
      else say("클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.", true);
      focusGrid();
    },
    toggle: toggleStyle,
    style: (p, v) => void applyStyle(p, v),
    growFont: (d) => {
      const cur = detailRef.current?.style.size ?? 11;
      const next = d > 0 ? FONT_SIZES.find((s) => s > cur) ?? cur : [...FONT_SIZES].reverse().find((s) => s < cur) ?? cur;
      void applyStyle("font.size", String(next));
    },
    border: async (kind) => {
      const thick = kind === "OuterThick";
      const st = await onSheetOnly("테두리", (s) => api.setBorder(s, range(), thick ? "Outer" : kind, thick ? "thick" : "thin", "#000000"));
      if (st) await afterChange(st);
    },
    align: (h) => void applyStyle("alignment.horizontal", detailRef.current?.style.align === h ? "general" : h),
    valign: (v) => void applyStyle("alignment.vertical", v),
    wrap: () => void applyStyle("alignment.wrap_text", detailRef.current?.style.wrap ? "false" : "true"),
    mergeCenter: async () => {
      const merged = detailRef.current?.style.merged;
      const st = await onSheetOnly("병합", (s) => (merged ? api.unmerge(s, range()) : api.merge(s, range(), true)));
      if (st) await afterChange(st);
    },
    incDec: (d) => {
      const cur = detailRef.current?.style.num_fmt ?? "general";
      void applyStyle("num_fmt", d > 0 ? increaseDecimals(cur) : decreaseDecimals(cur));
    },
    formatCells: () => tabRef.current.kind === "sheet" && setDialog({ kind: "format" }),
    insertRows,
    insertColumns,
    deleteRows,
    deleteColumns,
    insertSheet,
    deleteSheet: () => void deleteSheet(sheetIndex),
    rowHeight: () => void setSize(true),
    columnWidth: () => void setSize(false),
    autofitColumns: () => {
      const rg = range();
      void autofit(rg.c0, rg.c1);
    },
    hide: (rows, hidden) => void hide(rows, hidden),
    autosum: (fn) => void autosum(fn),
    fill: (d) => void fillDir(d),
    clear: (w) => void clearRange(w),
    find: () => setFindOpen(true),
    insertFunction: (name) => {
      const s = sessionRef.current;
      if (s) {
        const text = s.text.slice(0, s.caret) + name + "(" + s.text.slice(s.caret);
        const caret = s.caret + name.length + 1;
        setSession({ ...s, text, caret, point: null });
        setCaretSoon(s, caret);
      } else startEdit(`=${name}(`, "enter", "cell", shownInput());
    },
    importData,
    toggleSql: () => setShowSql((v) => !v),
    exportTable,
    freeze: (k) => void freeze(k),
    toggleGrid: async () => {
      const src = sourceRef.current;
      if (!(src instanceof SheetSource)) return;
      await api.setShowGridLines(src.sheet, !src.showGrid).catch(fail);
      await afterChange();
    },
    zoom: (z) => setZoom(z),
    zoomToSelection: () => {
      const src = sourceRef.current;
      if (!src) return;
      const rg = range();
      const w = src.cols.offset(rg.c1 + 1) - src.cols.offset(rg.c0);
      const h = src.rows.offset(rg.r1 + 1) - src.rows.offset(rg.r0);
      const host = document.querySelector(".grid-host")?.getBoundingClientRect();
      if (!host || w <= 0 || h <= 0) return;
      setZoom(Math.min(4, Math.max(0.1, Math.floor(Math.min((host.width - 60) / w, (host.height - 30) / h) * 10) / 10)));
    },
  };

  // ---------------------------------------------------------------- rendering
  const onSheetTab = tab.kind === "sheet";
  const editingHere = session !== null && tabKey(session.tab) === tabKey(tab);
  const editorPos = editingHere ? session!.pos : selection.focus;
  const formulaBarText = session ? session.text : shownInput();

  const refBoxes: RefBox[] = useMemo(() => {
    if (!session || tab.kind !== "sheet") return [];
    const viewName = (info?.sheets[tab.index] ?? "").toLowerCase();
    const ownName = session.tab.kind === "sheet" ? (info?.sheets[session.tab.index] ?? "") : "";
    return refSpans(session.text)
      .filter((s) => (s.sheet ?? ownName).toLowerCase() === viewName)
      .map((s) => ({ range: { r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c1 }, color: s.color }));
  }, [session, tab, info]);

  const scrollTarget = session?.point && tabKey(session.tab) === tabKey(tab) ? session.point.focus : selection.focus;
  const mode = !session ? "준비" : session.point ? "참조" : session.mode === "edit" ? "편집" : "입력";
  const fileName = info?.path?.split(/[\\/]/).pop() ?? "통합 문서1";
  useEffect(() => {
    document.title = `${info?.dirty ? "● " : ""}${fileName} - CellX`;
  }, [fileName, info?.dirty]);

  if (!isTauri()) {
    return (
      <div className="not-tauri">
        <h1>CellX</h1>
        <p>
          이 화면은 데스크톱 앱 안에서 실행됩니다. <code>pnpm tauri dev</code>로 실행하세요.
        </p>
      </div>
    );
  }

  const acList = ac && (
    <ul className="ac-list" onMouseDown={(e) => e.preventDefault()}>
      {ac.items.map((n, i) => (
        <li
          key={n}
          className={i === acIndex % ac.items.length ? "on" : ""}
          onDoubleClick={() => acceptCompletion(n)}
          onClick={() => setAcIndex(i)}
        >
          <span className="fx-ic">ƒx</span>
          {n}
        </li>
      ))}
    </ul>
  );

  const onBarCaret = (caret: number) => {
    const s = sessionRef.current;
    if (s && s.caret !== caret) setSession({ ...s, caret, point: s.point && s.point.end === caret ? s.point : null });
  };

  return (
    <div className="app">
      <div className="titlebar">
        <span className="logo">CX</span>
        <span className="doc-title">
          {info?.dirty ? "● " : ""}
          {fileName} - CellX
        </span>
        {busy && <span className="busy">{busy}…</span>}
      </div>
      <Ribbon
        actions={actions}
        style={onSheetTab ? (detail?.style ?? null) : null}
        canUndo={info?.can_undo ?? false}
        canRedo={info?.can_redo ?? false}
        showGrid={source instanceof SheetSource ? source.showGrid : true}
        sqlOpen={showSql}
        onDataSheet={tab.kind === "data"}
        functions={functions}
      />

      <div className="formula-bar">
        <input
          ref={nameBox}
          className="name-box"
          defaultValue={rangeLabel(selection.anchor, selection.focus)}
          key={`${tabKey(tab)}:${rangeLabel(selection.anchor, selection.focus)}`}
          onKeyDown={(e) => {
            if (e.key === "Escape") return focusGrid();
            if (e.key !== "Enter") return;
            const v = (e.target as HTMLInputElement).value.trim();
            const [a, b] = v.split(":");
            const pa = parseAddr(a ?? "");
            const pb = b ? parseAddr(b) : pa;
            if (pa && pb) setSelection({ anchor: pa, focus: pb });
            else say(`셀 주소가 올바르지 않습니다: ${v}`, true);
            focusGrid();
          }}
          onFocus={(e) => e.target.select()}
        />
        <div className="fb-buttons">
          <button title="취소" disabled={!session} onMouseDown={(e) => e.preventDefault()} onClick={cancelEdit}>
            ✕
          </button>
          <button title="입력" disabled={!session} onMouseDown={(e) => e.preventDefault()} onClick={() => void commit("none")}>
            ✓
          </button>
          <button title="함수 삽입" onMouseDown={(e) => e.preventDefault()} onClick={() => actions.insertFunction("SUM")}>
            <i>fx</i>
          </button>
        </div>
        <div className="fb-input-wrap">
          <FormulaText
            ref={barInput}
            className="formula-input"
            value={formulaBarText}
            onFocus={() => {
              const s = sessionRef.current;
              if (!s) {
                const input = shownInput();
                const ns = newSession(input, "edit", "bar", input);
                if (ns) setSession(ns);
              } else if (s.origin === "cell") setSession({ ...s, origin: "bar", mode: "edit" });
            }}
            onChange={(text, caret) => {
              const s = sessionRef.current;
              if (s) setSession({ ...s, text, caret, point: null });
            }}
            onCaret={onBarCaret}
            onKeyDown={(e) => {
              const s = sessionRef.current;
              if (s && !e.nativeEvent.isComposing) editKey(e, s);
            }}
          />
          {session?.origin === "bar" && acList}
        </div>
      </div>

      <main className="main">
        <div className="grid-host">
          {source && (
            <Grid
              key={tabKey(tab)}
              source={source}
              selection={selection}
              zoom={zoom}
              scrollTarget={scrollTarget}
              refBoxes={refBoxes}
              copyRange={copyMark && copyMark.tab === tabKey(tab) ? copyMark.range : null}
              editorAt={editorPos}
              renderEditor={(rect, font) => {
                const width = editingHere ? editorWidth(session!.text, font, rect.w) : rect.w;
                const lines = editingHere ? session!.text.split("\n").length : 1;
                return (
                <div
                  className={`cell-editor-host ${editingHere ? "editing" : ""}`}
                  style={{ left: rect.x, top: rect.y, width, height: Math.max(rect.h, lines * rect.h * 0.82) }}
                >
                  <FormulaText
                    ref={cellInput}
                    className="cell-editor"
                    font={font}
                    visible={editingHere}
                    value={editingHere ? session!.text : ""}
                    onChange={(text, caret) => {
                      const s = sessionRef.current;
                      if (s) setSession({ ...s, text, caret, point: null, origin: "cell" });
                      else if (text) {
                        // Typing (or IME composition) on a selected cell starts entry mode.
                        const ns = newSession(text, "enter", "cell", shownInput());
                        if (ns) setSession({ ...ns, caret });
                      }
                    }}
                    onCaret={(caret) => {
                      const s = sessionRef.current;
                      if (s && s.caret !== caret) setSession({ ...s, caret, point: s.point && s.point.end === caret ? s.point : null });
                    }}
                    onKeyDown={onCellKeyDown}
                    onCopy={(e) => {
                      if (!sessionRef.current) doCopy(false, e);
                    }}
                    onCut={(e) => {
                      if (sessionRef.current) return;
                      if (tabRef.current.kind === "sheet") doCopy(true, e);
                      else e.preventDefault();
                    }}
                    onPaste={(e) => {
                      if (sessionRef.current) return;
                      e.preventDefault();
                      void doPaste(e.clipboardData.getData("text/plain"));
                    }}
                  />
                  {session?.origin === "cell" && editingHere && acList}
                </div>
                );
              }}
              onCellDown={onCellDown}
              onCellDrag={onCellDrag}
              onCellUp={() => {
                drag.current = null;
              }}
              onDoubleClick={async (h) => {
                if (sessionRef.current) return;
                moveTo({ r: h.r, c: h.c });
                const input = await currentInput();
                startEdit(input, "edit", "cell", input);
              }}
              onContextMenu={onContextMenu}
              onAutoFill={
                onSheetTab
                  ? async (src, target) => {
                      const st = await run("채우기", () => api.autoFill(sheetIndex, src, target));
                      if (st) {
                        setSelection({ anchor: { r: target.r0, c: target.c0 }, focus: { r: target.r1, c: target.c1 } });
                        await afterChange(st);
                      }
                    }
                  : undefined
              }
              onColumnResize={(c, w, done) => {
                const rg = range();
                const [a, b] = rg.r0 === 1 && rg.r1 >= rowCount() && c >= rg.c0 && c <= rg.c1 ? [rg.c0, rg.c1] : [c, c];
                for (let x = a; x <= b; x++) source.cols.set(x, w);
                force((v) => v + 1);
                if (done && onSheetTab) void api.setColumnWidth(sheetIndex, a, b, Math.round(w)).then(() => refreshInfo());
              }}
              onRowResize={
                onSheetTab
                  ? (r, hpx, done) => {
                      const rg = range();
                      const [a, b] = rg.c0 === 1 && rg.c1 >= colCount() && r >= rg.r0 && r <= rg.r1 ? [rg.r0, rg.r1] : [r, r];
                      for (let x = a; x <= b; x++) source.rows.set(x, hpx);
                      force((v) => v + 1);
                      if (done) void api.setRowHeight(sheetIndex, a, b, Math.round(hpx)).then(() => refreshInfo());
                    }
                  : undefined
              }
              onAutoFitColumn={(c) => {
                const rg = range();
                if (rg.r0 === 1 && rg.r1 >= rowCount() && c >= rg.c0 && c <= rg.c1) void autofit(rg.c0, rg.c1);
                else void autofit(c, c);
              }}
              onZoom={setZoom}
              onPageSize={(n) => (pageRows.current = n)}
            />
          )}
          {findOpen && (
            <FindDialog
              message={findMsg}
              onFind={find}
              onClose={() => {
                setFindOpen(false);
                setFindMsg(null);
                focusGrid();
              }}
            />
          )}
        </div>
        {showSql && (
          <SqlPanel
            tables={info?.tables ?? []}
            onCreate={async (sql, name) => {
              const table = await run("쿼리 실행 중", () => api.queryToTable(sql, name));
              if (!table) return;
              await refreshInfo();
              sources.current.delete(tabKey({ kind: "data", name: table.name }));
              switchTab({ kind: "data", name: table.name });
              say(`${table.name}: ${formatCount(table.rows)}행`);
            }}
          />
        )}
      </main>

      {source instanceof DataSource && <DataBar source={source} onChange={() => force((x) => x + 1)} />}

      <footer className="sheetbar">
        <div className="tabs">
          {info?.sheets.map((name, i) => (
            <button
              key={`s${i}`}
              className={`tab ${tab.kind === "sheet" && tab.index === i ? "on" : ""}`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                const s = sessionRef.current;
                switchTab({ kind: "sheet", index: i }, !!s && pointable(s));
              }}
              onDoubleClick={() => renameSheet(i)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({
                  x: e.clientX,
                  y: e.clientY - 110,
                  items: [
                    { label: "삽입", onClick: insertSheet },
                    { label: "삭제", disabled: (info?.sheets.length ?? 1) <= 1, onClick: () => deleteSheet(i) },
                    { label: "이름 바꾸기", onClick: () => renameSheet(i) },
                  ],
                });
              }}
            >
              {name}
            </button>
          ))}
          <button className="tab add" onMouseDown={(e) => e.preventDefault()} onClick={insertSheet} title="새 시트 (Shift+F11)">
            ＋
          </button>
          {info?.tables.map((t) => (
            <button
              key={`d${t.name}`}
              className={`tab data ${tab.kind === "data" && tab.name === t.name ? "on" : ""}`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                const s = sessionRef.current;
                switchTab({ kind: "data", name: t.name }, !!s && pointable(s));
              }}
              title={`${t.name}: ${formatCount(t.rows)}행 · 수식에서 ${t.name}[열이름]`}
            >
              <span className="db">DB</span>
              {t.name}
            </button>
          ))}
        </div>
      </footer>

      <footer className="statusbar">
        <span className="mode">{mode}</span>
        {message && (
          <span className={message.error ? "msg err" : "msg"} onClick={() => setMessage(null)} title={message.text}>
            {message.text}
          </span>
        )}
        {onSheetTab && detail?.bridge_error && <span className="msg err">{detail.bridge_error}</span>}
        <span className="spacer" />
        {source instanceof DataSource && (
          <span>
            {formatCount(source.total)}행 · 조회 {source.lastQueryMs.toFixed(0)} ms
          </span>
        )}
        {stats && onSheetTab && stats.bridge_calls > 0 && <span title="데이터 엔진 계산 시간">데이터 엔진 {stats.bridge_ms.toFixed(0)} ms</span>}
        {selStats && selStats.count > 1 && (
          <>
            {selStats.average !== null && <span>평균: {formatNumber(selStats.average)}</span>}
            <span>개수: {formatCount(selStats.count)}</span>
            {selStats.numeric_count > 0 && <span>합계: {formatNumber(selStats.sum)}</span>}
          </>
        )}
        <span className="zoom">
          <button onClick={() => setZoom((z) => Math.max(0.1, Math.round((z - 0.1) * 10) / 10))}>−</button>
          <input type="range" min={10} max={400} value={Math.round(zoom * 100)} onChange={(e) => setZoom(Number(e.target.value) / 100)} />
          <button onClick={() => setZoom((z) => Math.min(4, Math.round((z + 0.1) * 10) / 10))}>＋</button>
          <span className="zoom-pct" onClick={() => setZoom(1)}>
            {Math.round(zoom * 100)}%
          </span>
        </span>
      </footer>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {dialog?.kind === "prompt" && (
        <PromptDialog
          title={dialog.title}
          label={dialog.label}
          value={dialog.value}
          onDone={(v) => {
            dialog.resolve(v);
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "format" && detail && (
        <FormatCellsDialog
          style={detail.style}
          onDone={(r) => {
            setDialog(null);
            focusGrid();
            if (r) void applyFormat(r);
          }}
        />
      )}
    </div>
  );
}

function DataBar(props: { source: DataSource; onChange(): void }) {
  const { source } = props;
  const defaultColumn = (source.info.columns.find((c) => c.kind === "Text") ?? source.info.columns[0])?.name ?? "";
  const [column, setColumn] = useState(defaultColumn);
  const [criterion, setCriterion] = useState("");
  useEffect(() => setColumn(defaultColumn), [source, defaultColumn]);
  const apply = (filters: typeof source.filters) => {
    source.setView(source.sort, filters);
    props.onChange();
  };
  return (
    <div className="data-bar">
      <span className="label">필터</span>
      <select value={column} onChange={(e) => setColumn(e.target.value)}>
        {source.info.columns.map((c) => (
          <option key={c.name}>{c.name}</option>
        ))}
      </select>
      <input
        placeholder="조건: 서울, >=100, *카드*, <>취소"
        value={criterion}
        onChange={(e) => setCriterion(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && criterion.trim()) {
            apply([...source.filters, { column, criterion: criterion.trim() }]);
            setCriterion("");
          }
        }}
      />
      {source.filters.map((f, i) => (
        <span key={i} className="chip">
          {f.column} {f.criterion}
          <button onClick={() => apply(source.filters.filter((_, j) => j !== i))}>×</button>
        </span>
      ))}
      <span className="spacer" />
      <span className="hint">
        수식에서 <code>{source.info.name}[열]</code> · 수식 입력 중 열을 클릭하면 참조가 들어갑니다
      </span>
    </div>
  );
}

function SqlPanel(props: { tables: TableInfo[]; onCreate(sql: string, name: string): Promise<void> }) {
  const first = props.tables[0];
  const groupCol = first?.columns.find((c) => c.kind === "Text") ?? first?.columns[0];
  const [sql, setSql] = useState(
    first ? `SELECT ${groupCol?.name ?? "*"}, COUNT(*) AS cnt\nFROM ${first.name}\nGROUP BY 1\nORDER BY 2 DESC` : "SELECT 1 AS x",
  );
  const [name, setName] = useState("result");
  return (
    <aside className="side">
      <h3>SQL</h3>
      <p className="muted">SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.</p>
      <textarea value={sql} onChange={(e) => setSql(e.target.value)} spellCheck={false} />
      <label>
        결과 시트 이름
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <button className="primary" onClick={() => props.onCreate(sql, name)}>
        실행
      </button>
      <h4>데이터 시트</h4>
      {props.tables.length === 0 && <p className="muted">아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.</p>}
      {props.tables.map((t) => (
        <details key={t.name}>
          <summary>
            {t.name} <span className="muted">{formatCount(t.rows)}행</span>
          </summary>
          <ul>
            {t.columns.map((c) => (
              <li key={c.name}>
                {c.name} <span className="muted">{c.sql_type}</span>
              </li>
            ))}
          </ul>
        </details>
      ))}
      <h4>시트에서 쓰는 법</h4>
      <ul className="muted help">
        <li>=SUMIFS(표[금액], 표[지역], "서울")</li>
        <li>=COUNTIFS(표[상태], "&lt;&gt;취소")</li>
        <li>=XLOOKUP(A2, 표[주문번호], 표[금액])</li>
        <li>=DQUERY("SELECT ... FROM 표")</li>
      </ul>
    </aside>
  );
}
