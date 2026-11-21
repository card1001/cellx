// Canvas-based virtual grid. Draws only what is visible, so it stays fast at any size.
// Scrolling happens on a transparent div laid over the canvas; the div also takes
// mouse, keyboard and clipboard events.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CellStyle } from "../api";
import {
  firstRowAt,
  normalize,
  parseTsv,
  type Pos,
  type Range,
  scrollHeightFor,
  scrollTopFor,
  startsEdit,
} from "./model";

export interface CellData {
  text: string;
  numeric?: boolean;
  error?: boolean;
  bridge?: boolean;
  style?: CellStyle;
}

export interface GridSource {
  rowCount(): number;
  colCount(): number;
  readonly rowHeight: number;
  colWidth(c: number): number;
  colHeader(c: number): string;
  /** Second header line (e.g. a column type); optional. */
  colSubHeader?(c: number): string;
  rowHeader(r: number): string;
  cell(r: number, c: number): CellData | undefined;
  /** Asks the source to load a range; it calls subscribers when data arrives. */
  request(r0: number, c0: number, r1: number, c1: number): void;
  /** Lets an open-ended sheet grow as the user scrolls towards its end. */
  grow?(r: number, c: number): void;
  subscribe(cb: () => void): () => void;
}

export interface Selection {
  anchor: Pos;
  focus: Pos;
}

interface Props {
  source: GridSource;
  selection: Selection;
  onSelection(sel: Selection): void;
  /** Text to put in the editor when editing starts without a typed key. */
  beginEdit(pos: Pos): Promise<string>;
  commitEdit(pos: Pos, text: string): Promise<void>;
  onEditText?(text: string | null): void;
  onDelete?(range: Range): void;
  copyText?(range: Range): string;
  onPaste?(at: Pos, rows: string[][]): void;
  onUndo?(): void;
  onRedo?(): void;
  onHeaderClick?(c: number): void;
  onColumnResize?(c: number, width: number): void;
  readOnlyCell?(pos: Pos): boolean;
}

const HEADER_H = 24;
const FONT = '13px "Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans KR", system-ui, sans-serif';
const HEADER_FONT = '12px "Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans KR", system-ui, sans-serif';
const ACCENT = "#4f46e5";

function cssVar(name: string, fallback: string) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

export function Grid(props: Props) {
  const { source, selection } = props;
  const wrapRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [scroll, setScroll] = useState({ top: 0, left: 0 });
  const [tick, setTick] = useState(0);
  const [editing, setEditingState] = useState<{ pos: Pos; text: string } | null>(null);
  // The ref guards against committing twice (Enter, then the blur that follows).
  const editingNow = useRef<{ pos: Pos; text: string } | null>(null);
  const setEditing = (v: { pos: Pos; text: string } | null) => {
    editingNow.current = v;
    setEditingState(v);
  };
  const dragging = useRef<false | "select" | { col: number; startX: number; startW: number }>(false);
  const editRef = useRef<HTMLTextAreaElement>(null);

  const rows = source.rowCount();
  const cols = source.colCount();
  const rh = source.rowHeight;
  const rowHeaderW = Math.max(44, String(rows).length * 8 + 18);
  const bodyH = Math.max(0, size.h - HEADER_H);
  const bodyW = Math.max(0, size.w - rowHeaderW);

  // Column geometry. Widths change rarely, so prefix sums are rebuilt on demand.
  const colX = useRef<number[]>([0]);
  const rebuildCols = useCallback(() => {
    const xs = new Array<number>(cols + 1);
    xs[0] = 0;
    for (let c = 1; c <= cols; c++) xs[c] = xs[c - 1] + source.colWidth(c);
    colX.current = xs;
  }, [cols, source]);
  rebuildCols();
  const totalW = colX.current[cols] ?? 0;
  const colLeft = (c: number) => colX.current[c - 1] ?? 0;
  const colAt = (x: number) => {
    // Binary search the first column whose right edge is past x.
    const xs = colX.current;
    let lo = 1;
    let hi = cols;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (xs[mid] > x) hi = mid;
      else lo = mid + 1;
    }
    return lo;
  };

  const firstRowF = firstRowAt(scroll.top, rows, rh, bodyH);
  const firstRow = Math.max(1, Math.floor(firstRowF) + 1);
  const rowOffset = (firstRowF - Math.floor(firstRowF)) * rh;
  const visibleRows = Math.ceil(bodyH / rh) + 1;
  const lastRow = Math.min(rows, firstRow + visibleRows);
  const firstCol = colAt(scroll.left);
  let lastCol = firstCol;
  while (lastCol < cols && colLeft(lastCol + 1) < scroll.left + bodyW) lastCol++;

  const rowY = (r: number) => HEADER_H + (r - firstRow) * rh - rowOffset;
  const colXScreen = (c: number) => rowHeaderW + colLeft(c) - scroll.left;

  useEffect(() => source.subscribe(() => setTick((t) => t + 1)), [source]);

  useEffect(() => {
    source.request(firstRow, firstCol, lastRow, lastCol);
    source.grow?.(lastRow, lastCol);
  }, [source, firstRow, firstCol, lastRow, lastCol]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  // ------------------------------------------------------------------ drawing
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.floor(size.w * dpr);
    canvas.height = Math.floor(size.h * dpr);
    canvas.style.width = `${size.w}px`;
    canvas.style.height = `${size.h}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const bg = cssVar("--grid-bg", "#ffffff");
    const line = cssVar("--grid-line", "#e3e5e8");
    const headerBg = cssVar("--grid-header", "#f4f5f7");
    const headerText = cssVar("--grid-header-text", "#555b66");
    const text = cssVar("--grid-text", "#1f2328");
    const sel = normalize(selection.anchor, selection.focus);

    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, size.w, size.h);
    ctx.font = FONT;
    ctx.textBaseline = "middle";

    // Cells
    ctx.save();
    ctx.beginPath();
    ctx.rect(rowHeaderW, HEADER_H, bodyW, bodyH);
    ctx.clip();
    // Grid lines
    ctx.strokeStyle = line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let r = firstRow; r <= lastRow + 1; r++) {
      const y = Math.round(rowY(r)) + 0.5;
      ctx.moveTo(rowHeaderW, y);
      ctx.lineTo(size.w, y);
    }
    for (let c = firstCol; c <= lastCol + 1; c++) {
      const x = Math.round(colXScreen(c)) + 0.5;
      ctx.moveTo(x, HEADER_H);
      ctx.lineTo(x, size.h);
    }
    ctx.stroke();

    for (let r = firstRow; r <= lastRow; r++) {
      const y = rowY(r);
      for (let c = firstCol; c <= lastCol; c++) {
        const cell = source.cell(r, c);
        if (!cell) continue;
        const x = colXScreen(c);
        const w = source.colWidth(c);
        const st = cell.style;
        if (st?.fill) {
          ctx.fillStyle = st.fill;
          ctx.fillRect(x, y, w, rh);
        }
        if (cell.text) {
          ctx.font = `${st?.italic ? "italic " : ""}${st?.bold ? "600 " : ""}${FONT}`;
          ctx.fillStyle = cell.error ? "#c62828" : st?.color ?? text;
          const align = st?.align ?? (cell.numeric || cell.error ? "right" : "left");
          // Like Excel, left-aligned text runs over empty cells to its right.
          let clipW = w;
          if (align === "left" && !cell.numeric) {
            const need = ctx.measureText(cell.text).width + 12;
            let k = c + 1;
            while (clipW < need && k <= lastCol + 20 && k <= cols && !source.cell(r, k)?.text) {
              clipW += source.colWidth(k);
              k++;
            }
          }
          if (clipW > w) {
            // Hide the grid lines under the overflowing part, as Excel does.
            ctx.fillStyle = bg;
            ctx.fillRect(x + w - 1, y + 1, clipW - w, rh - 1);
            ctx.fillStyle = cell.error ? "#c62828" : st?.color ?? text;
          }
          ctx.save();
          ctx.beginPath();
          ctx.rect(x + 1, y, clipW - 2, rh);
          ctx.clip();
          let tx = x + 6;
          if (align === "right") {
            ctx.textAlign = "right";
            tx = x + w - 6;
          } else if (align === "center") {
            ctx.textAlign = "center";
            tx = x + w / 2;
          } else {
            ctx.textAlign = "left";
          }
          ctx.fillText(cell.text, tx, y + rh / 2 + 1);
          if (st?.underline) {
            const m = ctx.measureText(cell.text).width;
            const ux = align === "right" ? tx - m : align === "center" ? tx - m / 2 : tx;
            ctx.fillRect(ux, y + rh - 6, m, 1);
          }
          ctx.restore();
        }
        if (cell.bridge) {
          // A small corner mark: this cell is computed by the data engine.
          ctx.fillStyle = ACCENT;
          ctx.beginPath();
          ctx.moveTo(x, y);
          ctx.lineTo(x + 6, y);
          ctx.lineTo(x, y + 6);
          ctx.closePath();
          ctx.fill();
        }
      }
    }
    // Selection
    const sx0 = colXScreen(sel.c0);
    const sx1 = colXScreen(sel.c1) + source.colWidth(sel.c1);
    const sy0 = rowY(sel.r0);
    const sy1 = rowY(sel.r1) + rh;
    if (sel.r0 !== sel.r1 || sel.c0 !== sel.c1) {
      ctx.fillStyle = "rgba(79, 70, 229, 0.08)";
      ctx.fillRect(sx0, sy0, sx1 - sx0, sy1 - sy0);
    }
    ctx.strokeStyle = ACCENT;
    ctx.lineWidth = 2;
    ctx.strokeRect(sx0 + 1, sy0 + 1, sx1 - sx0 - 1, sy1 - sy0 - 1);
    ctx.restore();

    // Column headers
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, 0, size.w, HEADER_H);
    ctx.font = HEADER_FONT;
    ctx.textAlign = "center";
    for (let c = firstCol; c <= lastCol; c++) {
      const x = colXScreen(c);
      const w = source.colWidth(c);
      const inSel = c >= sel.c0 && c <= sel.c1;
      if (inSel) {
        ctx.fillStyle = "rgba(79, 70, 229, 0.14)";
        ctx.fillRect(x, 0, w, HEADER_H);
      }
      ctx.save();
      ctx.beginPath();
      ctx.rect(Math.max(x, rowHeaderW), 0, w, HEADER_H);
      ctx.clip();
      ctx.fillStyle = inSel ? ACCENT : headerText;
      const sub = source.colSubHeader?.(c);
      const label = source.colHeader(c);
      if (sub) {
        ctx.textAlign = "left";
        ctx.fillText(label, x + 6, HEADER_H / 2);
        const lw = ctx.measureText(label).width;
        ctx.fillStyle = "#9aa0a6";
        ctx.fillText(sub, x + 12 + lw, HEADER_H / 2);
        ctx.textAlign = "center";
      } else {
        ctx.fillText(label, x + w / 2, HEADER_H / 2);
      }
      ctx.restore();
    }
    // Row headers
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, HEADER_H, rowHeaderW, size.h);
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, HEADER_H, rowHeaderW, bodyH);
    ctx.clip();
    for (let r = firstRow; r <= lastRow; r++) {
      const y = rowY(r);
      const inSel = r >= sel.r0 && r <= sel.r1;
      if (inSel) {
        ctx.fillStyle = "rgba(79, 70, 229, 0.14)";
        ctx.fillRect(0, y, rowHeaderW, rh);
      }
      ctx.fillStyle = inSel ? ACCENT : headerText;
      ctx.fillText(source.rowHeader(r), rowHeaderW / 2, y + rh / 2 + 1);
    }
    ctx.restore();
    // Header borders
    ctx.strokeStyle = line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, HEADER_H + 0.5);
    ctx.lineTo(size.w, HEADER_H + 0.5);
    ctx.moveTo(rowHeaderW + 0.5, 0);
    ctx.lineTo(rowHeaderW + 0.5, size.h);
    for (let c = firstCol; c <= lastCol + 1; c++) {
      const x = Math.round(colXScreen(c)) + 0.5;
      if (x < rowHeaderW) continue;
      ctx.moveTo(x, 0);
      ctx.lineTo(x, HEADER_H);
    }
    for (let r = firstRow; r <= lastRow + 1; r++) {
      const y = Math.round(rowY(r)) + 0.5;
      if (y < HEADER_H) continue;
      ctx.moveTo(0, y);
      ctx.lineTo(rowHeaderW, y);
    }
    ctx.stroke();
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, 0, rowHeaderW, HEADER_H);
  });

  // ------------------------------------------------------------------ navigation
  const setScrollFor = useCallback(
    (top: number, left: number) => {
      const el = scrollRef.current;
      if (!el) return;
      el.scrollTop = top;
      el.scrollLeft = left;
      setScroll({ top: el.scrollTop, left: el.scrollLeft });
    },
    [],
  );

  const ensureVisible = useCallback(
    (p: Pos) => {
      let top = scroll.top;
      let left = scroll.left;
      const fullRows = Math.max(1, Math.floor(bodyH / rh) - 1);
      if (p.r < firstRow + (rowOffset > 0 ? 1 : 0)) top = scrollTopFor(p.r - 1, rows, rh, bodyH);
      else if (p.r > firstRow + fullRows - 1) top = scrollTopFor(p.r - fullRows, rows, rh, bodyH);
      const x0 = colLeft(p.c);
      const x1 = x0 + source.colWidth(p.c);
      if (x0 < left) left = x0;
      else if (x1 > left + bodyW) left = x1 - bodyW;
      if (top !== scroll.top || left !== scroll.left) setScrollFor(top, left);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [scroll, bodyH, bodyW, rows, rh, firstRow, rowOffset, source],
  );

  const select = useCallback(
    (focus: Pos, extend: boolean) => {
      const f = {
        r: Math.min(Math.max(1, focus.r), rows),
        c: Math.min(Math.max(1, focus.c), cols),
      };
      props.onSelection({ anchor: extend ? selection.anchor : f, focus: f });
      ensureVisible(f);
    },
    [props, rows, cols, selection.anchor, ensureVisible],
  );

  const hit = (clientX: number, clientY: number) => {
    const rect = scrollRef.current!.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    const inColHeader = y < HEADER_H;
    const inRowHeader = x < rowHeaderW;
    const c = colAt(scroll.left + Math.max(0, x - rowHeaderW));
    const r = Math.min(rows, Math.max(1, firstRow + Math.floor((y - HEADER_H + rowOffset) / rh)));
    return { x, y, r, c, inColHeader, inRowHeader };
  };

  const startEdit = async (pos: Pos, typed?: string) => {
    if (props.readOnlyCell?.(pos)) return;
    const text = typed ?? (await props.beginEdit(pos));
    setEditing({ pos, text });
    props.onEditText?.(text);
    requestAnimationFrame(() => {
      const el = editRef.current;
      if (el) {
        el.focus();
        el.setSelectionRange(el.value.length, el.value.length);
      }
    });
  };

  const finishEdit = async (commit: boolean, move: "down" | "up" | "right" | "left" | "none") => {
    const e = editingNow.current;
    if (!e) return;
    setEditing(null);
    props.onEditText?.(null);
    scrollRef.current?.focus();
    // Move first so a slow recalculation does not hold the cursor back.
    if (move !== "none") {
      const d = { down: [1, 0], up: [-1, 0], right: [0, 1], left: [0, -1] }[move];
      select({ r: e.pos.r + d[0], c: e.pos.c + d[1] }, false);
    }
    if (commit) await props.commitEdit(e.pos, e.text);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (editing) return;
    const f = selection.focus;
    const mod = e.ctrlKey || e.metaKey;
    const page = Math.max(1, Math.floor(bodyH / rh) - 1);
    const moves: Record<string, Pos> = {
      ArrowDown: { r: mod ? rows : f.r + 1, c: f.c },
      ArrowUp: { r: mod ? 1 : f.r - 1, c: f.c },
      ArrowRight: { r: f.r, c: mod ? cols : f.c + 1 },
      ArrowLeft: { r: f.r, c: mod ? 1 : f.c - 1 },
      PageDown: { r: f.r + page, c: f.c },
      PageUp: { r: f.r - page, c: f.c },
      Home: { r: mod ? 1 : f.r, c: 1 },
    };
    if (moves[e.key]) {
      e.preventDefault();
      select(moves[e.key], e.shiftKey);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      select({ r: f.r + (e.shiftKey ? -1 : 1), c: f.c }, false);
    } else if (e.key === "Tab") {
      e.preventDefault();
      select({ r: f.r, c: f.c + (e.shiftKey ? -1 : 1) }, false);
    } else if (e.key === "F2") {
      e.preventDefault();
      void startEdit(f);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      props.onDelete?.(normalize(selection.anchor, selection.focus));
    } else if (mod && (e.key === "z" || e.key === "Z")) {
      e.preventDefault();
      if (e.shiftKey) props.onRedo?.();
      else props.onUndo?.();
    } else if (mod && (e.key === "y" || e.key === "Y")) {
      e.preventDefault();
      props.onRedo?.();
    } else if (mod && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      props.onSelection({ anchor: { r: 1, c: 1 }, focus: { r: rows, c: cols } });
    } else if (startsEdit(e)) {
      e.preventDefault();
      void startEdit(f, e.key);
    }
  };

  // Clipboard: listen on the document while the grid has focus.
  useEffect(() => {
    const onCopy = (e: ClipboardEvent) => {
      if (document.activeElement !== scrollRef.current || !props.copyText) return;
      e.clipboardData?.setData("text/plain", props.copyText(normalize(selection.anchor, selection.focus)));
      e.preventDefault();
    };
    const onPaste = (e: ClipboardEvent) => {
      if (document.activeElement !== scrollRef.current || !props.onPaste) return;
      const text = e.clipboardData?.getData("text/plain") ?? "";
      if (!text) return;
      e.preventDefault();
      const s = normalize(selection.anchor, selection.focus);
      props.onPaste({ r: s.r0, c: s.c0 }, parseTsv(text));
    };
    document.addEventListener("copy", onCopy);
    document.addEventListener("paste", onPaste);
    return () => {
      document.removeEventListener("copy", onCopy);
      document.removeEventListener("paste", onPaste);
    };
  }, [props, selection]);

  const resizeTarget = (x: number) => {
    for (let c = firstCol; c <= lastCol; c++) {
      const right = colXScreen(c) + source.colWidth(c);
      if (Math.abs(x - right) <= 4) return c;
    }
    return null;
  };

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const h = hit(e.clientX, e.clientY);
    if (editing) void finishEdit(true, "none");
    scrollRef.current?.focus();
    if (h.inColHeader && !h.inRowHeader) {
      const rc = props.onColumnResize ? resizeTarget(h.x) : null;
      if (rc !== null) {
        dragging.current = { col: rc, startX: e.clientX, startW: source.colWidth(rc) };
        return;
      }
      if (props.onHeaderClick) props.onHeaderClick(h.c);
      else props.onSelection({ anchor: { r: 1, c: h.c }, focus: { r: rows, c: h.c } });
      return;
    }
    if (h.inRowHeader && !h.inColHeader) {
      props.onSelection({ anchor: { r: h.r, c: 1 }, focus: { r: h.r, c: cols } });
      return;
    }
    if (h.inColHeader || h.inRowHeader) return;
    dragging.current = "select";
    select({ r: h.r, c: h.c }, e.shiftKey);
  };

  const onMouseMove = (e: React.MouseEvent) => {
    const d = dragging.current;
    const el = scrollRef.current;
    if (!d) {
      if (el) {
        const h = hit(e.clientX, e.clientY);
        el.style.cursor =
          props.onColumnResize && h.inColHeader && resizeTarget(h.x) !== null ? "col-resize" : "cell";
      }
      return;
    }
    if (d === "select") {
      const h = hit(e.clientX, e.clientY);
      props.onSelection({ anchor: selection.anchor, focus: { r: h.r, c: h.c } });
    } else {
      const w = Math.max(24, d.startW + e.clientX - d.startX);
      props.onColumnResize?.(d.col, w);
    }
  };

  useEffect(() => {
    const up = () => (dragging.current = false);
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  const onWheel = (e: React.WheelEvent) => {
    // Scroll by whole rows so compressed (very tall) grids still move predictably.
    if (Math.abs(e.deltaY) < Math.abs(e.deltaX)) return;
    e.preventDefault();
    const deltaRows = Math.sign(e.deltaY) * Math.max(1, Math.round(Math.abs(e.deltaY) / rh));
    const target = Math.min(Math.max(0, firstRowF + deltaRows), Math.max(0, rows - bodyH / rh));
    setScrollFor(scrollTopFor(target, rows, rh, bodyH), scroll.left);
  };

  // React attaches wheel listeners as passive; attach our own to be able to preventDefault.
  const wheelRef = useRef(onWheel);
  wheelRef.current = onWheel;
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const h = (e: WheelEvent) => wheelRef.current(e as unknown as React.WheelEvent);
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  }, []);

  const editorStyle = editing
    ? {
        left: colXScreen(editing.pos.c),
        top: rowY(editing.pos.r),
        minWidth: source.colWidth(editing.pos.c),
        height: rh,
      }
    : undefined;

  void tick;
  return (
    <div className="grid-wrap" ref={wrapRef}>
      <canvas ref={canvasRef} className="grid-canvas" />
      <div
        ref={scrollRef}
        className="grid-scroll"
        tabIndex={0}
        onScroll={(e) => setScroll({ top: e.currentTarget.scrollTop, left: e.currentTarget.scrollLeft })}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onDoubleClick={(e) => {
          const h = hit(e.clientX, e.clientY);
          if (!h.inColHeader && !h.inRowHeader) void startEdit({ r: h.r, c: h.c });
        }}
        onKeyDown={onKeyDown}
      >
        <div
          style={{
            width: totalW + rowHeaderW,
            height: scrollHeightFor(rows, rh) + HEADER_H,
          }}
        />
      </div>
      {editing && (
        <textarea
          ref={editRef}
          className="grid-editor"
          style={editorStyle}
          value={editing.text}
          spellCheck={false}
          onChange={(e) => {
            setEditing({ ...editing, text: e.target.value });
            props.onEditText?.(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.altKey) {
              e.preventDefault();
              void finishEdit(true, e.shiftKey ? "up" : "down");
            } else if (e.key === "Tab") {
              e.preventDefault();
              void finishEdit(true, e.shiftKey ? "left" : "right");
            } else if (e.key === "Escape") {
              e.preventDefault();
              void finishEdit(false, "none");
            }
          }}
          onBlur={() => void finishEdit(true, "none")}
        />
      )}
    </div>
  );
}
