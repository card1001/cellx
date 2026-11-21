// Data sources for the grid: regular sheets (calculation engine) and data sheets (DuckDB).
import { api, type ColumnFilter, type ColumnKind, type Page, type Scalar, type Sort, type TableInfo } from "../api";
import type { CellData, GridSource } from "./Grid";
import { colName, MAX_COLS, MAX_ROWS } from "./model";

type Listener = () => void;

class Emitter {
  private listeners = new Set<Listener>();
  subscribe(cb: Listener) {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }
  protected emit() {
    for (const l of this.listeners) l();
  }
}

const BLOCK_R = 64;
const BLOCK_C = 16;

/** A regular sheet. Cells are fetched in blocks around the viewport and cached. */
export class SheetSource extends Emitter implements GridSource {
  readonly rowHeight = 24;
  private rows: number;
  private cols: number;
  private cells = new Map<number, CellData>();
  private widths = new Map<number, number>();
  private loaded = new Set<string>();
  private inflight = new Set<string>();
  private generation = 0;
  private last: [number, number, number, number] = [1, 1, 40, 20];

  constructor(
    readonly sheet: number,
    usedRows: number,
    usedCols: number,
  ) {
    super();
    this.rows = Math.min(MAX_ROWS, Math.max(200, usedRows + 100));
    this.cols = Math.min(MAX_COLS, Math.max(26, usedCols + 5));
  }

  private key(r: number, c: number) {
    return r * 20000 + c;
  }

  rowCount() {
    return this.rows;
  }
  colCount() {
    return this.cols;
  }
  colWidth(c: number) {
    return this.widths.get(c) ?? 100;
  }
  setColWidth(c: number, w: number) {
    this.widths.set(c, w);
    this.emit();
  }
  colHeader(c: number) {
    return colName(c);
  }
  rowHeader(r: number) {
    return String(r);
  }
  cell(r: number, c: number) {
    return this.cells.get(this.key(r, c));
  }

  grow(r: number, c: number) {
    let changed = false;
    if (r > this.rows - 50 && this.rows < MAX_ROWS) {
      this.rows = Math.min(MAX_ROWS, r + 500);
      changed = true;
    }
    if (c > this.cols - 3 && this.cols < MAX_COLS) {
      this.cols = Math.min(MAX_COLS, c + 10);
      changed = true;
    }
    if (changed) this.emit();
  }

  request(r0: number, c0: number, r1: number, c1: number) {
    this.last = [r0, c0, r1, c1];
    const br0 = Math.floor((r0 - 1) / BLOCK_R);
    const br1 = Math.floor((r1 - 1) / BLOCK_R);
    const bc0 = Math.floor((c0 - 1) / BLOCK_C);
    const bc1 = Math.floor((c1 - 1) / BLOCK_C);
    for (let br = br0; br <= br1; br++) {
      for (let bc = bc0; bc <= bc1; bc++) {
        const k = `${br}:${bc}`;
        if (this.loaded.has(k) || this.inflight.has(k)) continue;
        this.inflight.add(k);
        void this.fetchBlock(br, bc, k, this.generation);
      }
    }
  }

  private async fetchBlock(br: number, bc: number, k: string, gen: number) {
    const r0 = br * BLOCK_R + 1;
    const c0 = bc * BLOCK_C + 1;
    const r1 = r0 + BLOCK_R - 1;
    const c1 = c0 + BLOCK_C - 1;
    try {
      const vp = await api.viewport(this.sheet, r0, c0, r1, c1);
      if (gen !== this.generation) return;
      for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) this.cells.delete(this.key(r, c));
      for (const cell of vp.cells) this.cells.set(this.key(cell.row, cell.col), cell);
      vp.col_widths.forEach((w, i) => {
        const c = c0 + i;
        if (!this.widths.has(c)) this.widths.set(c, w);
      });
      this.loaded.add(k);
      this.emit();
    } finally {
      this.inflight.delete(k);
    }
  }

  /** Drops cached cells after an edit and reloads what is on screen. */
  invalidate() {
    this.generation++;
    this.loaded.clear();
    this.inflight.clear();
    const [r0, c0, r1, c1] = this.last;
    this.request(r0, c0, r1, c1);
  }

  displayText(r: number, c: number) {
    return this.cell(r, c)?.text ?? "";
  }
}

const PAGE = 256;

function pad(n: number) {
  return String(n).padStart(2, "0");
}

export function serialToDate(serial: number, withTime: boolean): string {
  const d = new Date(Math.round((serial - 25569) * 86400000));
  const date = `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
  if (!withTime) return date;
  return `${date} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

const numberFormat = new Intl.NumberFormat("ko-KR", { maximumFractionDigits: 10 });

export function formatScalar(v: Scalar, kind: ColumnKind): CellData {
  switch (v.t) {
    case "Empty":
      return { text: "" };
    case "Number":
      if (kind === "Date") return { text: serialToDate(v.v, false), numeric: true };
      if (kind === "Timestamp") return { text: serialToDate(v.v, true), numeric: true };
      return { text: numberFormat.format(v.v), numeric: true };
    case "Bool":
      return { text: v.v ? "TRUE" : "FALSE" };
    case "Error":
      return { text: v.v, error: true };
    default:
      return { text: v.v };
  }
}

const KIND_LABEL: Record<ColumnKind, string> = {
  Number: "숫자",
  Text: "문자",
  Bool: "논리",
  Date: "날짜",
  Timestamp: "일시",
  Other: "기타",
};

/** A data sheet backed by DuckDB, paged on demand. */
export class DataSource extends Emitter implements GridSource {
  readonly rowHeight = 24;
  private pages = new Map<number, Page>();
  private inflight = new Set<number>();
  private widths = new Map<number, number>();
  private generation = 0;
  total: number;
  sort: Sort | null = null;
  filters: ColumnFilter[] = [];
  lastQueryMs = 0;

  constructor(public info: TableInfo) {
    super();
    this.total = info.rows;
  }

  rowCount() {
    return Math.max(1, this.total);
  }
  colCount() {
    return Math.max(1, this.info.columns.length);
  }
  colWidth(c: number) {
    const w = this.widths.get(c);
    if (w) return w;
    const col = this.info.columns[c - 1];
    if (!col) return 100;
    const byKind = col.kind === "Text" ? 150 : col.kind === "Timestamp" ? 160 : 120;
    return Math.max(byKind, col.name.length * 9 + 60);
  }
  setColWidth(c: number, w: number) {
    this.widths.set(c, w);
    this.emit();
  }
  colHeader(c: number) {
    const col = this.info.columns[c - 1];
    if (!col) return "";
    const marker = this.sort?.column === col.name ? (this.sort.descending ? " ▼" : " ▲") : "";
    return col.name + marker;
  }
  colSubHeader(c: number) {
    const col = this.info.columns[c - 1];
    return col ? KIND_LABEL[col.kind] : "";
  }
  rowHeader(r: number) {
    return String(r);
  }
  column(c: number) {
    return this.info.columns[c - 1];
  }

  private rowAt(r: number): { page: Page; i: number } | undefined {
    const p = Math.floor((r - 1) / PAGE);
    const page = this.pages.get(p);
    if (!page) return undefined;
    const i = r - 1 - p * PAGE;
    return i < page.rows.length ? { page, i } : undefined;
  }

  cell(r: number, c: number) {
    const at = this.rowAt(r);
    const col = this.info.columns[c - 1];
    if (!at || !col) return undefined;
    return formatScalar(at.page.rows[at.i][c - 1], col.kind);
  }

  rawValue(r: number, c: number): Scalar | undefined {
    const at = this.rowAt(r);
    return at?.page.rows[at.i][c - 1];
  }

  rowId(r: number): number | undefined {
    const at = this.rowAt(r);
    return at?.page.row_ids[at.i];
  }

  request(r0: number, _c0: number, r1: number) {
    const p0 = Math.floor((r0 - 1) / PAGE);
    const p1 = Math.floor((r1 - 1) / PAGE);
    for (let p = p0; p <= p1; p++) {
      if (this.pages.has(p) || this.inflight.has(p)) continue;
      this.inflight.add(p);
      void this.fetchPage(p, this.generation);
    }
  }

  private async fetchPage(p: number, gen: number) {
    try {
      const t = performance.now();
      const page = await api.dataPage(this.info.name, p * PAGE, PAGE, this.sort, this.filters);
      if (gen !== this.generation) return;
      this.lastQueryMs = performance.now() - t;
      this.total = page.total;
      this.pages.set(p, page);
      this.emit();
    } finally {
      this.inflight.delete(p);
    }
  }

  setView(sort: Sort | null, filters: ColumnFilter[]) {
    this.sort = sort;
    this.filters = filters;
    this.reload();
  }

  reload() {
    this.generation++;
    this.pages.clear();
    this.inflight.clear();
    this.request(1, 1, PAGE);
    this.emit();
  }
}
