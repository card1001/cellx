// Data sources for the grid: regular sheets (calculation engine) and data sheets (DuckDB).
import {
  api,
  type CellStyle,
  type ColumnFilter,
  type ColumnKind,
  type Memo,
  type Page,
  type Scalar,
  type SheetLayout,
  type Sort,
  type TableInfo,
} from "../api";
import { Axis } from "./axis";
import { colName, MAX_COLS, MAX_ROWS } from "./model";

export interface CellData {
  text: string;
  numeric?: boolean;
  error?: boolean;
  bridge?: boolean;
  link?: boolean;
  style?: CellStyle;
  /** The number, when shown in the General format: fitted to the column width. */
  general?: number;
}

export interface Merge {
  r: number;
  c: number;
  h: number;
  w: number;
}

export interface GridSource {
  readonly kind: "sheet" | "data";
  rows: Axis;
  cols: Axis;
  frozenRows: number;
  frozenCols: number;
  showGrid: boolean;
  /** Workbook default font: [family, size in points]. */
  font: [string, number];
  colHeader(c: number): string;
  colSubHeader?(c: number): string;
  rowHeader(r: number): string;
  cell(r: number, c: number): CellData | undefined;
  /** The merged range a cell belongs to, if any. */
  mergeAt(r: number, c: number): Merge | undefined;
  /** Cells with memos (sheets only). */
  memos?(): Memo[];
  request(r0: number, c0: number, r1: number, c1: number): void;
  grow?(r: number, c: number): void;
  subscribe(cb: () => void): () => void;
}

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
  readonly kind = "sheet" as const;
  rows: Axis;
  cols: Axis;
  frozenRows = 0;
  frozenCols = 0;
  showGrid = true;
  /** 수식 표시 (Ctrl+`): formula cells show their formula. */
  showFormulas = false;
  lastRow = 0;
  lastCol = 0;
  private cells = new Map<number, CellData>();
  private merges: Merge[] = [];
  private mergeIndex = new Map<number, Merge>();
  private loaded = new Set<string>();
  private inflight = new Set<string>();
  private generation = 0;
  private last: [number, number, number, number] = [1, 1, 40, 20];
  private memoList: Memo[] = [];
  private memoIndex = new Map<number, Memo>();

  constructor(
    readonly sheet: number,
    layout: SheetLayout,
    public font: [string, number],
  ) {
    super();
    this.rows = new Axis(200, layout.default_row_height);
    this.cols = new Axis(26, layout.default_col_width);
    this.applyLayout(layout);
    void this.loadMemos();
  }

  memos() {
    return this.memoList;
  }

  memoAt(r: number, c: number): Memo | undefined {
    return this.memoIndex.get(this.key(r, c));
  }

  private async loadMemos() {
    try {
      this.memoList = await api.memos(this.sheet);
      this.memoIndex = new Map(this.memoList.map((m) => [this.key(m.row, m.col), m]));
      this.emit();
    } catch {
      /* the sheet may be gone */
    }
  }

  private key(r: number, c: number) {
    return r * 20000 + c;
  }

  applyLayout(l: SheetLayout) {
    const rowCount = Math.min(MAX_ROWS, Math.max(this.rows.count, 200, l.last_row + 100));
    const colCount = Math.min(MAX_COLS, Math.max(this.cols.count, 26, l.last_col + 5));
    this.rows = new Axis(rowCount, l.default_row_height, l.rows, l.hidden_rows);
    const colSizes: [number, number][] = [];
    for (const [a, b, w] of l.cols) {
      // Ranges like 1..16384 with one width are common in xlsx; keep them compact.
      const end = Math.min(b, Math.max(colCount, a + 200));
      for (let c = a; c <= end; c++) colSizes.push([c, w]);
    }
    this.cols = new Axis(colCount, l.default_col_width, colSizes, l.hidden_cols);
    this.frozenRows = l.frozen_rows;
    this.frozenCols = l.frozen_cols;
    this.showGrid = l.show_grid_lines;
    this.lastRow = l.last_row;
    this.lastCol = l.last_col;
    this.merges = l.merges.map(([r, c, h, w]) => ({ r, c, h, w }));
    this.mergeIndex.clear();
    for (const m of this.merges) {
      if (m.h * m.w > 100_000) continue;
      for (let r = m.r; r < m.r + m.h; r++) for (let c = m.c; c < m.c + m.w; c++) this.mergeIndex.set(this.key(r, c), m);
    }
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
  mergeAt(r: number, c: number) {
    return this.mergeIndex.get(this.key(r, c));
  }

  grow(r: number, c: number) {
    let changed = false;
    if (r > this.rows.count - 50 && this.rows.count < MAX_ROWS) {
      this.rows.count = Math.min(MAX_ROWS, r + 500);
      changed = true;
    }
    if (c > this.cols.count - 3 && this.cols.count < MAX_COLS) {
      this.cols.count = Math.min(MAX_COLS, c + 10);
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
      const vp = await api.viewport(this.sheet, r0, c0, r1, c1, this.showFormulas);
      if (gen !== this.generation) return;
      for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) this.cells.delete(this.key(r, c));
      for (const cell of vp.cells) this.cells.set(this.key(cell.row, cell.col), cell);
      this.loaded.add(k);
      this.emit();
    } finally {
      this.inflight.delete(k);
    }
  }

  setShowFormulas(on: boolean) {
    if (this.showFormulas === on) return;
    this.showFormulas = on;
    this.generation++;
    this.loaded.clear();
    this.inflight.clear();
    const [r0, c0, r1, c1] = this.last;
    this.request(r0, c0, r1, c1);
  }

  /** Reloads layout and the cells on screen after an edit. */
  async invalidate() {
    this.generation++;
    this.loaded.clear();
    this.inflight.clear();
    const [r0, c0, r1, c1] = this.last;
    this.request(r0, c0, r1, c1);
    this.applyLayout(await api.layout(this.sheet));
    await this.loadMemos();
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
  readonly kind = "data" as const;
  rows: Axis;
  cols: Axis;
  frozenRows = 0;
  frozenCols = 0;
  showGrid = true;
  private pages = new Map<number, Page>();
  private inflight = new Set<number>();
  private generation = 0;
  // Unsorted scans use stable _rid cursors. Requests for a later viewport page
  // advance sequentially so the virtual row index remains deterministic.
  private keysetRequested = new Set<number>();
  private keysetLoading = false;
  private keysetNextPage = 0;
  private keysetCursor: number | null = null;
  total: number;
  sort: Sort | null = null;
  filters: ColumnFilter[] = [];
  lastQueryMs = 0;

  constructor(
    public info: TableInfo,
    public font: [string, number],
  ) {
    super();
    this.total = info.rows;
    this.rows = new Axis(Math.max(1, info.rows), 22);
    const widths: [number, number][] = info.columns.map((col, i) => {
      const byKind = col.kind === "Text" ? 140 : col.kind === "Timestamp" ? 150 : 110;
      return [i + 1, Math.max(byKind, col.name.length * 8 + 60)];
    });
    this.cols = new Axis(Math.max(1, info.columns.length), 100, widths);
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
  mergeAt() {
    return undefined;
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
    if (!this.sort) {
      for (let p = p0; p <= p1; p++) {
        if (!this.pages.has(p) && p >= this.keysetNextPage) this.keysetRequested.add(p);
      }
      void this.drainKeysetPages(this.generation);
      return;
    }
    for (let p = p0; p <= p1; p++) {
      if (this.pages.has(p) || this.inflight.has(p)) continue;
      this.inflight.add(p);
      void this.fetchOffsetPage(p, this.generation);
    }
  }

  private async fetchOffsetPage(p: number, gen: number) {
    try {
      const t = performance.now();
      const page = await api.dataPage(this.info.name, p * PAGE, PAGE, this.sort, this.filters);
      if (gen !== this.generation) return;
      this.lastQueryMs = performance.now() - t;
      this.applyPage(p, page);
    } finally {
      this.inflight.delete(p);
    }
  }

  private applyPage(p: number, page: Page) {
    if (page.total !== this.total) {
      this.total = page.total;
      this.rows.count = Math.max(1, page.total);
    }
    this.pages.set(p, page);
    this.emit();
  }

  private async drainKeysetPages(gen: number) {
    if (this.keysetLoading) return;
    this.keysetLoading = true;
    try {
      while (gen === this.generation && this.keysetRequested.size) {
        const target = Math.max(...this.keysetRequested);
        if (this.pages.has(this.keysetNextPage)) {
          this.keysetNextPage++;
          continue;
        }
        const pageNumber = this.keysetNextPage;
        const t = performance.now();
        const page = await api.dataPageAfterRid(this.info.name, this.keysetCursor, PAGE, this.filters);
        if (gen !== this.generation) return;
        this.lastQueryMs = performance.now() - t;
        this.applyPage(pageNumber, page);
        this.keysetRequested.delete(pageNumber);
        this.keysetCursor = page.row_ids.length ? page.row_ids[page.row_ids.length - 1] : this.keysetCursor;
        this.keysetNextPage++;
        if (page.rows.length < PAGE) {
          // No further rows exist; avoid repeatedly querying beyond the end.
          this.keysetRequested.clear();
          break;
        }
        if (pageNumber >= target && !this.keysetRequested.size) break;
      }
    } finally {
      this.keysetLoading = false;
      if (gen === this.generation && this.keysetRequested.size) void this.drainKeysetPages(gen);
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
    this.keysetRequested.clear();
    this.keysetLoading = false;
    this.keysetNextPage = 0;
    this.keysetCursor = null;
    this.request(1, 1, PAGE);
    this.emit();
  }
}
