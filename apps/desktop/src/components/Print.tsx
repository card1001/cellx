// Printing: Excel's 인쇄 settings, then the sheet as an HTML table for the system print
// dialog (which also saves PDF).
import { useState } from "react";
import { createPortal } from "react-dom";
import type { CellStyle, CellView } from "../api";
import { fontCss } from "../grid/Grid";
import type { Merge } from "../grid/sources";
import { Modal } from "./Dialogs";

export interface PrintOptions {
  scope: "sheet" | "selection";
  landscape: boolean;
  gridlines: boolean;
  headings: boolean;
  fit: "none" | "width" | "page";
}

export interface PrintJob {
  title: string;
  options: PrintOptions;
  r0: number;
  c0: number;
  r1: number;
  c1: number;
  colWidths: number[];
  rowHeights: number[];
  cells: Map<string, CellView>;
  merges: Merge[];
  font: [string, number];
}

export function PrintDialog(props: { hasSelection: boolean; onDone(o: PrintOptions | null): void }) {
  const [o, setO] = useState<PrintOptions>({ scope: props.hasSelection ? "selection" : "sheet", landscape: false, gridlines: false, headings: false, fit: "width" });
  return (
    <Modal title="인쇄" onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(o);
        }}
      >
        <label>
          인쇄 대상
          <select value={o.scope} onChange={(e) => setO({ ...o, scope: e.target.value as PrintOptions["scope"] })}>
            <option value="sheet">활성 시트 인쇄</option>
            <option value="selection" disabled={!props.hasSelection}>
              선택 영역 인쇄
            </option>
          </select>
        </label>
        <label>
          용지 방향
          <select value={o.landscape ? "l" : "p"} onChange={(e) => setO({ ...o, landscape: e.target.value === "l" })}>
            <option value="p">세로 방향</option>
            <option value="l">가로 방향</option>
          </select>
        </label>
        <label>
          배율
          <select value={o.fit} onChange={(e) => setO({ ...o, fit: e.target.value as PrintOptions["fit"] })}>
            <option value="none">현재 크기</option>
            <option value="width">모든 열을 한 페이지에 맞추기</option>
            <option value="page">한 페이지에 시트 맞추기</option>
          </select>
        </label>
        <div className="cf-row">
          <label className="check">
            <input type="checkbox" checked={o.gridlines} onChange={(e) => setO({ ...o, gridlines: e.target.checked })} /> 눈금선 인쇄
          </label>
          <label className="check">
            <input type="checkbox" checked={o.headings} onChange={(e) => setO({ ...o, headings: e.target.checked })} /> 행/열 머리글 인쇄
          </label>
        </div>
        <div className="muted">PDF로 저장하려면 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.</div>
        <div className="actions">
          <button type="submit" className="primary" autoFocus>
            인쇄
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

function colName(c: number): string {
  let s = "";
  while (c > 0) {
    const m = (c - 1) % 26;
    s = String.fromCharCode(65 + m) + s;
    c = Math.floor((c - 1) / 26);
  }
  return s;
}

function cellCss(st: CellStyle | undefined, numeric: boolean, font: [string, number], grid: boolean): React.CSSProperties {
  const b = (x?: { style: string; color: string }) =>
    x ? `${x.style.startsWith("medium") ? 2 : x.style === "thick" || x.style === "double" ? 3 : 1}px ${x.style === "double" ? "double" : x.style.includes("dash") || x.style === "dotted" ? "dashed" : "solid"} ${x.color}` : grid ? "1px solid #d4d4d8" : undefined;
  return {
    font: fontCss(st, font, 1),
    color: st?.color,
    background: st?.fill,
    textAlign: (st?.align ?? (numeric ? "right" : "left")) as React.CSSProperties["textAlign"],
    verticalAlign: st?.valign === "top" ? "top" : st?.valign === "center" ? "middle" : "bottom",
    whiteSpace: st?.wrap ? "pre-wrap" : "pre",
    textDecoration: [st?.underline ? "underline" : "", st?.strike ? "line-through" : ""].join(" ").trim() || undefined,
    borderTop: b(st?.border_top),
    borderRight: b(st?.border_right),
    borderBottom: b(st?.border_bottom),
    borderLeft: b(st?.border_left),
  };
}

/** The page that is printed; hidden on screen by CSS. */
export function PrintView(props: { job: PrintJob }) {
  const j = props.job;
  const o = j.options;
  const totalW = j.colWidths.reduce((a, b) => a + b, 0) + (o.headings ? 40 : 0);
  const totalH = j.rowHeights.reduce((a, b) => a + b, 0) + (o.headings ? 20 : 0);
  // Printable area of an A4 page at 96 dpi with Excel's normal margins.
  const pageW = o.landscape ? 1010 : 690;
  const pageH = o.landscape ? 650 : 980;
  const scale = o.fit === "none" ? 1 : Math.min(1, pageW / totalW, o.fit === "page" ? pageH / totalH : 1);
  const covered = new Set<string>();
  const mergeAt = new Map<string, Merge>();
  for (const m of j.merges) {
    mergeAt.set(`${m.r}:${m.c}`, m);
    for (let r = m.r; r < m.r + m.h; r++) for (let c = m.c; c < m.c + m.w; c++) if (r !== m.r || c !== m.c) covered.add(`${r}:${c}`);
  }
  const rows = [];
  for (let r = j.r0; r <= j.r1; r++) {
    const h = j.rowHeights[r - j.r0];
    if (h === 0) continue;
    const tds = [];
    if (o.headings) tds.push(<th key="h">{r}</th>);
    for (let c = j.c0; c <= j.c1; c++) {
      if (j.colWidths[c - j.c0] === 0 || covered.has(`${r}:${c}`)) continue;
      const cell = j.cells.get(`${r}:${c}`);
      const m = mergeAt.get(`${r}:${c}`);
      tds.push(
        <td key={c} rowSpan={m?.h} colSpan={m?.w} style={cellCss(cell?.style, !!cell?.numeric, j.font, o.gridlines)}>
          {cell?.text ?? ""}
        </td>,
      );
    }
    rows.push(
      <tr key={r} style={{ height: h }}>
        {tds}
      </tr>,
    );
  }
  return createPortal(
    <div id="print-root">
      <style>{`@page { size: A4 ${o.landscape ? "landscape" : "portrait"}; margin: 18mm 15mm; }`}</style>
      <div style={{ zoom: scale }}>
        <table className="print-table" style={{ width: totalW }}>
          <colgroup>
            {o.headings && <col style={{ width: 40 }} />}
            {j.colWidths.map((w, i) => (w > 0 ? <col key={i} style={{ width: w }} /> : null))}
          </colgroup>
          {o.headings && (
            <thead>
              <tr>
                <th />
                {j.colWidths.map((w, i) => (w > 0 ? <th key={i}>{colName(j.c0 + i)}</th> : null))}
              </tr>
            </thead>
          )}
          <tbody>{rows}</tbody>
        </table>
      </div>
    </div>,
    document.body,
  );
}
