// Canvas grid with Excel's look and mouse behaviour. It draws only what is visible
// (any sheet size), and supports frozen panes, merged cells, borders, fonts, wrapping,
// zoom, the fill handle, and the coloured reference boxes of the formula being edited.
//
// The grid owns scrolling and the in-progress mouse gestures that need pixel state
// (column resize, fill-handle drag). Selection and editing live in the app, which
// receives the gestures through callbacks.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { CellStyle } from "../api";
import { REF_COLORS } from "../formula";
import { MAX_SCROLL_PX, normalize, type Pos, type Range } from "./model";
import type { CellData, GridSource, Merge } from "./sources";

export interface Selection {
  anchor: Pos;
  focus: Pos;
}

export interface RefBox {
  range: Range;
  color: number;
}

export type Hit = {
  area: "corner" | "colHeader" | "rowHeader" | "cell";
  r: number;
  c: number;
};

export interface CellRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Props {
  source: GridSource;
  selection: Selection;
  zoom: number;
  /** Cell to keep in view (active cell or the moving point-mode reference). */
  scrollTarget: Pos;
  refBoxes?: RefBox[];
  copyRange?: Range | null;
  /** Renders the cell editor / key catcher over this cell. */
  editorAt: Pos;
  renderEditor(rect: CellRect, font: string): ReactNode;
  onCellDown(hit: Hit, e: React.MouseEvent): void;
  onCellDrag(hit: Hit): void;
  onCellUp(): void;
  onDoubleClick(hit: Hit): void;
  onContextMenu(hit: Hit, x: number, y: number): void;
  onAutoFill?(source: Range, target: Range): void;
  onColumnResize?(c: number, width: number, done: boolean): void;
  onRowResize?(r: number, height: number, done: boolean): void;
  onAutoFitColumn?(c: number): void;
  onZoom?(z: number): void;
  /** Rows that fit on screen, for PageUp/PageDown. */
  onPageSize?(rows: number): void;
}

const ACCENT = "#4f46e5";
export const UI_FONT = '"맑은 고딕", "Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans KR", sans-serif';

export function fontCss(style: CellStyle | undefined, base: [string, number], zoom: number): string {
  const family = style?.font ?? base[0];
  const pt = style?.size ?? base[1];
  const px = ((pt * 4) / 3) * zoom;
  return `${style?.italic ? "italic " : ""}${style?.bold ? "bold " : ""}${px.toFixed(2)}px "${family}", ${UI_FONT}`;
}

function borderWidth(style: string): number {
  if (style.startsWith("medium")) return 2;
  if (style === "thick" || style === "double") return 3;
  return 1;
}

function wrapLines(ctx: CanvasRenderingContext2D, text: string, width: number): string[] {
  const out: string[] = [];
  for (const para of text.split("\n")) {
    let line = "";
    for (const ch of para) {
      if (line && ctx.measureText(line + ch).width > width) {
        // Prefer breaking at the last space, like Excel.
        const sp = line.lastIndexOf(" ");
        if (sp > 0 && ch !== " ") {
          out.push(line.slice(0, sp));
          line = line.slice(sp + 1) + ch;
        } else {
          out.push(line);
          line = ch === " " ? "" : ch;
        }
      } else line += ch;
    }
    out.push(line);
  }
  return out;
}

export function Grid(props: Props) {
  const { source, selection, zoom: z } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [scroll, setScroll] = useState({ top: 0, left: 0 });
  const [, setTick] = useState(0);
  const [fillTarget, setFillTarget] = useState<Range | null>(null);
  const [dash, setDash] = useState(0);
  const gesture = useRef<
    | null
    | { kind: "select" }
    | { kind: "fill"; source: Range }
    | { kind: "colResize"; c: number; startX: number; startW: number }
    | { kind: "rowResize"; r: number; startY: number; startH: number }
  >(null);

  const rows = source.rows;
  const cols = source.cols;
  const fr = Math.min(source.frozenRows, rows.count);
  const fc = Math.min(source.frozenCols, cols.count);
  const hz = Math.max(0.75, Math.min(z, 1.5));
  const HEADER_H = Math.round(20 * hz);
  const rowHeaderW = Math.round(Math.max(34, String(rows.count).length * 7 + 14) * hz);

  // Unzoomed geometry of frozen panes and the scrolling region.
  const frozenH = rows.offset(fr + 1);
  const frozenW = cols.offset(fc + 1);
  const scrollTotalH = rows.total() - frozenH;
  const scrollTotalW = cols.total() - frozenW;
  const viewH = Math.max(0, size.h - HEADER_H - frozenH * z);
  const viewW = Math.max(0, size.w - rowHeaderW - frozenW * z);
  const bodyH = viewH / z;
  const bodyW = viewW / z;

  // Very tall sheets are compressed so the scroll area stays within browser limits.
  const scrollAreaH = Math.min(scrollTotalH * z, MAX_SCROLL_PX);
  const yRatio =
    scrollTotalH * z > MAX_SCROLL_PX && scrollAreaH > viewH ? (scrollTotalH * z - viewH) / (scrollAreaH - viewH) : 1;
  const scrollY = (scroll.top * yRatio) / z;
  const scrollX = scroll.left / z;
  const topForScrollY = (y: number) => (y * z) / yRatio;

  const rowY = useCallback(
    (r: number) => (r <= fr ? HEADER_H + rows.offset(r) * z : HEADER_H + (rows.offset(r) - scrollY) * z),
    [fr, HEADER_H, rows, z, scrollY],
  );
  const colX = useCallback(
    (c: number) => (c <= fc ? rowHeaderW + cols.offset(c) * z : rowHeaderW + (cols.offset(c) - scrollX) * z),
    [fc, rowHeaderW, cols, z, scrollX],
  );

  const firstRow = Math.max(fr + 1, rows.indexAt(frozenH + scrollY));
  const lastRow = Math.min(rows.count, rows.indexAt(frozenH + scrollY + bodyH) + 1);
  const firstCol = Math.max(fc + 1, cols.indexAt(frozenW + scrollX));
  const lastCol = Math.min(cols.count, cols.indexAt(frozenW + scrollX + bodyW) + 1);

  useEffect(() => source.subscribe(() => setTick((t) => t + 1)), [source]);

  useEffect(() => {
    if (fr) source.request(1, fc ? 1 : firstCol, fr, lastCol);
    if (fc) source.request(firstRow, 1, lastRow, fc);
    source.request(firstRow, firstCol, lastRow, lastCol);
    source.grow?.(lastRow, lastCol);
  }, [source, firstRow, firstCol, lastRow, lastCol, fr, fc]);

  const pageSize = Math.max(1, Math.floor(bodyH / rows.defaultSize) - 1);
  const onPageSize = props.onPageSize;
  useEffect(() => {
    onPageSize?.(pageSize);
  }, [pageSize, onPageSize]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  // Marching ants around the copied range.
  useEffect(() => {
    if (!props.copyRange) return;
    const id = setInterval(() => setDash((d) => (d + 1) % 8), 120);
    return () => clearInterval(id);
  }, [props.copyRange]);

  const setScrollTo = useCallback((top: number, left: number) => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = top;
    el.scrollLeft = left;
    setScroll({ top: el.scrollTop, left: el.scrollLeft });
  }, []);

  // Keep the target cell visible.
  const target = props.scrollTarget;
  useEffect(() => {
    let y = scrollY;
    let x = scrollX;
    if (target.r > fr) {
      const top = rows.offset(target.r) - frozenH;
      const bottom = top + rows.size(target.r);
      if (top < y) y = top;
      else if (bottom > y + bodyH) y = bottom - bodyH;
    }
    if (target.c > fc) {
      const left = cols.offset(target.c) - frozenW;
      const right = left + cols.size(target.c);
      if (left < x) x = left;
      else if (right > x + bodyW) x = right - bodyW;
    }
    if (Math.abs(y - scrollY) > 0.5 || Math.abs(x - scrollX) > 0.5)
      setScrollTo(topForScrollY(Math.max(0, y)), Math.max(0, x) * z);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target.r, target.c]);

  // ------------------------------------------------------------------ geometry helpers
  const cellRect = useCallback(
    (r: number, c: number, m?: Merge): CellRect => {
      const r1 = m ? m.r + m.h - 1 : r;
      const c1 = m ? m.c + m.w - 1 : c;
      const x = colX(m ? m.c : c);
      const y = rowY(m ? m.r : r);
      return { x, y, w: colX(c1) + cols.size(c1) * z - x, h: rowY(r1) + rows.size(r1) * z - y };
    },
    [colX, rowY, cols, rows, z],
  );

  const rangeRect = (rg: Range): CellRect => {
    const a = cellRect(rg.r0, rg.c0);
    const b = cellRect(rg.r1, rg.c1);
    return { x: a.x, y: a.y, w: b.x + b.w - a.x, h: b.y + b.h - a.y };
  };

  /** A selection grown to cover any merged cell it touches (Excel behaviour). */
  const expandForMerges = (rg: Range): Range => {
    const out = { ...rg };
    if ((out.r1 - out.r0) * (out.c1 - out.c0) > 40_000) return out;
    for (let pass = 0; pass < 3; pass++) {
      for (let r = out.r0; r <= out.r1; r++) {
        for (const c of [out.c0, out.c1]) {
          const m = source.mergeAt(r, c);
          if (m) {
            out.r0 = Math.min(out.r0, m.r);
            out.c0 = Math.min(out.c0, m.c);
            out.r1 = Math.max(out.r1, m.r + m.h - 1);
            out.c1 = Math.max(out.c1, m.c + m.w - 1);
          }
        }
      }
      for (let c = out.c0; c <= out.c1; c++) {
        for (const r of [out.r0, out.r1]) {
          const m = source.mergeAt(r, c);
          if (m) {
            out.r0 = Math.min(out.r0, m.r);
            out.c0 = Math.min(out.c0, m.c);
            out.r1 = Math.max(out.r1, m.r + m.h - 1);
            out.c1 = Math.max(out.c1, m.c + m.w - 1);
          }
        }
      }
    }
    return out;
  };

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

    const bg = "#ffffff";
    const gridLine = "#d4d4d4";
    const headerBg = "#f3f3f3";
    const headerSel = "#e1e1e9";
    const headerText = "#444444";
    const textColor = "#000000";
    const sel = expandForMerges(normalize(selection.anchor, selection.focus));
    const baseFont = source.font;

    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, size.w, size.h);

    const list = (a: number, b: number) => {
      const out: number[] = [];
      for (let i = a; i <= b; i++) out.push(i);
      return out;
    };
    const scrollRows = list(firstRow, lastRow);
    const scrollCols = list(firstCol, lastCol);
    const frozenRows = list(1, fr);
    const frozenCols = list(1, fc);

    const bodyX0 = rowHeaderW + frozenW * z;
    const bodyY0 = HEADER_H + frozenH * z;
    const regions: { rs: number[]; cs: number[]; clip: [number, number, number, number] }[] = [];
    if (fr && fc) regions.push({ rs: frozenRows, cs: frozenCols, clip: [rowHeaderW, HEADER_H, frozenW * z, frozenH * z] });
    if (fr) regions.push({ rs: frozenRows, cs: scrollCols, clip: [bodyX0, HEADER_H, size.w - bodyX0, frozenH * z] });
    if (fc) regions.push({ rs: scrollRows, cs: frozenCols, clip: [rowHeaderW, bodyY0, frozenW * z, size.h - bodyY0] });
    regions.push({ rs: scrollRows, cs: scrollCols, clip: [bodyX0, bodyY0, size.w - bodyX0, size.h - bodyY0] });

    const isEmpty = (r: number, c: number) => !source.cell(r, c)?.text && !source.mergeAt(r, c);

    for (const reg of regions) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(...reg.clip);
      ctx.clip();

      if (source.showGrid) {
        ctx.strokeStyle = gridLine;
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (const r of reg.rs) {
          if (rows.size(r) === 0) continue;
          const y = Math.round(rowY(r) + rows.size(r) * z) - 0.5;
          ctx.moveTo(reg.clip[0], y);
          ctx.lineTo(reg.clip[0] + reg.clip[2], y);
        }
        for (const c of reg.cs) {
          if (cols.size(c) === 0) continue;
          const x = Math.round(colX(c) + cols.size(c) * z) - 0.5;
          ctx.moveTo(x, reg.clip[1]);
          ctx.lineTo(x, reg.clip[1] + reg.clip[3]);
        }
        ctx.stroke();
      }

      // Fills and merged-cell backgrounds hide grid lines, as in Excel.
      const seenMerges = new Set<Merge>();
      const items: { r: number; c: number; rect: CellRect; cell: CellData | undefined; merge?: Merge }[] = [];
      for (const r of reg.rs) {
        if (rows.size(r) === 0) continue;
        for (const c of reg.cs) {
          if (cols.size(c) === 0) continue;
          const m = source.mergeAt(r, c);
          if (m) {
            if (seenMerges.has(m)) continue;
            seenMerges.add(m);
            const rect = cellRect(m.r, m.c, m);
            const cell = source.cell(m.r, m.c);
            ctx.fillStyle = cell?.style?.fill ?? bg;
            ctx.fillRect(rect.x, rect.y, rect.w - 1, rect.h - 1);
            items.push({ r: m.r, c: m.c, rect, cell, merge: m });
            continue;
          }
          const cell = source.cell(r, c);
          const rect = cellRect(r, c);
          if (cell?.style?.fill) {
            ctx.fillStyle = cell.style.fill;
            ctx.fillRect(rect.x - 0.5, rect.y - 0.5, rect.w, rect.h);
          }
          if (cell) items.push({ r, c, rect, cell });
        }
      }

      // Text.
      ctx.textBaseline = "alphabetic";
      for (const it of items) {
        const cell = it.cell;
        if (!cell?.text) continue;
        const st = cell.style;
        ctx.font = fontCss(st, baseFont, z);
        const fontPx = (((st?.size ?? baseFont[1]) * 4) / 3) * z;
        const pad = 3 * z;
        const { rect } = it;
        let align = st?.align ?? (cell.numeric ? "right" : cell.error ? "center" : "left");
        let text = cell.text;
        let clipX = rect.x;
        let clipW = rect.w;
        const measured = ctx.measureText(text).width;
        if (cell.numeric && !st?.wrap && !it.merge && measured > rect.w - 2 * pad) {
          // Numbers never spill: Excel shows #### when they do not fit.
          const hw = ctx.measureText("#").width || 1;
          text = "#".repeat(Math.max(1, Math.floor((rect.w - 2 * pad) / hw)));
          align = "center";
        } else if (!st?.wrap && !it.merge && measured > rect.w - 2 * pad) {
          // Text spills into empty neighbours.
          if (align === "left" || align === "center") {
            let c = it.c + 1;
            while (clipW < measured + 2 * pad && c <= cols.count && isEmpty(it.r, c) && c - it.c < 40) {
              clipW += cols.size(c) * z;
              c++;
            }
          }
          if (align === "right" || align === "center") {
            let c = it.c - 1;
            while (clipW < measured + 2 * pad && c >= 1 && isEmpty(it.r, c) && it.c - c < 40) {
              clipX -= cols.size(c) * z;
              clipW += cols.size(c) * z;
              c--;
            }
          }
          if (clipW > rect.w) {
            ctx.fillStyle = bg;
            ctx.fillRect(clipX, rect.y, clipW - 1, rect.h - 1);
            if (st?.fill) {
              ctx.fillStyle = st.fill;
              ctx.fillRect(rect.x, rect.y, rect.w - 1, rect.h - 1);
            }
          }
        }
        ctx.save();
        ctx.beginPath();
        ctx.rect(clipX, rect.y, clipW, rect.h);
        ctx.clip();
        ctx.fillStyle = st?.color ?? textColor;
        const lines = st?.wrap ? wrapLines(ctx, text, rect.w - 2 * pad) : [text];
        const lineH = fontPx * 1.25;
        const blockH = lines.length * lineH;
        const valign = st?.valign ?? "bottom";
        let y0 =
          valign === "top"
            ? rect.y + pad + fontPx * 0.95
            : valign === "center"
              ? rect.y + (rect.h - blockH) / 2 + fontPx * 0.95
              : rect.y + rect.h - blockH + fontPx * 0.95 - pad * 0.6;
        for (const line of lines) {
          let tx: number;
          if (align === "right") {
            ctx.textAlign = "right";
            tx = it.merge || clipW <= rect.w ? rect.x + rect.w - pad : rect.x + rect.w - pad;
          } else if (align === "center") {
            ctx.textAlign = "center";
            tx = rect.x + rect.w / 2;
          } else {
            ctx.textAlign = "left";
            tx = rect.x + pad;
          }
          ctx.fillText(line, tx, y0);
          if (st?.underline || st?.strike) {
            const w = ctx.measureText(line).width;
            const lx = align === "right" ? tx - w : align === "center" ? tx - w / 2 : tx;
            ctx.fillRect(lx, st.underline ? y0 + 1.5 * z : y0 - fontPx * 0.32, w, Math.max(1, z));
          }
          y0 += lineH;
        }
        ctx.restore();
        if (cell.bridge) {
          // A small mark: this value is computed by the data engine.
          ctx.fillStyle = ACCENT;
          ctx.beginPath();
          ctx.moveTo(rect.x + rect.w - 1, rect.y);
          ctx.lineTo(rect.x + rect.w - 1 - 5 * z, rect.y);
          ctx.lineTo(rect.x + rect.w - 1, rect.y + 5 * z);
          ctx.closePath();
          ctx.fill();
        }
      }

      // Borders.
      for (const it of items) {
        const st = it.cell?.style;
        if (!st) continue;
        const { x, y, w, h } = it.rect;
        const side = (b: { style: string; color: string } | undefined, x0: number, y0: number, x1: number, y1: number) => {
          if (!b) return;
          const bw = borderWidth(b.style);
          ctx.strokeStyle = b.color;
          ctx.lineWidth = bw;
          ctx.setLineDash(b.style === "dotted" ? [1, 2] : b.style.includes("dash") ? [4, 2] : []);
          const off = bw % 2 ? 0.5 : 0;
          ctx.beginPath();
          ctx.moveTo(Math.round(x0) - off, Math.round(y0) - off);
          ctx.lineTo(Math.round(x1) - off, Math.round(y1) - off);
          ctx.stroke();
          if (b.style === "double") {
            ctx.strokeStyle = bg;
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(Math.round(x0) - 0.5, Math.round(y0) - 0.5);
            ctx.lineTo(Math.round(x1) - 0.5, Math.round(y1) - 0.5);
            ctx.stroke();
          }
          ctx.setLineDash([]);
        };
        side(st.border_top, x, y, x + w, y);
        side(st.border_bottom, x, y + h, x + w, y + h);
        side(st.border_left, x, y, x, y + h);
        side(st.border_right, x + w, y, x + w, y + h);
      }
      ctx.restore();
    }

    // Overlays, clipped to the cell area.
    ctx.save();
    ctx.beginPath();
    ctx.rect(rowHeaderW, HEADER_H, size.w - rowHeaderW, size.h - HEADER_H);
    ctx.clip();

    for (const box of props.refBoxes ?? []) {
      const rr = rangeRect(box.range);
      const color = REF_COLORS[box.color % REF_COLORS.length];
      ctx.fillStyle = color + "1a";
      ctx.fillRect(rr.x, rr.y, rr.w, rr.h);
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.strokeRect(rr.x + 1, rr.y + 1, rr.w - 2, rr.h - 2);
      ctx.fillStyle = color;
      for (const [cx, cy] of [
        [rr.x, rr.y],
        [rr.x + rr.w, rr.y],
        [rr.x, rr.y + rr.h],
        [rr.x + rr.w, rr.y + rr.h],
      ]) {
        ctx.fillRect(cx - 2.5, cy - 2.5, 5, 5);
      }
    }

    // Selection: tinted range with the active cell left clear, thick outline, fill handle.
    const sr = rangeRect(sel);
    const activeMerge = source.mergeAt(selection.focus.r, selection.focus.c);
    const ar = cellRect(selection.focus.r, selection.focus.c, activeMerge);
    if (sel.r0 !== sel.r1 || sel.c0 !== sel.c1) {
      ctx.fillStyle = "rgba(79, 70, 229, 0.12)";
      ctx.beginPath();
      ctx.rect(sr.x, sr.y, sr.w, sr.h);
      ctx.rect(ar.x, ar.y, ar.w, ar.h);
      ctx.fill("evenodd");
    }
    ctx.strokeStyle = ACCENT;
    ctx.lineWidth = 2;
    ctx.strokeRect(sr.x, sr.y, sr.w - 1, sr.h - 1);
    if (props.onAutoFill) {
      const hx = sr.x + sr.w - 1;
      const hy = sr.y + sr.h - 1;
      ctx.fillStyle = "#ffffff";
      ctx.fillRect(hx - 4, hy - 4, 8, 8);
      ctx.fillStyle = ACCENT;
      ctx.fillRect(hx - 3, hy - 3, 6, 6);
    }

    if (fillTarget) {
      const t = rangeRect(fillTarget);
      ctx.strokeStyle = "#555";
      ctx.lineWidth = 1;
      ctx.setLineDash([3, 3]);
      ctx.strokeRect(t.x + 0.5, t.y + 0.5, t.w - 1, t.h - 1);
      ctx.setLineDash([]);
    }

    if (props.copyRange) {
      const cr = rangeRect(props.copyRange);
      ctx.strokeStyle = ACCENT;
      ctx.lineWidth = 2;
      ctx.setLineDash([5, 3]);
      ctx.lineDashOffset = -dash;
      ctx.strokeRect(cr.x + 1, cr.y + 1, cr.w - 2, cr.h - 2);
      ctx.setLineDash([]);
      ctx.lineDashOffset = 0;
    }
    ctx.restore();

    // Column headers.
    ctx.font = `${Math.round(11 * hz)}px ${UI_FONT}`;
    ctx.textBaseline = "middle";
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, 0, size.w, HEADER_H);
    const headerCols = [...frozenCols, ...scrollCols];
    for (const c of headerCols) {
      if (cols.size(c) === 0) continue;
      const x = colX(c);
      const w = cols.size(c) * z;
      const inSel = c >= sel.c0 && c <= sel.c1;
      ctx.save();
      ctx.beginPath();
      const clipX = c <= fc ? rowHeaderW : bodyX0;
      ctx.rect(Math.max(x, clipX), 0, Math.max(0, x + w - Math.max(x, clipX)), HEADER_H);
      ctx.clip();
      if (inSel) {
        ctx.fillStyle = headerSel;
        ctx.fillRect(x, 0, w, HEADER_H);
        ctx.fillStyle = ACCENT;
        ctx.fillRect(x, HEADER_H - 2, w, 2);
      }
      ctx.fillStyle = inSel ? ACCENT : headerText;
      const sub = source.colSubHeader?.(c);
      if (sub) {
        ctx.textAlign = "left";
        const label = source.colHeader(c);
        ctx.fillText(label, x + 6, HEADER_H / 2 + 1);
        const lw = ctx.measureText(label).width;
        ctx.fillStyle = "#9aa0a6";
        ctx.fillText(sub, x + 12 + lw, HEADER_H / 2 + 1);
      } else {
        ctx.textAlign = "center";
        ctx.fillText(source.colHeader(c), x + w / 2, HEADER_H / 2 + 1);
      }
      ctx.restore();
    }
    // Row headers.
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, HEADER_H, rowHeaderW, size.h);
    const headerRows = [...frozenRows, ...scrollRows];
    ctx.textAlign = "center";
    for (const r of headerRows) {
      if (rows.size(r) === 0) continue;
      const y = rowY(r);
      const h = rows.size(r) * z;
      const inSel = r >= sel.r0 && r <= sel.r1;
      ctx.save();
      ctx.beginPath();
      const clipY = r <= fr ? HEADER_H : bodyY0;
      ctx.rect(0, Math.max(y, clipY), rowHeaderW, Math.max(0, y + h - Math.max(y, clipY)));
      ctx.clip();
      if (inSel) {
        ctx.fillStyle = headerSel;
        ctx.fillRect(0, y, rowHeaderW, h);
        ctx.fillStyle = ACCENT;
        ctx.fillRect(rowHeaderW - 2, y, 2, h);
      }
      ctx.fillStyle = inSel ? ACCENT : headerText;
      ctx.fillText(source.rowHeader(r), rowHeaderW / 2, y + h / 2 + 1);
      ctx.restore();
    }
    // Header separators.
    ctx.strokeStyle = "#c6c6c6";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const c of headerCols) {
      const x = Math.round(colX(c) + cols.size(c) * z) - 0.5;
      if (x < rowHeaderW || (c > fc && x < bodyX0)) continue;
      ctx.moveTo(x, 0);
      ctx.lineTo(x, HEADER_H);
    }
    for (const r of headerRows) {
      const y = Math.round(rowY(r) + rows.size(r) * z) - 0.5;
      if (y < HEADER_H || (r > fr && y < bodyY0)) continue;
      ctx.moveTo(0, y);
      ctx.lineTo(rowHeaderW, y);
    }
    ctx.moveTo(0, HEADER_H - 0.5);
    ctx.lineTo(size.w, HEADER_H - 0.5);
    ctx.moveTo(rowHeaderW - 0.5, 0);
    ctx.lineTo(rowHeaderW - 0.5, size.h);
    ctx.stroke();
    // Frozen pane dividers.
    ctx.strokeStyle = "#8f8f8f";
    ctx.beginPath();
    if (fr) {
      ctx.moveTo(rowHeaderW, bodyY0 - 0.5);
      ctx.lineTo(size.w, bodyY0 - 0.5);
    }
    if (fc) {
      ctx.moveTo(bodyX0 - 0.5, HEADER_H);
      ctx.lineTo(bodyX0 - 0.5, size.h);
    }
    ctx.stroke();
    // Select-all corner with Excel's triangle.
    ctx.fillStyle = headerBg;
    ctx.fillRect(0, 0, rowHeaderW - 1, HEADER_H - 1);
    ctx.fillStyle = "#b5b5b5";
    ctx.beginPath();
    ctx.moveTo(rowHeaderW - 4, HEADER_H - 4);
    ctx.lineTo(rowHeaderW - 4, HEADER_H - 13 * hz);
    ctx.lineTo(rowHeaderW - 4 - 9 * hz, HEADER_H - 4);
    ctx.closePath();
    ctx.fill();
  });

  // ------------------------------------------------------------------ hit testing
  const hit = (clientX: number, clientY: number): Hit & { x: number; y: number } => {
    const rect = scrollRef.current!.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    const bodyX0 = rowHeaderW + frozenW * z;
    const bodyY0 = HEADER_H + frozenH * z;
    const c = x < bodyX0 ? cols.indexAt(Math.max(0, (x - rowHeaderW) / z)) : cols.indexAt(frozenW + scrollX + (x - bodyX0) / z);
    const r = y < bodyY0 ? rows.indexAt(Math.max(0, (y - HEADER_H) / z)) : rows.indexAt(frozenH + scrollY + (y - bodyY0) / z);
    const area = y < HEADER_H ? (x < rowHeaderW ? "corner" : "colHeader") : x < rowHeaderW ? "rowHeader" : "cell";
    return { area, r: Math.min(Math.max(1, r), rows.count), c: Math.min(Math.max(1, c), cols.count), x, y };
  };

  const colBorderAt = (x: number): number | null => {
    const cands = [...Array.from({ length: fc }, (_, i) => i + 1)];
    for (let c = Math.max(fc + 1, firstCol - 1); c <= lastCol; c++) cands.push(c);
    for (const c of cands) if (Math.abs(x - (colX(c) + cols.size(c) * z)) <= 3) return c;
    return null;
  };
  const rowBorderAt = (y: number): number | null => {
    const cands = [...Array.from({ length: fr }, (_, i) => i + 1)];
    for (let r = Math.max(fr + 1, firstRow - 1); r <= lastRow; r++) cands.push(r);
    for (const r of cands) if (Math.abs(y - (rowY(r) + rows.size(r) * z)) <= 3) return r;
    return null;
  };

  const onFillHandle = (x: number, y: number) => {
    if (!props.onAutoFill) return false;
    const sr = rangeRect(expandForMerges(normalize(selection.anchor, selection.focus)));
    return Math.abs(x - (sr.x + sr.w - 1)) <= 5 && Math.abs(y - (sr.y + sr.h - 1)) <= 5;
  };

  const onMouseDown = (e: React.MouseEvent) => {
    const h = hit(e.clientX, e.clientY);
    if (e.button === 2) {
      props.onContextMenu(h, e.clientX, e.clientY);
      return;
    }
    if (e.button !== 0) return;
    if (h.area === "colHeader" && props.onColumnResize) {
      const c = colBorderAt(h.x);
      if (c !== null) {
        e.preventDefault();
        if (e.detail === 2) {
          props.onAutoFitColumn?.(c);
          return;
        }
        gesture.current = { kind: "colResize", c, startX: e.clientX, startW: cols.size(c) };
        return;
      }
    }
    if (h.area === "rowHeader" && props.onRowResize) {
      const r = rowBorderAt(h.y);
      if (r !== null) {
        e.preventDefault();
        gesture.current = { kind: "rowResize", r, startY: e.clientY, startH: rows.size(r) };
        return;
      }
    }
    if (h.area === "cell" && onFillHandle(h.x, h.y)) {
      e.preventDefault();
      gesture.current = { kind: "fill", source: expandForMerges(normalize(selection.anchor, selection.focus)) };
      return;
    }
    e.preventDefault(); // keep focus in the cell editor
    gesture.current = { kind: "select" };
    props.onCellDown(h, e);
  };

  const onMouseMove = (e: React.MouseEvent) => {
    const g = gesture.current;
    const el = scrollRef.current;
    const h = hit(e.clientX, e.clientY);
    if (!g) {
      if (!el) return;
      let cursor = "cell";
      if (h.area === "colHeader") cursor = props.onColumnResize && colBorderAt(h.x) !== null ? "col-resize" : "s-resize";
      else if (h.area === "rowHeader") cursor = props.onRowResize && rowBorderAt(h.y) !== null ? "row-resize" : "e-resize";
      else if (h.area === "cell" && onFillHandle(h.x, h.y)) cursor = "crosshair";
      el.style.cursor = cursor;
      return;
    }
    if (g.kind === "select") props.onCellDrag(h);
    else if (g.kind === "colResize") props.onColumnResize?.(g.c, Math.max(0, g.startW + (e.clientX - g.startX) / z), false);
    else if (g.kind === "rowResize") props.onRowResize?.(g.r, Math.max(0, g.startH + (e.clientY - g.startY) / z), false);
    else if (g.kind === "fill") {
      const s = g.source;
      const dr = h.r > s.r1 ? h.r - s.r1 : h.r < s.r0 ? h.r - s.r0 : 0;
      const dc = h.c > s.c1 ? h.c - s.c1 : h.c < s.c0 ? h.c - s.c0 : 0;
      let t: Range | null = null;
      if (Math.abs(dr) >= Math.abs(dc) && dr !== 0) t = dr > 0 ? { ...s, r1: h.r } : { ...s, r0: h.r };
      else if (dc !== 0) t = dc > 0 ? { ...s, c1: h.c } : { ...s, c0: h.c };
      setFillTarget(t);
    }
  };

  const fillTargetRef = useRef<Range | null>(null);
  fillTargetRef.current = fillTarget;
  const propsRef = useRef(props);
  propsRef.current = props;
  useEffect(() => {
    const up = (e: MouseEvent) => {
      const g = gesture.current;
      gesture.current = null;
      if (!g) return;
      const p = propsRef.current;
      const zz = p.zoom;
      if (g.kind === "select") p.onCellUp();
      else if (g.kind === "colResize") p.onColumnResize?.(g.c, Math.max(0, g.startW + (e.clientX - g.startX) / zz), true);
      else if (g.kind === "rowResize") p.onRowResize?.(g.r, Math.max(0, g.startH + (e.clientY - g.startY) / zz), true);
      else if (g.kind === "fill") {
        const t = fillTargetRef.current;
        setFillTarget(null);
        if (t) p.onAutoFill?.(g.source, t);
      }
    };
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  // Wheel: three rows per notch; Ctrl+wheel zooms (Excel).
  const wheelRef = useRef<(e: WheelEvent) => void>(() => {});
  wheelRef.current = (e: WheelEvent) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      props.onZoom?.(Math.min(4, Math.max(0.1, Math.round((z + (e.deltaY < 0 ? 0.1 : -0.1)) * 10) / 10)));
      return;
    }
    if (Math.abs(e.deltaY) < Math.abs(e.deltaX)) return;
    e.preventDefault();
    const step = Math.sign(e.deltaY) * Math.max(3, Math.round(Math.abs(e.deltaY) / 40) * 3);
    const curRow = rows.indexAt(frozenH + scrollY);
    const nextRow = Math.min(Math.max(fr + 1, curRow + step), rows.count);
    setScrollTo(topForScrollY(rows.offset(nextRow) - frozenH), scroll.left);
  };
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const h = (e: WheelEvent) => wheelRef.current(e);
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  }, []);

  const em = source.mergeAt(props.editorAt.r, props.editorAt.c);
  const editorRect = cellRect(props.editorAt.r, props.editorAt.c, em);
  const editorFont = fontCss(source.cell(props.editorAt.r, props.editorAt.c)?.style, source.font, z);

  return (
    <div className="grid-wrap">
      <canvas ref={canvasRef} className="grid-canvas" />
      <div
        ref={scrollRef}
        className="grid-scroll"
        onScroll={(e) => setScroll({ top: e.currentTarget.scrollTop, left: e.currentTarget.scrollLeft })}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onContextMenu={(e) => e.preventDefault()}
        onDoubleClick={(e) => {
          const h = hit(e.clientX, e.clientY);
          if (h.area === "cell" && !onFillHandle(h.x, h.y)) props.onDoubleClick(h);
        }}
      >
        <div style={{ width: rowHeaderW + frozenW * z + scrollTotalW * z, height: HEADER_H + frozenH * z + scrollAreaH }} />
      </div>
      {props.renderEditor(editorRect, editorFont)}
    </div>
  );
}
