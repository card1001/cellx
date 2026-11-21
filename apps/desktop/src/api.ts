// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export type Scalar =
  | { t: "Empty" }
  | { t: "Number"; v: number }
  | { t: "Text"; v: string }
  | { t: "Bool"; v: boolean }
  | { t: "Error"; v: string };

export interface CellStyle {
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  color?: string;
  fill?: string;
  align?: "left" | "right" | "center";
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
  col_widths: number[];
  row_heights: number[];
}

export interface CellDetail {
  input: string;
  text: string;
  bridge_error: string | null;
}

export interface RecalcStats {
  passes: number;
  bridge_calls: number;
  bridge_cache_hits: number;
  calc_ms: number;
  bridge_ms: number;
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
}

export const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  bookInfo: () => invoke<BookInfo>("book_info"),
  newBook: () => invoke<BookInfo>("new_book"),
  openFile: (path: string) => invoke<BookInfo>("open_file", { path }),
  saveFile: (path: string) => invoke<BookInfo>("save_file", { path }),
  addSheet: () => invoke<BookInfo>("add_sheet"),
  renameSheet: (sheet: number, name: string) => invoke<BookInfo>("rename_sheet", { sheet, name }),
  dimensions: (sheet: number) => invoke<[number, number]>("dimensions", { sheet }),
  viewport: (sheet: number, row0: number, col0: number, row1: number, col1: number) =>
    invoke<Viewport>("viewport", { sheet, row0, col0, row1, col1 }),
  cellDetail: (sheet: number, row: number, col: number) =>
    invoke<CellDetail>("cell_detail", { sheet, row, col }),
  setCell: (sheet: number, row: number, col: number, input: string) =>
    invoke<RecalcStats>("set_cell", { sheet, row, col, input }),
  setCells: (sheet: number, cells: [number, number, string][]) =>
    invoke<RecalcStats>("set_cells", { sheet, cells }),
  setColumnWidth: (sheet: number, col: number, px: number) =>
    invoke<void>("set_column_width", { sheet, col, px }),
  undo: () => invoke<[number, number, number] | null>("undo"),
  redo: () => invoke<[number, number, number] | null>("redo"),
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
      return Number.isInteger(s.v) && Math.abs(s.v) < 1e15 ? String(s.v) : String(s.v);
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
