// Printing like Excel's 파일 → 인쇄: settings with a page preview, Excel's page breaks
// (automatic and manual, down then over), print titles, page numbers and charts.
// The system print dialog also saves PDF.
import { ShapeFigure, ShapeText } from "./Shape";
import { SparklineSvg } from "./Sparkline";
import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { CellStyle, CellView, Chart, ChartData, PageSetup, Picture, Shape, SparklineView } from "../api";
import { fontCss } from "../grid/Grid";
import type { Merge } from "../grid/sources";
import { ChartSvg } from "./Chart";
import { Modal } from "./Dialogs";

export interface PrintJob {
  title: string;
  /** File name, for &F in headers and footers. */
  file: string;
  r0: number;
  c0: number;
  r1: number;
  c1: number;
  /** Sizes of every row/column from 1 (for title rows outside the range). */
  colWidth(c: number): number;
  rowHeight(r: number): number;
  cells: Map<string, CellView>;
  merges: Merge[];
  font: [string, number];
  charts: { chart: Chart; data: ChartData }[];
  /** Pictures with their image data. */
  pictures: { picture: Picture; url: string }[];
  shapes?: Shape[];
  sparklines?: SparklineView[];
}

/** Printable A4 area at 96 dpi with Excel's normal margins (0.7" sides, 0.75" top/bottom). */
export function pageSize(landscape: boolean): { w: number; h: number } {
  return landscape ? { w: 989, h: 650 } : { w: 660, h: 979 };
}

export interface Pagination {
  scale: number;
  rowPages: [number, number][];
  colPages: [number, number][];
  /** (row page, column page) in printing order: down, then over. */
  order: [number, number][];
}

function split(from: number, to: number, size: (i: number) => number, avail: (page: number) => number, breaks: number[]): [number, number][] {
  const out: [number, number][] = [];
  let start = from;
  let used = 0;
  for (let i = from; i <= to; i++) {
    const s = size(i);
    const forced = i > start && breaks.includes(i);
    if (i > start && (forced || used + s > avail(out.length))) {
      out.push([start, i - 1]);
      start = i;
      used = 0;
    }
    used += s;
  }
  out.push([start, to]);
  return out;
}

/** The footer as printed: 「페이지 번호」 stands for a centered "&P / &N" when no footer is set. */
export function effectiveFooter(setup: PageSetup): [string, string, string] {
  const f = setup.footer ?? ["", "", ""];
  return setup.page_numbers && f.every((x) => !x) ? ["", "&P / &N", ""] : f;
}

/** A header or footer part with its codes filled in (&P, &N, &D, &T, &F, &A, &&). */
export function renderHeaderFooter(part: string, page: number, pages: number, file: string, sheet: string): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}:${pad(now.getMinutes())}`;
  return part.replace(/&([PNDTFA&])/g, (_, c: string) =>
    c === "P" ? String(page) : c === "N" ? String(pages) : c === "D" ? date : c === "T" ? time : c === "F" ? file : c === "A" ? sheet : "&",
  );
}

const HF_H = 28;

export function paginate(job: PrintJob, setup: PageSetup): Pagination {
  const page = pageSize(setup.landscape);
  const headW = setup.headings ? 40 : 0;
  const headH = setup.headings ? 20 : 0;
  let totalW = headW;
  for (let c = job.c0; c <= job.c1; c++) totalW += job.colWidth(c);
  let totalH = headH;
  for (let r = job.r0; r <= job.r1; r++) totalH += job.rowHeight(r);
  // Room for the header and the footer.
  const footer = ((setup.header ?? []).some((x) => x) ? HF_H : 0) + (effectiveFooter(setup).some((x) => x) ? HF_H : 0);
  const scale = setup.fit === "None" ? 1 : Math.min(1, page.w / totalW, setup.fit === "Page" ? (page.h - footer) / totalH : 1);
  const titles = setup.title_rows;
  let titleH = 0;
  if (titles) for (let r = titles[0]; r <= titles[1]; r++) titleH += job.rowHeight(r);
  const inRange = (r: number) => r >= job.r0 && r <= job.r1;
  const rowPages = split(
    job.r0,
    job.r1,
    job.rowHeight,
    // Title rows repeat on every page that does not already start with them.
    (n) => (page.h - footer) / scale - headH - (titles && (n > 0 || !inRange(titles[0])) ? titleH : 0),
    setup.row_breaks,
  );
  const colPages = split(job.c0, job.c1, job.colWidth, () => page.w / scale - headW, setup.col_breaks);
  const order: [number, number][] = [];
  for (let cp = 0; cp < colPages.length; cp++) for (let rp = 0; rp < rowPages.length; rp++) order.push([rp, cp]);
  return { scale, rowPages, colPages, order };
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
    x
      ? `${x.style.startsWith("medium") ? 2 : x.style === "thick" || x.style === "double" ? 3 : 1}px ${x.style === "double" ? "double" : x.style.includes("dash") || x.style === "dotted" ? "dashed" : "solid"} ${x.color}`
      : grid
        ? "1px solid #c8c8c8"
        : undefined;
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

/** One printed page. */
export function PageView(props: { job: PrintJob; setup: PageSetup; pag: Pagination; index: number }) {
  const { job, setup, pag } = props;
  const [rp, cp] = pag.order[props.index];
  const [ra, rb] = pag.rowPages[rp];
  const [ca, cb] = pag.colPages[cp];
  const page = pageSize(setup.landscape);
  const header = setup.header ?? ["", "", ""];
  const footer = effectiveFooter(setup);
  const t = setup.title_rows;
  const titleRows: number[] = [];
  if (t && (rp > 0 || t[0] < job.r0)) for (let r = t[0]; r <= t[1]; r++) if (r < ra) titleRows.push(r);
  const rowsToDraw = [...titleRows, ...Array.from({ length: rb - ra + 1 }, (_, i) => ra + i)];
  const covered = new Set<string>();
  const mergeAt = new Map<string, Merge>();
  for (const m of job.merges) {
    mergeAt.set(`${m.r}:${m.c}`, m);
    for (let r = m.r; r < m.r + m.h; r++) for (let c = m.c; c < m.c + m.w; c++) if (r !== m.r || c !== m.c) covered.add(`${r}:${c}`);
  }
  const colsToDraw = Array.from({ length: cb - ca + 1 }, (_, i) => ca + i).filter((c) => job.colWidth(c) > 0);
  const headW = setup.headings ? 40 : 0;
  const headH = setup.headings ? 20 : 0;
  const width = colsToDraw.reduce((a, c) => a + job.colWidth(c), headW);
  let titleH = 0;
  for (const r of titleRows) titleH += job.rowHeight(r);
  // Charts whose top-left cell is on this page.
  const charts = job.charts.filter((x) => x.chart.anchor.row >= ra && x.chart.anchor.row <= rb && x.chart.anchor.col >= ca && x.chart.anchor.col <= cb);
  const chartPos = (ch: { anchor: Chart["anchor"] }) => {
    let left = headW + ch.anchor.dx;
    for (let c = ca; c < ch.anchor.col; c++) left += job.colWidth(c);
    let top = headH + titleH + ch.anchor.dy;
    for (let r = ra; r < ch.anchor.row; r++) top += job.rowHeight(r);
    return { left, top };
  };
  const shapes = (job.shapes ?? []).filter((x) => x.anchor.row >= ra && x.anchor.row <= rb && x.anchor.col >= ca && x.anchor.col <= cb);
  const pictures = job.pictures.filter(
    (x) => x.picture.anchor.row >= ra && x.picture.anchor.row <= rb && x.picture.anchor.col >= ca && x.picture.anchor.col <= cb,
  );
  return (
    <div className="print-page" style={{ width: page.w, height: page.h }}>
      {header.some((x) => x) && (
        <div className="print-hf print-header">
          {header.map((x, i) => (
            <span key={i}>{renderHeaderFooter(x, props.index + 1, pag.order.length, job.file, job.title)}</span>
          ))}
        </div>
      )}
      <div style={{ zoom: pag.scale, position: "relative", width, marginTop: header.some((x) => x) ? HF_H : 0 }}>
        <table className="print-table" style={{ width }}>
          <colgroup>
            {setup.headings && <col style={{ width: headW }} />}
            {colsToDraw.map((c) => (
              <col key={c} style={{ width: job.colWidth(c) }} />
            ))}
          </colgroup>
          {setup.headings && (
            <thead>
              <tr style={{ height: headH }}>
                <th />
                {colsToDraw.map((c) => (
                  <th key={c}>{colName(c)}</th>
                ))}
              </tr>
            </thead>
          )}
          <tbody>
            {rowsToDraw.map((r, k) => {
              const h = job.rowHeight(r);
              if (h === 0) return null;
              return (
                <tr key={`${k}:${r}`} style={{ height: h }}>
                  {setup.headings && <th>{r}</th>}
                  {colsToDraw.map((c) => {
                    if (covered.has(`${r}:${c}`)) return null;
                    const cell = job.cells.get(`${r}:${c}`);
                    const m = mergeAt.get(`${r}:${c}`);
                    return (
                      <td key={c} rowSpan={m?.h} colSpan={m?.w} style={cellCss(cell?.style, !!cell?.numeric, job.font, setup.gridlines)}>
                        {cell?.text ?? ""}
                      </td>
                    );
                  })}
                </tr>
              );
            })}
          </tbody>
        </table>
        {pictures.map(({ picture, url }) => {
          const p = chartPos(picture);
          return (
            <div key={`p${picture.id}`} className="print-picture" style={{ left: p.left, top: p.top, width: picture.anchor.width, height: picture.anchor.height }}>
              <img src={url} alt={picture.name} />
            </div>
          );
        })}
        {(job.sparklines ?? [])
          .filter((x) => x.row >= ra && x.row <= rb && x.col >= ca && x.col <= cb)
          .map((sp) => {
            const p = chartPos({ anchor: { row: sp.row, col: sp.col, dx: 0, dy: 0, width: 0, height: 0 } });
            const w = job.colWidth(sp.col);
            const h = job.rowHeight(sp.row);
            return (
              <div key={`k${sp.row}:${sp.col}`} className="print-sparkline" style={{ left: p.left, top: p.top, width: w, height: h }}>
                <SparklineSvg s={sp} w={w} h={h} />
              </div>
            );
          })}
        {shapes.map((shape) => {
          const p = chartPos(shape);
          return (
            <div key={`s${shape.id}`} className="print-shape" style={{ left: p.left, top: p.top, width: shape.anchor.width, height: shape.anchor.height }}>
              <ShapeFigure shape={shape} w={shape.anchor.width} h={shape.anchor.height} />
              <ShapeText shape={shape} />
            </div>
          );
        })}
        {charts.map(({ chart, data }) => {
          const p = chartPos(chart);
          return (
            <div key={chart.id} className="print-chart" style={{ left: p.left, top: p.top, width: chart.anchor.width, height: chart.anchor.height }}>
              <ChartSvg kind={chart.kind} data={data} width={chart.anchor.width} height={chart.anchor.height} legend={chart.legend} trendline={chart.trendline} />
            </div>
          );
        })}
      </div>
      {footer.some((x) => x) && (
        <div className="print-hf print-footer">
          {footer.map((x, i) => (
            <span key={i}>{renderHeaderFooter(x, props.index + 1, pag.order.length, job.file, job.title)}</span>
          ))}
        </div>
      )}
    </div>
  );
}

/** Every page, for the print dialog; hidden on screen. */
export function PrintView(props: { job: PrintJob; setup: PageSetup }) {
  const pag = useMemo(() => paginate(props.job, props.setup), [props.job, props.setup]);
  return createPortal(
    <div id="print-root">
      <style>{`@page { size: A4 ${props.setup.landscape ? "landscape" : "portrait"}; margin: 0.75in 0.7in; }`}</style>
      {pag.order.map((_, i) => (
        <PageView key={i} job={props.job} setup={props.setup} pag={pag} index={i} />
      ))}
    </div>,
    document.body,
  );
}

/** 파일 → 인쇄: settings on the left, the page on the right. */
export function PrintPreviewDialog(props: {
  job: PrintJob;
  setup: PageSetup;
  hasSelection: boolean;
  selectionOnly: boolean;
  onScope(selection: boolean): void;
  onChange(s: PageSetup): void;
  onPrint(): void;
  onClose(): void;
}) {
  const s = props.setup;
  const pag = useMemo(() => paginate(props.job, s), [props.job, s]);
  const [index, setIndex] = useState(0);
  const i = Math.min(index, pag.order.length - 1);
  const page = pageSize(s.landscape);
  const viewScale = Math.min(0.62, 440 / page.h, 520 / page.w);
  const [titles, setTitles] = useState(s.title_rows ? `${s.title_rows[0]}:${s.title_rows[1]}` : "");
  return (
    <Modal title="인쇄" wide onClose={props.onClose}>
      <div className="modal-body print-preview">
        <div className="print-settings">
          <button className="primary big-print" onClick={props.onPrint}>
            🖨 인쇄
          </button>
          <label>
            인쇄 대상
            <select value={props.selectionOnly ? "s" : "a"} onChange={(e) => props.onScope(e.target.value === "s")}>
              <option value="a">활성 시트 인쇄</option>
              <option value="s" disabled={!props.hasSelection}>
                선택 영역 인쇄
              </option>
            </select>
          </label>
          <label>
            용지 방향
            <select value={s.landscape ? "l" : "p"} onChange={(e) => props.onChange({ ...s, landscape: e.target.value === "l" })}>
              <option value="p">세로 방향</option>
              <option value="l">가로 방향</option>
            </select>
          </label>
          <label>
            배율
            <select value={s.fit} onChange={(e) => props.onChange({ ...s, fit: e.target.value as PageSetup["fit"] })}>
              <option value="None">현재 크기</option>
              <option value="Width">모든 열을 한 페이지에 맞추기</option>
              <option value="Page">한 페이지에 시트 맞추기</option>
            </select>
          </label>
          <label>
            인쇄 제목 (반복할 행)
            <input
              value={titles}
              placeholder="예: 1:1"
              onChange={(e) => setTitles(e.target.value)}
              onBlur={() => {
                const m = /^\$?(\d+)(?::\$?(\d+))?$/.exec(titles.trim());
                props.onChange({ ...s, title_rows: m ? [Number(m[1]), Number(m[2] ?? m[1])] : null });
              }}
            />
          </label>
          <label className="check">
            <input type="checkbox" checked={s.gridlines} onChange={(e) => props.onChange({ ...s, gridlines: e.target.checked })} /> 눈금선 인쇄
          </label>
          <label className="check">
            <input type="checkbox" checked={s.headings} onChange={(e) => props.onChange({ ...s, headings: e.target.checked })} /> 행/열 머리글 인쇄
          </label>
          <label className="check">
            <input type="checkbox" checked={s.page_numbers} onChange={(e) => props.onChange({ ...s, page_numbers: e.target.checked })} /> 바닥글에 페이지 번호
          </label>
          <div className="muted">PDF는 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.</div>
        </div>
        <div className="print-stage">
          <div className="print-sheet" style={{ width: page.w * viewScale + 2, height: page.h * viewScale + 2 }}>
            <div style={{ transform: `scale(${viewScale})`, transformOrigin: "0 0" }}>
              <PageView job={props.job} setup={s} pag={pag} index={i} />
            </div>
          </div>
          <div className="cf-row print-nav">
            <button disabled={i === 0} onClick={() => setIndex(i - 1)}>
              ◀
            </button>
            <span>
              {i + 1} / {pag.order.length} 페이지
            </span>
            <button disabled={i >= pag.order.length - 1} onClick={() => setIndex(i + 1)}>
              ▶
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
