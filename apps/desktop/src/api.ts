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
  /** Conditional formatting data bar. */
  bar?: { value: number; axis: number; color: string; negative: string; gradient: boolean; show_value: boolean };
  /** Conditional formatting icon or rating. */
  icon?: { name: string; color: string; count: number; max: number; show_value: boolean };
}

export interface Memo {
  row: number;
  col: number;
  text: string;
  author: string;
}

export type ChartKind = "Column" | "Bar" | "Line" | "Pie" | "Doughnut" | "Area" | "Scatter";

export interface ChartAnchor {
  row: number;
  col: number;
  dx: number;
  dy: number;
  width: number;
  height: number;
}

export interface RecoveredFile {
  id: string;
  original: string | null;
  name: string;
  /** Seconds since 1970. */
  saved_at: number;
}

/** A range a trace arrow starts or ends at (1-based, inclusive). */
export interface TraceArea {
  sheet: number;
  row1: number;
  col1: number;
  row2: number;
  col2: number;
}

/** One step of 수식 계산: `text` marks the next part with \u0002 … \u0003. */
export interface EvalStep {
  text: string;
  value: string;
}

export type ShapeKind =
  | "rect"
  | "roundRect"
  | "ellipse"
  | "triangle"
  | "diamond"
  | "rightArrow"
  | "pentagon"
  | "line"
  | "arrow"
  | "textBox";

export interface Shape {
  id: number;
  sheet: number;
  name: string;
  kind: ShapeKind;
  anchor: ChartAnchor;
  fill: string | null;
  line: string | null;
  line_width: number;
  text: string;
  text_color: string;
  font_size: number;
  bold: boolean;
  align: "l" | "ctr" | "r";
  valign: "t" | "ctr" | "b";
  flip_v: boolean;
  flip_h: boolean;
}

export type SparkKind = "line" | "column" | "winLoss";

export interface SparklineGroup {
  id: number;
  sheet: number;
  kind: SparkKind;
  color: string;
  negative_color: string;
  markers: boolean;
  high_low: boolean;
  items: { row: number; col: number; range: string }[];
}

export interface SparklineView {
  group: number;
  row: number;
  col: number;
  kind: SparkKind;
  color: string;
  negative_color: string;
  markers: boolean;
  high_low: boolean;
  values: (number | null)[];
}

export interface Slicer {
  id: number;
  sheet: number;
  name: string;
  caption: string;
  table: string;
  column: string;
  anchor: ChartAnchor;
  columns: number;
}

export interface SlicerView {
  slicer: Slicer;
  items: { text: string; selected: boolean; available: boolean }[];
  filtered: boolean;
}

export interface Scenario {
  sheet: number;
  name: string;
  comment: string;
  cells: [number, number, string][];
}

export interface Picture {
  id: number;
  sheet: number;
  name: string;
  mime: string;
  anchor: ChartAnchor;
  /** Width ÷ height of the image itself. */
  ratio: number;
}

export interface Chart {
  id: number;
  sheet: number;
  kind: ChartKind;
  title: string;
  range: string;
  series_in_rows: boolean;
  legend: boolean;
  trendline?: boolean;
  anchor: ChartAnchor;
}

export interface ChartData {
  title: string;
  categories: string[];
  x_values: (number | null)[] | null;
  series: { name: string; values: (number | null)[] }[];
  sheet: number;
}

export type Aggregate = "Sum" | "Count" | "Average" | "Max" | "Min";
export type PivotSource = { kind: "Range"; range: string } | { kind: "Table"; name: string };

export interface PivotDef {
  id: number;
  source: PivotSource;
  rows: string[];
  columns: string | null;
  values: { field: string; agg: Aggregate }[];
  sheet: number;
  row: number;
  col: number;
  extent: [number, number];
}

export type PasteWhat = "All" | "Formulas" | "Values" | "Formats" | "ColumnWidths";
export type PasteOp = "None" | "Add" | "Subtract" | "Multiply" | "Divide";
export interface PasteSpecial {
  what: PasteWhat;
  op: PasteOp;
  skip_blanks: boolean;
  transpose: boolean;
}

export interface SortKey {
  col: number;
  descending: boolean;
}

export type FilterOp =
  | "Equal"
  | "NotEqual"
  | "GreaterThan"
  | "GreaterThanOrEqual"
  | "LessThan"
  | "LessThanOrEqual"
  | "Contains"
  | "NotContains"
  | "BeginsWith"
  | "EndsWith";

export interface SheetColumnFilter {
  col: number;
  values: string[] | null;
  conditions: { op: FilterOp; value: string }[];
  and: boolean;
}

export interface AutoFilter {
  sheet: number;
  range: Range;
  columns: SheetColumnFilter[];
}

export interface FilterValue {
  text: string;
  checked: boolean;
}

export type ValidationKind = "Any" | "WholeNumber" | "Decimal" | "List" | "Date" | "Time" | "TextLength" | "Custom";
export type ValidationOp =
  | "Between"
  | "NotBetween"
  | "Equal"
  | "NotEqual"
  | "GreaterThan"
  | "LessThan"
  | "GreaterThanOrEqual"
  | "LessThanOrEqual";
export type ErrorStyle = "Stop" | "Warning" | "Information";

export interface Validation {
  sheet: number;
  range: string;
  kind: ValidationKind;
  op: ValidationOp;
  formula1: string;
  formula2: string;
  allow_blank: boolean;
  in_cell_dropdown: boolean;
  show_input: boolean;
  input_title: string;
  input_message: string;
  show_error: boolean;
  error_style: ErrorStyle;
  error_title: string;
  error_message: string;
}

export interface ValidationFailure {
  style: ErrorStyle;
  title: string;
  message: string;
}

export interface SplitOptions {
  delimiters: string;
  consecutive: boolean;
  qualifier: string | null;
  widths: number[] | null;
  dest_row: number;
  dest_col: number;
}

export interface NameView {
  name: string;
  scope: number | null;
  formula: string;
}

export interface LinkView {
  target: string;
  internal: boolean;
  tooltip: string | null;
}

export interface GoalSeekResult {
  found: boolean;
  value: number;
  result: number;
  iterations: number;
}

export interface OutlineView {
  rows: [number, number][];
  cols: [number, number][];
}

export type SubtotalFn = "Sum" | "Count" | "Average" | "Max" | "Min";

export interface TableView {
  name: string;
  sheet: number;
  range: Range;
  style: string;
  stripes: boolean;
  header: boolean;
  totals: boolean;
  columns: string[];
}

export interface ProtectOptions {
  format_cells: boolean;
  format_columns: boolean;
  format_rows: boolean;
  insert_rows: boolean;
  insert_columns: boolean;
  delete_rows: boolean;
  delete_columns: boolean;
  sort: boolean;
  auto_filter: boolean;
}

export interface ProtectionView {
  protected: boolean;
  has_password: boolean;
  options: ProtectOptions;
  unlocked: Range[];
}

export type FitMode = "None" | "Width" | "Page";

export interface PageSetup {
  sheet: number;
  landscape: boolean;
  fit: FitMode;
  gridlines: boolean;
  headings: boolean;
  title_rows: [number, number] | null;
  row_breaks: number[];
  col_breaks: number[];
  page_numbers: boolean;
  /** 인쇄 영역 */
  print_area: Range | null;
  /** 머리글 and 바닥글: left, center, right, with Excel's codes (&P, &N, &D, &T, &F, &A). */
  header: [string, string, string];
  footer: [string, string, string];
}

/** IronCalc's conditional formatting rule input (tagged by `type`). */
export type CfRule = { type: string; [k: string]: unknown };
export interface CfView {
  index: number;
  range: string;
  cf_rule: { [k: string]: unknown } | string;
  priority: number;
}

export interface CellView {
  row: number;
  col: number;
  text: string;
  numeric: boolean;
  error: boolean;
  bridge: boolean;
  link?: boolean;
  style?: CellStyle;
  /** The number, when shown in the General format: fitted to the column width. */
  general?: number;
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
  /** The data engine results did not settle: some values may be out of date. */
  not_converged?: boolean;
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

export type TransformValue =
  | { type: "text"; value: string }
  | { type: "number"; value: number }
  | { type: "boolean"; value: boolean };

export type CompareOperator = "equal" | "not_equal" | "greater_than" | "greater_than_or_equal" | "less_than" | "less_than_or_equal";
export type FilterExpr =
  | { op: "compare"; column: string; operator: CompareOperator; value: TransformValue }
  | { op: "is_null"; column: string; negated: boolean }
  | { op: "and" | "or"; conditions: FilterExpr[] }
  | { op: "not"; expression: FilterExpr };

export type CastType = "text" | "integer" | "big_int" | "double" | "boolean" | "date" | "timestamp";
export interface TransformSortKey { column: string; descending: boolean }
export type JoinKind = "inner" | "left";
export type AggregateFunction = "count" | "sum" | "average" | "min" | "max";
export interface AggregateSpec { function: AggregateFunction; column: string | null; as_name: string }
export type TransformStep =
  | { type: "select_columns"; columns: string[] }
  | { type: "filter"; predicate: FilterExpr }
  | { type: "rename_column"; from: string; to: string }
  | { type: "cast_column"; column: string; target: CastType }
  | { type: "sort"; keys: TransformSortKey[] }
  | { type: "limit"; rows: number }
  | { type: "join"; table: string; left_column: string; right_column: string; kind: JoinKind; suffix: string }
  | { type: "group_by"; columns: string[]; aggregates: AggregateSpec[] };
export interface TransformPipeline { source_table: string; steps: TransformStep[] }
export interface SavedPipeline {
  id: string;
  name: string;
  pipeline: TransformPipeline;
  last_incremental_source_rid?: number | null;
  incremental_output_table?: string | null;
}
export interface SavedConnector {
  id: string; name: string; path: string; table_name: string; locale: string;
  schema: [string, string][];
  fingerprint: string;
}
export interface ConnectorStatus { id: string; available: boolean; changed: boolean }
export interface TransformPreview { columns: string[]; rows: Scalar[][]; truncated: boolean }
export interface ColumnProfile {
  name: string;
  sql_type: string;
  null_count: number;
  empty_count: number;
  distinct_count: number;
  min: Scalar | null;
  max: Scalar | null;
}
export interface TableProfile {
  table: string;
  row_count: number;
  columns: ColumnProfile[];
}
export interface TransformJobSnapshot {
  id: string;
  pipeline_id: string;
  output_table: string;
  incremental: boolean;
  pipeline: TransformPipeline | null;
  started_at_ms: number;
  finished_at_ms: number | null;
  result_rows: number | null;
  status: "queued" | "running" | "cancelling" | "completed" | "cancelled" | "failed";
  phase: string;
  result: TableInfo | null;
  error: string | null;
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

export interface SheetTab {
  name: string;
  hidden: boolean;
  /** #RRGGBB */
  color: string | null;
}

export interface BookInfo {
  sheets: string[];
  tables: TableInfo[];
  path: string | null;
  dirty: boolean;
  tabs: SheetTab[];
  can_undo: boolean;
  can_redo: boolean;
  font: [string, number];
  protected: boolean[];
  book_protected: boolean;
  /** 1904 날짜 체계 */
  date1904: boolean;
  book_password: boolean;
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
  viewport: (sheet: number, row0: number, col0: number, row1: number, col1: number, formulas = false) =>
    invoke<Viewport>("viewport", { sheet, row0, col0, row1, col1, formulas }),
  tracePrecedents: (sheet: number, row: number, col: number) => invoke<TraceArea[]>("trace_precedents", { sheet, row, col }),
  traceDependents: (sheet: number, row: number, col: number) => invoke<TraceArea[]>("trace_dependents", { sheet, row, col }),
  shapes: (sheet: number) => invoke<Shape[]>("shapes", { sheet }),
  addShape: (sheet: number, row: number, col: number, kind: ShapeKind) => invoke<Shape>("add_shape", { sheet, row, col, kind }),
  updateShape: (shape: Shape) => invoke<void>("update_shape", { shape }),
  deleteShape: (id: number) => invoke<void>("delete_shape", { id }),
  sparklines: (sheet: number) => invoke<SparklineView[]>("sparklines", { sheet }),
  sparklineGroups: (sheet: number) => invoke<SparklineGroup[]>("sparkline_groups", { sheet }),
  addSparklines: (sheet: number, kind: SparkKind, data: string, location: Range) =>
    invoke<SparklineGroup>("add_sparklines", { sheet, kind, data, location }),
  updateSparklineGroup: (group: SparklineGroup) => invoke<void>("update_sparkline_group", { group }),
  deleteSparklines: (sheet: number, range: Range) => invoke<void>("delete_sparklines", { sheet, range }),
  slicers: (sheet: number) => invoke<SlicerView[]>("slicers", { sheet }),
  addSlicer: (table: string, column: string, sheet: number) => invoke<Slicer>("add_slicer", { table, column, sheet }),
  updateSlicer: (slicer: Slicer) => invoke<void>("update_slicer", { slicer }),
  deleteSlicer: (id: number) => invoke<void>("delete_slicer", { id }),
  slicerSelect: (id: number, values: string[] | null) => invoke<RecalcStats>("slicer_select", { id, values }),
  addDataTable: (sheet: number, range: Range, rowInput: string | null, colInput: string | null) =>
    invoke<RecalcStats>("add_data_table", { sheet, range, rowInput, colInput }),
  deleteDataTable: (sheet: number, row: number, col: number) => invoke<RecalcStats>("delete_data_table", { sheet, row, col }),
  dataTableAt: (sheet: number, row: number, col: number) => invoke<boolean>("data_table_at", { sheet, row, col }),
  scenarios: (sheet: number) => invoke<Scenario[]>("scenarios", { sheet }),
  setScenario: (scenario: Scenario) => invoke<void>("set_scenario", { scenario }),
  deleteScenario: (sheet: number, name: string) => invoke<void>("delete_scenario", { sheet, name }),
  showScenario: (sheet: number, name: string) => invoke<RecalcStats>("show_scenario", { sheet, name }),
  scenarioSummary: (sheet: number, results: [number, number][]) => invoke<number>("scenario_summary", { sheet, results }),
  protectBook: (password: string | null) => invoke<void>("protect_book", { password }),
  setDate1904: (on: boolean) => invoke<RecalcStats>("set_date1904", { on }),
  unprotectBook: (password: string | null) => invoke<void>("unprotect_book", { password }),
  customViews: () => invoke<{ name: string; active_sheet: number }[]>("custom_views"),
  addCustomView: (name: string, active: number) => invoke<unknown>("add_custom_view", { name, active }),
  deleteCustomView: (name: string) => invoke<void>("delete_custom_view", { name }),
  showCustomView: (name: string) => invoke<number>("show_custom_view", { name }),
  formulaSteps: (sheet: number, row: number, col: number) => invoke<EvalStep[]>("formula_steps", { sheet, row, col }),
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
  memos: (sheet: number) => invoke<Memo[]>("memos", { sheet }),
  setMemo: (sheet: number, row: number, col: number, text: string | null) =>
    invoke<RecalcStats>("set_memo", { sheet, row, col, text }),
  clearMemos: (sheet: number, range: Range) => invoke<RecalcStats>("clear_memos", { sheet, range }),
  moveSheet: (from: number, to: number) => invoke<BookInfo>("move_sheet", { from, to }),
  copySheet: (sheet: number, to: number) => invoke<BookInfo>("copy_sheet", { sheet, to }),
  setSheetColor: (sheet: number, color: string | null) => invoke<BookInfo>("set_sheet_color", { sheet, color }),
  setSheetHidden: (sheet: number, hidden: boolean) => invoke<BookInfo>("set_sheet_hidden", { sheet, hidden }),
  autosave: () => invoke<boolean>("autosave"),
  recoveryList: () => invoke<RecoveredFile[]>("recovery_list"),
  recoveryOpen: (id: string) => invoke<BookInfo>("recovery_open", { id }),
  recoveryDiscard: (id: string) => invoke<void>("recovery_discard", { id }),
  pictures: (sheet: number) => invoke<Picture[]>("pictures", { sheet }),
  pictureData: (id: number) => invoke<string>("picture_data", { id }),
  addPictureFile: (sheet: number, row: number, col: number, path: string) => invoke<Picture>("add_picture_file", { sheet, row, col, path }),
  addPictureData: (sheet: number, row: number, col: number, data: string) => invoke<Picture>("add_picture_data", { sheet, row, col, data }),
  updatePicture: (p: Picture) => invoke<void>("update_picture", { id: p.id, name: p.name, anchor: p.anchor }),
  deletePicture: (id: number) => invoke<void>("delete_picture", { id }),
  charts: (sheet: number) => invoke<Chart[]>("charts", { sheet }),
  addChart: (sheet: number, kind: ChartKind, range: Range) => invoke<Chart>("add_chart", { sheet, kind, range }),
  updateChart: (chart: Chart) => invoke<void>("update_chart", { chart }),
  deleteChart: (id: number) => invoke<void>("delete_chart", { id }),
  chartData: (id: number) => invoke<ChartData>("chart_data", { id }),
  conditionalFormats: (sheet: number) => invoke<CfView[]>("conditional_formats", { sheet }),
  addConditionalFormat: (sheet: number, range: Range, rule: CfRule) =>
    invoke<RecalcStats>("add_conditional_format", { sheet, range, rule }),
  deleteConditionalFormat: (sheet: number, index: number) => invoke<RecalcStats>("delete_conditional_format", { sheet, index }),
  clearConditionalFormats: (sheet: number, range: Range | null) =>
    invoke<RecalcStats>("clear_conditional_formats", { sheet, range }),
  pasteSpecial: (sheet: number, row: number, col: number, options: PasteSpecial) =>
    invoke<RecalcStats>("paste_special", { sheet, row, col, options }),
  pivotFields: (source: PivotSource, sheet: number) => invoke<string[]>("pivot_fields", { source, sheet }),
  pivotAt: (sheet: number, row: number, col: number) => invoke<PivotDef | null>("pivot_at", { sheet, row, col }),
  savePivot: (def: PivotDef, newSheet: boolean) => invoke<PivotDef>("save_pivot", { def, newSheet }),
  refreshPivots: () => invoke<RecalcStats>("refresh_pivots"),
  deletePivot: (id: number) => invoke<RecalcStats>("delete_pivot", { id }),
  sortRange: (sheet: number, range: Range, keys: SortKey[], hasHeader: boolean) =>
    invoke<RecalcStats>("sort_range", { sheet, range, keys, hasHeader }),
  toggleAutoFilter: (sheet: number, range: Range) => invoke<AutoFilter | null>("toggle_auto_filter", { sheet, range }),
  autoFilter: (sheet: number) => invoke<AutoFilter | null>("auto_filter", { sheet }),
  setFilterColumn: (sheet: number, filter: SheetColumnFilter) => invoke<RecalcStats>("set_filter_column", { sheet, filter }),
  clearFilters: (sheet: number) => invoke<RecalcStats>("clear_filters", { sheet }),
  applyFilter: (sheet: number) => invoke<RecalcStats>("apply_filter", { sheet }),
  filterValues: (sheet: number, col: number) => invoke<FilterValue[]>("filter_values", { sheet, col }),
  textToColumns: (sheet: number, range: Range, options: SplitOptions) => invoke<RecalcStats>("text_to_columns", { sheet, range, options }),
  removeDuplicates: (sheet: number, range: Range, cols: number[], hasHeader: boolean) =>
    invoke<{ removed: number; remaining: number }>("remove_duplicates", { sheet, range, cols, hasHeader }),
  validationAt: (sheet: number, row: number, col: number) => invoke<Validation | null>("validation_at", { sheet, row, col }),
  setValidation: (sheet: number, range: Range, validation: Validation) => invoke<RecalcStats>("set_validation", { sheet, range, validation }),
  clearValidation: (sheet: number, range: Range) => invoke<RecalcStats>("clear_validation", { sheet, range }),
  validateInput: (sheet: number, row: number, col: number, input: string) =>
    invoke<ValidationFailure | null>("validate_input", { sheet, row, col, input }),
  listItems: (sheet: number, row: number, col: number) => invoke<string[] | null>("list_items", { sheet, row, col }),
  flashFill: (sheet: number, row: number, col: number) => invoke<[number, RecalcStats]>("flash_fill", { sheet, row, col }),
  replaceAll: (sheet: number | null, find: string, replace: string, matchCase: boolean, wholeCell: boolean) =>
    invoke<[number, RecalcStats]>("replace_all", { sheet, find, replace, matchCase, wholeCell }),
  replaceInCell: (sheet: number, row: number, col: number, find: string, replace: string, matchCase: boolean, wholeCell: boolean) =>
    invoke<boolean>("replace_in_cell", { sheet, row, col, find, replace, matchCase, wholeCell }),
  names: () => invoke<NameView[]>("names"),
  addName: (name: string, scope: number | null, formula: string) => invoke<void>("add_name", { name, scope, formula }),
  updateName: (name: string, scope: number | null, newName: string, newScope: number | null, formula: string) =>
    invoke<void>("update_name", { name, scope, newName, newScope, formula }),
  deleteName: (name: string, scope: number | null) => invoke<void>("delete_name", { name, scope }),
  linkAt: (sheet: number, row: number, col: number) => invoke<LinkView | null>("link_at", { sheet, row, col }),
  setLink: (sheet: number, row: number, col: number, target: string, internal: boolean, text: string | null) =>
    invoke<RecalcStats>("set_link", { sheet, row, col, target, internal, text }),
  deleteLink: (sheet: number, row: number, col: number) => invoke<RecalcStats>("delete_link", { sheet, row, col }),
  goalSeek: (sheet: number, setCell: [number, number], target: number, changing: [number, number]) =>
    invoke<GoalSeekResult>("goal_seek", { sheet, setCell, target, changing }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  printPage: () => invoke<void>("print_page"),
  outline: (sheet: number) => invoke<OutlineView>("outline", { sheet }),
  group: (sheet: number, rows: boolean, from: number, to: number) => invoke<RecalcStats>("group", { sheet, rows, from, to }),
  ungroup: (sheet: number, rows: boolean, from: number, to: number) => invoke<RecalcStats>("ungroup", { sheet, rows, from, to }),
  clearOutline: (sheet: number) => invoke<RecalcStats>("clear_outline", { sheet }),
  toggleGroup: (sheet: number, rows: boolean, index: number, level: number, collapse: boolean) =>
    invoke<RecalcStats>("toggle_group", { sheet, rows, index, level, collapse }),
  showOutlineLevel: (sheet: number, rows: boolean, level: number) => invoke<RecalcStats>("show_outline_level", { sheet, rows, level }),
  subtotal: (sheet: number, range: Range, options: { group_col: number; func: SubtotalFn; value_cols: number[]; replace: boolean }) =>
    invoke<RecalcStats>("subtotal", { sheet, range, options }),
  removeSubtotals: (sheet: number, range: Range) => invoke<RecalcStats>("remove_subtotals", { sheet, range }),
  tablesOn: (sheet: number) => invoke<TableView[]>("tables_on", { sheet }),
  createTable: (sheet: number, range: Range, hasHeader: boolean, style: string) => invoke<TableView>("create_table", { sheet, range, hasHeader, style }),
  updateTable: (update: { name: string; new_name: string; style: string; stripes: boolean; totals: boolean }) =>
    invoke<RecalcStats>("update_table", { update }),
  convertTableToRange: (name: string) => invoke<RecalcStats>("convert_table_to_range", { name }),
  protection: (sheet: number) => invoke<ProtectionView>("protection", { sheet }),
  protectSheet: (sheet: number, password: string | null, options: ProtectOptions) => invoke<void>("protect_sheet", { sheet, password, options }),
  unprotectSheet: (sheet: number, password: string | null) => invoke<void>("unprotect_sheet", { sheet, password }),
  setLocked: (sheet: number, range: Range, locked: boolean) => invoke<RecalcStats>("set_locked", { sheet, range, locked }),
  pageSetup: (sheet: number) => invoke<PageSetup>("page_setup", { sheet }),
  setPageSetup: (setup: PageSetup) => invoke<void>("set_page_setup", { setup }),
  insertPageBreak: (sheet: number, row: number, col: number) => invoke<void>("insert_page_break", { sheet, row, col }),
  removePageBreak: (sheet: number, row: number, col: number) => invoke<void>("remove_page_break", { sheet, row, col }),
  resetPageBreaks: (sheet: number) => invoke<void>("reset_page_breaks", { sheet }),
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

  importData: (path: string, name: string, locale: string) => invoke<TableInfo>("import_data", { path, name, locale }),
  queryToTable: (sql: string, name: string) => invoke<TableInfo>("query_to_table", { sql, name }),
  profileDataTable: (table: string) => invoke<TableProfile>("profile_data_table", { table }),
  transformPipelines: () => invoke<SavedPipeline[]>("transform_pipelines"),
  saveTransformPipeline: (saved: SavedPipeline) => invoke<SavedPipeline[]>("save_transform_pipeline", { saved }),
  deleteTransformPipeline: (id: string) => invoke<SavedPipeline[]>("delete_transform_pipeline", { id }),
  previewTransform: (pipeline: TransformPipeline) => invoke<TransformPreview>("preview_transform", { pipeline }),
  executeTransformPipeline: (id: string, outputTable: string) =>
    invoke<TableInfo>("execute_transform_pipeline", { id, outputTable }),
  startTransformJob: (
    pipelineId: string,
    outputTable: string,
    incremental = false,
    pipelineOverride: TransformPipeline | null = null,
  ) => invoke<TransformJobSnapshot>("start_transform_job", { pipelineId, outputTable, incremental, pipelineOverride }),
  getTransformJob: (id: string) => invoke<TransformJobSnapshot>("get_transform_job", { id }),
  listTransformJobs: () => invoke<TransformJobSnapshot[]>("list_transform_jobs"),
  cancelTransformJob: (id: string) => invoke<TransformJobSnapshot>("cancel_transform_job", { id }),
  dataConnectors: () => invoke<SavedConnector[]>("data_connectors"),
  dataConnectorStatuses: () => invoke<ConnectorStatus[]>("data_connector_statuses"),
  saveDataConnector: (connector: SavedConnector) => invoke<TableInfo>("save_data_connector", { connector }),
  refreshDataConnector: (id: string, acceptSchemaChange = false) =>
    invoke<TableInfo>("refresh_data_connector", { id, acceptSchemaChange }),
  deleteDataConnector: (id: string) => invoke<SavedConnector[]>("delete_data_connector", { id }),
  dataPage: (table: string, offset: number, limit: number, sort: Sort | null, filters: ColumnFilter[]) =>
    invoke<Page>("data_page", { table, offset, limit, sort, filters }),
  updateDataCell: (table: string, rid: number, column: string, text: string) =>
    invoke<RecalcStats>("update_data_cell", { table, rid, column, text }),
  exportTable: (table: string, path: string, locale: string) => invoke<void>("export_table", { table, path, locale }),
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
