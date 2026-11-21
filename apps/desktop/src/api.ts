// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export type Scalar =
  | { t: "Empty" }
  | { t: "Number"; v: number }
  | { t: "Text"; v: string }
  | { t: "Bool"; v: boolean }
  | { t: "Error"; v: string };

export interface BorderView {
  style: string;
  color: string;
}

export interface CellStyle {
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  color?: string;
  fill?: string;
  align?: "left" | "right" | "center";
  valign?: "top" | "center";
  wrap?: boolean;
  font?: string;
  size?: number;
  border_top?: BorderView;
  border_right?: BorderView;
  border_bottom?: BorderView;
  border_left?: BorderView;
}

export interface CellView {
  row: number;
  col: number;
  text: string;
  numeric: boolean;
  error: boolean;
  bridge: boolean;
  style?: CellStyle;
}

export interface Viewport {
  cells: CellView[];
}

export interface SheetLayout {
  default_row_height: number;
  default_col_width: number;
  rows: [number, number][];
  hidden_rows: number[];
  cols: [number, number, number][];
  hidden_cols: number[];
  merges: [number, number, number, number][];
  frozen_rows: number;
  frozen_cols: number;
  show_grid_lines: boolean;
  last_row: number;
  last_col: number;
}

export interface CurrentStyle {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  font: string;
  size: number;
  align: string;
  valign: string;
  wrap: boolean;
  num_fmt: string;
  merged: boolean;
}

export interface CellDetail {
  input: string;
  text: string;
  bridge_error: string | null;
  style: CurrentStyle;
}

export interface RecalcStats {
  passes: number;
  bridge_calls: number;
  bridge_cache_hits: number;
  calc_ms: number;
  bridge_ms: number;
}

export interface SelectionStats {
  count: number;
  numeric_count: number;
  sum: number;
  average: number | null;
  min: number | null;
  max: number | null;
}

export type ColumnKind = "Number" | "Text" | "Bool" | "Date" | "Timestamp" | "Other";

export interface ColumnInfo {
  name: string;
  sql_type: string;
  kind: ColumnKind;
}

export interface TableInfo {
  name: string;
  rows: number;
  columns: ColumnInfo[];
}

export interface Page {
  columns: string[];
  row_ids: number[];
  rows: Scalar[][];
  total: number;
}

export interface Sort {
  column: string;
  descending: boolean;
}

export interface ColumnFilter {
  column: string;
  criterion: string;
}

export interface BookInfo {
  sheets: string[];
  tables: TableInfo[];
  path: string | null;
  dirty: boolean;
  can_undo: boolean;
  can_redo: boolean;
  font: [string, number];
}

export interface Range {
  r0: number;
  c0: number;
  r1: number;
  c1: number;
}

export type Direction = "Up" | "Down" | "Left" | "Right";

export const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  bookInfo: () => invoke<BookInfo>("book_info"),
  newBook: () => invoke<BookInfo>("new_book"),
  openFile: (path: string) => invoke<BookInfo>("open_file", { path }),
  saveFile: (path: string) => invoke<BookInfo>("save_file", { path }),
  functionNames: () => invoke<string[]>("function_names"),

  addSheet: () => invoke<BookInfo>("add_sheet"),
  renameSheet: (sheet: number, name: string) => invoke<BookInfo>("rename_sheet", { sheet, name }),
  deleteSheet: (sheet: number) => invoke<BookInfo>("delete_sheet", { sheet }),

  layout: (sheet: number) => invoke<SheetLayout>("layout", { sheet }),
  viewport: (sheet: number, row0: number, col0: number, row1: number, col1: number) =>
    invoke<Viewport>("viewport", { sheet, row0, col0, row1, col1 }),
  cellDetail: (sheet: number, row: number, col: number) => invoke<CellDetail>("cell_detail", { sheet, row, col }),
  selectionStats: (sheet: number, range: Range) => invoke<SelectionStats>("selection_stats", { sheet, range }),
  navigateEdge: (sheet: number, row: number, col: number, dir: Direction) =>
    invoke<[number, number]>("navigate_edge", { sheet, row, col, dir }),
  currentRegion: (sheet: number, row: number, col: number) => invoke<Range>("current_region", { sheet, row, col }),
  autosumFormula: (sheet: number, row: number, col: number) => invoke<string>("autosum_formula", { sheet, row, col }),
  find: (sheet: number, query: string, row: number, col: number, matchCase: boolean) =>
    invoke<[number, number] | null>("find", { sheet, query, row, col, matchCase }),

  setCell: (sheet: number, row: number, col: number, input: string) =>
    invoke<RecalcStats>("set_cell", { sheet, row, col, input }),
  setCells: (sheet: number, cells: [number, number, string][]) => invoke<RecalcStats>("set_cells", { sheet, cells }),
  fillRange: (sheet: number, range: Range, input: string) => invoke<RecalcStats>("fill_range", { sheet, range, input }),
  clear: (sheet: number, range: Range, what: "contents" | "formats" | "all") =>
    invoke<RecalcStats>("clear", { sheet, range, what }),
  copy: (sheet: number, range: Range, cut: boolean) => invoke<string>("copy", { sheet, range, cut }),
  clearClipboard: () => invoke<void>("clear_clipboard"),
  paste: (sheet: number, row: number, col: number, text: string, internal: boolean) =>
    invoke<RecalcStats>("paste", { sheet, row, col, text, internal }),
  autoFill: (sheet: number, source: Range, target: Range) => invoke<RecalcStats>("auto_fill", { sheet, source, target }),
  undo: () => invoke<RecalcStats>("undo"),
  redo: () => invoke<RecalcStats>("redo"),

  insertRows: (sheet: number, at: number, count: number) => invoke<RecalcStats>("insert_rows", { sheet, at, count }),
  deleteRows: (sheet: number, at: number, count: number) => invoke<RecalcStats>("delete_rows", { sheet, at, count }),
  insertColumns: (sheet: number, at: number, count: number) =>
    invoke<RecalcStats>("insert_columns", { sheet, at, count }),
  deleteColumns: (sheet: number, at: number, count: number) =>
    invoke<RecalcStats>("delete_columns", { sheet, at, count }),
  setColumnWidth: (sheet: number, c0: number, c1: number, px: number) =>
    invoke<void>("set_column_width", { sheet, c0, c1, px }),
  setRowHeight: (sheet: number, r0: number, r1: number, px: number) =>
    invoke<void>("set_row_height", { sheet, r0, r1, px }),
  setHidden: (sheet: number, rows: boolean, from: number, to: number, hidden: boolean) =>
    invoke<void>("set_hidden", { sheet, rows, from, to, hidden }),
  setFrozen: (sheet: number, rows: number, cols: number) => invoke<void>("set_frozen", { sheet, rows, cols }),
  setShowGridLines: (sheet: number, show: boolean) => invoke<void>("set_show_grid_lines", { sheet, show }),

  setStyle: (sheet: number, range: Range, path: string, value: string) =>
    invoke<RecalcStats>("set_style", { sheet, range, path, value }),
  setBorder: (sheet: number, range: Range, kind: string, style: string, color: string) =>
    invoke<RecalcStats>("set_border", { sheet, range, kind, style, color }),
  merge: (sheet: number, range: Range, center: boolean) => invoke<RecalcStats>("merge", { sheet, range, center }),
  unmerge: (sheet: number, range: Range) => invoke<RecalcStats>("unmerge", { sheet, range }),

  importData: (path: string, name: string) => invoke<TableInfo>("import_data", { path, name }),
  queryToTable: (sql: string, name: string) => invoke<TableInfo>("query_to_table", { sql, name }),
  dataPage: (table: string, offset: number, limit: number, sort: Sort | null, filters: ColumnFilter[]) =>
    invoke<Page>("data_page", { table, offset, limit, sort, filters }),
  updateDataCell: (table: string, rid: number, column: string, text: string) =>
    invoke<RecalcStats>("update_data_cell", { table, rid, column, text }),
  exportTable: (table: string, path: string) => invoke<void>("export_table", { table, path }),
  dropTable: (table: string) => invoke<BookInfo>("drop_table", { table }),
};

export function scalarText(s: Scalar): string {
  switch (s.t) {
    case "Empty":
      return "";
    case "Number":
      return String(s.v);
    case "Bool":
      return s.v ? "TRUE" : "FALSE";
    default:
      return s.v;
  }
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
