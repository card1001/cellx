import { formatLocaleNumber, getActiveLocale, tr } from "./i18n";
import { DataJobProgress } from "./components/DataJobProgress";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AutoFilter,
  type BookInfo,
  type NameView,
  type PageSetup,
  type ProtectionView,
  type TableView,
  type ValidationFailure,
  type CellDetail,
  type Chart,
  type ChartKind,
  type Picture,
  type Shape,
  type ShapeKind,
  type Slicer,
  type SlicerView,
  type SparkKind,
  type SparklineGroup,
  type SparklineView,
  type RecoveredFile,
  type Direction,
  type PasteSpecial,
  type PivotDef,
  errorText,
  isTauri,
  type RecalcStats,
  type SelectionStats,
  type TableInfo,
  type TransformPipeline,
  type SavedPipeline,
  type TransformPreview,
  type AggregateFunction,
  type SavedConnector,
  type ConnectorStatus,
  type TableProfile,
  type TransformJobSnapshot,
} from "./api";
import { colorScaleRule, dataBarRule, highlightRule, type HighlightKind, ICON_SETS, iconSetRule, ratingRule } from "./cf";
import { CHART_KINDS, ChartLayer } from "./components/Chart";
import { PictureLayer, pictureUrl } from "./components/Picture";
import { ShapeFormatDialog, ShapeLayer } from "./components/Shape";
import { BookProtectDialog, CustomViewsDialog, DataTableDialog, ScenarioDialog } from "./components/WhatIf";
import { SlicerInsertDialog, SlicerLayer, SlicerSettingsDialog } from "./components/Slicer";
import { SparklineCreateDialog, SparklineLayer, SparklineStyleDialog } from "./components/Sparkline";
import { arrowKey, EvaluateDialog, TraceLayer, type TraceArrow } from "./components/Trace";
import { RecoveryDialog, SaveChangesDialog, type SaveChoice } from "./components/FileDialogs";
import { useI18n, type Locale } from "./i18n";
import { HeaderFooterDialog, MoveCopyDialog, UnhideDialog } from "./components/SheetDialogs";
import { ColorPicker } from "./components/ColorPicker";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  type ColumnChoice,
  DuplicatesDialog,
  FilterMenu,
  type FindOptions,
  FindReplaceDialog,
  GoalSeekDialog,
  HyperlinkDialog,
  ListDropdown,
  NameManagerDialog,
  SortDialog,
  TextToColumnsDialog,
  ValidationAlert,
  ValidationDialog,
  CreateTableDialog,
  MessageBox,
  PasswordDialog,
  ProtectDialog,
  SubtotalDialog,
  ImportDataDialog,
  ExportEncodingDialog,
} from "./components/DataDialogs";
import { type FormatResult, FormatCellsDialog, PromptDialog } from "./components/Dialogs";
import { type PrintJob, paginate, PrintPreviewDialog, PrintView } from "./components/Print";
import { CfManagerDialog, ChartDialog, HighlightDialog, MemoBox, PasteSpecialDialog, PivotDialog } from "./components/ObjectDialogs";
import { FormulaText } from "./components/FormulaText";
import { ContextMenu, type MenuItem } from "./components/Menu";
import { Ribbon, type RibbonActions } from "./components/Ribbon";
import { decreaseDecimals, increaseDecimals } from "./formats";
import { canPointAt, completeFunctions, cycleAbsolute, functionPrefixAt, insertRef, type PointRef, quoteSheet, refSpans } from "./formula";
import { fontCss, Grid, type Hit, type RefBox, type Selection } from "./grid/Grid";
import { addr, colName, normalize, parseAddr, type Pos, type Range, rangeLabel, toTsv } from "./grid/model";
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
  | { kind: "pasteSpecial" }
  | { kind: "highlight"; cf: HighlightKind }
  | { kind: "cfManager" }
  | { kind: "pivot"; def: PivotDef }
  | { kind: "chart"; chart: Chart }
  | { kind: "sort"; range: Range; header: boolean }
  | { kind: "textToColumns"; range: Range; samples: string[] }
  | { kind: "duplicates"; range: Range; header: boolean }
  | { kind: "validation"; current: import("./api").Validation | null }
  | { kind: "names"; list: NameView[]; startNew: boolean }
  | { kind: "hyperlink"; text: string; current: { target: string; internal: boolean } | null }
  | { kind: "goalSeek" }
  | { kind: "evaluate"; sheet: number; row: number; col: number }
  | { kind: "shapeFormat"; shape: Shape }
  | { kind: "sparkCreate"; spark: SparkKind; data: string; location: string }
  | { kind: "sparkStyle"; group: SparklineGroup }
  | { kind: "slicerInsert"; table: string; columns: string[] }
  | { kind: "dataTable"; range: Range }
  | { kind: "bookProtect" }
  | { kind: "bookUnprotect" }
  | { kind: "customViews" }
  | { kind: "scenarios"; selection: string }
  | { kind: "slicerSettings"; slicer: Slicer }
  | { kind: "print" }
  | { kind: "subtotal"; range: Range }
  | { kind: "createTable"; range: Range; header: boolean; style: string }
  | { kind: "importData"; path: string; suggested: string; csv: boolean; resolve(v: { name: string; encoding: string | null } | null): void }
  | { kind: "exportEncoding"; resolve(v: string | null | undefined): void }
  | { kind: "protect" }
  | { kind: "unprotect" }
  | null;

const ARROWS: Record<string, Direction> = { ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
const DELTA: Record<Direction, [number, number]> = { Up: [-1, 0], Down: [1, 0], Left: [0, -1], Right: [0, 1] };
const PROTECTED_MSG = "변경하려는 셀이나 차트가 보호된 시트에 있습니다. 변경하려면 검토 탭에서 시트 보호를 해제하세요. 암호를 입력해야 할 수도 있습니다.";
const FONT_SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

const measureCtx = typeof document !== "undefined" ? document.createElement("canvas").getContext("2d") : null;

/** The cell editor grows to the right with its text, as in Excel. */
function editorWidth(text: string, font: string, min: number): number {
  if (!measureCtx) return min;
  measureCtx.font = font;
  const longest = Math.max(...text.split("\n").map((l) => measureCtx.measureText(l).width));
  return Math.max(min, Math.ceil(longest + 14));
}

function formatCount(n: number): string {
  return formatLocaleNumber(n, getActiveLocale(), { maximumFractionDigits: 0 });
}

function formatNumber(n: number): string {
  // Status-bar formatting follows the UI locale; workbook cell formats remain unchanged.
  const options = Number.isInteger(n) ? undefined : { maximumSignificantDigits: 10 };
  return formatLocaleNumber(n, getActiveLocale(), options);
}

export default function App() {
  const { locale, setLocale, t } = useI18n();
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
  // A long data-sheet command (import, export, query) is running: shows the progress window.
  const [dataJob, setDataJob] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [showSql, setShowSql] = useState(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findTab, setFindTab] = useState<"찾기" | "바꾸기">("찾기");
  const [filter, setFilter] = useState<AutoFilter | null>(null);
  const [filterMenu, setFilterMenu] = useState<{
    col: number;
    x: number;
    y: number;
    values: import("./api").FilterValue[];
    numeric: boolean;
  } | null>(null);
  const [listItems, setListItems] = useState<string[] | null>(null);
  const [listOpen, setListOpen] = useState<{ x: number; y: number } | null>(null);
  const [inputTip, setInputTip] = useState<{ title: string; message: string } | null>(null);
  const [vAlert, setVAlert] = useState<{ failure: ValidationFailure; session: Session } | null>(null);
  const [printJob, setPrintJob] = useState<PrintJob | null>(null);
  const [printState, setPrintState] = useState<{ job: PrintJob; setup: PageSetup; selection: boolean } | null>(null);
  const [outline, setOutline] = useState<{ rows: Map<number, number>; cols: Map<number, number>; maxRow: number; maxCol: number } | null>(null);
  const [tables, setTables] = useState<TableView[]>([]);
  const [protection, setProtection] = useState<ProtectionView | null>(null);
  const [pageSetupState, setPageSetupState] = useState<PageSetup | null>(null);
  const [showPageBreaks, setShowPageBreaks] = useState(false);
  // 수식 분석: trace arrows, and the sheets showing formulas (Ctrl+`).
  const [arrows, setArrows] = useState<TraceArrow[]>([]);
  const traceFrontier = useRef<{ kind: "prec" | "dep"; root: string; cells: { sheet: number; row: number; col: number }[] } | null>(null);
  const [formulaSheets, setFormulaSheets] = useState<Set<number>>(() => new Set());
  const [msgBox, setMsgBox] = useState<string | null>(null);
  const [savePrompt, setSavePrompt] = useState<{ name: string; resolve(c: SaveChoice): void } | null>(null);
  const [recovered, setRecovered] = useState<RecoveredFile[]>([]);
  const [sheetDialog, setSheetDialog] = useState<{ kind: "move"; sheet: number } | { kind: "unhide" } | { kind: "headerFooter" } | null>(null);
  const [tabColorAt, setTabColorAt] = useState<{ sheet: number; x: number; y: number } | null>(null);
  const dragTab = useRef<number | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);
  const protectionRef = useRef<ProtectionView | null>(null);
  protectionRef.current = protection;
  const linkClick = useRef<{ r: number; c: number; t: number } | null>(null);
  const [findMsg, setFindMsg] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [copyMark, setCopyMark] = useState<{ tab: string; range: Range; cut: boolean } | null>(null);
  const [acIndex, setAcIndex] = useState(0);
  const [acDismissed, setAcDismissed] = useState(false);
  const [source, setSource] = useState<SheetSource | DataSource | null>(null);
  const [charts, setCharts] = useState<Chart[]>([]);
  const [chartVersion, setChartVersion] = useState(0);
  const [selectedChart, setSelectedChart] = useState<number | null>(null);
  const [pictures, setPictures] = useState<Picture[]>([]);
  const [selectedPicture, setSelectedPicture] = useState<number | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [sparklines, setSparklines] = useState<SparklineView[]>([]);
  const [slicers, setSlicers] = useState<SlicerView[]>([]);
  const [selectedSlicer, setSelectedSlicer] = useState<number | null>(null);
  const [selectedShape, setSelectedShape] = useState<number | null>(null);
  const [editingShape, setEditingShape] = useState<number | null>(null);
  const [hoverMemo, setHoverMemo] = useState<Pos | null>(null);
  const [editMemo, setEditMemo] = useState<Pos | null>(null);
  const [showMemos, setShowMemos] = useState(false);
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

  const say = (text: string, error = false) => setMessage({ text: tr(text), error });
  const fail = (e: unknown) => say(errorText(e), true);
  const focusGrid = () => requestAnimationFrame(() => cellInput.current?.focus({ preventScroll: true }));

  const ask = (title: string, label: string, value = "") =>
    new Promise<string | null>((resolve) => setDialog({ kind: "prompt", title: tr(title), label: tr(label), value, resolve }));

  const run = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(tr(label));
    try {
      return await fn();
    } catch (e) {
      fail(e);
      return undefined;
    } finally {
      setBusy(null);
    }
  };

  /** `run` for data-sheet commands that report progress and can be cancelled. */
  const runDataJob = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setDataJob(true);
    try {
      return await run(label, fn);
    } finally {
      setDataJob(false);
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
        s.showFormulas = formulaSheetsRef.current.has(t.index);
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
    // Excel takes the trace arrows away once the sheet changes.
    setArrows((a) => (a.length ? [] : a));
    traceFrontier.current = null;
    await Promise.all([...sources.current.values()].map((s) => (s instanceof SheetSource ? s.invalidate() : null)));
  };

  const formulaSheetsRef = useRef(formulaSheets);
  formulaSheetsRef.current = formulaSheets;

  const toggleShowFormulas = () => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const next = new Set(formulaSheetsRef.current);
    const on = !next.has(t.index);
    if (on) next.add(t.index);
    else next.delete(t.index);
    setFormulaSheets(next);
    const s = sources.current.get(tabKey(t));
    if (s instanceof SheetSource) s.setShowFormulas(on);
  };

  /** 참조되는/참조하는 셀 추적: each click adds one more level, as in Excel. */
  const trace = async (kind: "prec" | "dep") => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const f = selectionRef.current.focus;
    const root = `${t.index}:${f.r}:${f.c}`;
    let frontier = traceFrontier.current;
    if (!frontier || frontier.kind !== kind || frontier.root !== root) frontier = { kind, root, cells: [{ sheet: t.index, row: f.r, col: f.c }] };
    const found: TraceArrow[] = [];
    const next: { sheet: number; row: number; col: number }[] = [];
    for (const cell of frontier.cells.slice(0, 200)) {
      const list = await (kind === "prec" ? api.tracePrecedents : api.traceDependents)(cell.sheet, cell.row, cell.col).catch(() => []);
      for (const area of list) {
        if (kind === "prec") {
          found.push({ from: area, to: cell });
          for (let r = area.row1; r <= area.row2 && next.length < 400; r++)
            for (let c = area.col1; c <= area.col2 && next.length < 400; c++) next.push({ sheet: area.sheet, row: r, col: c });
        } else {
          found.push({ from: { sheet: cell.sheet, row1: cell.row, col1: cell.col, row2: cell.row, col2: cell.col }, to: { sheet: area.sheet, row: area.row1, col: area.col1 } });
          next.push({ sheet: area.sheet, row: area.row1, col: area.col1 });
        }
      }
    }
    if (!found.length) {
      if (frontier.cells.length === 1 && frontier.cells[0].row === f.r && frontier.cells[0].col === f.c)
        say(kind === "prec" ? "현재 셀에 수식이 없거나 참조하는 셀이 없습니다" : "현재 셀을 참조하는 수식이 없습니다", true);
      return;
    }
    traceFrontier.current = { kind, root, cells: next };
    setArrows((cur) => {
      const keys = new Set(cur.map(arrowKey));
      return [...cur, ...found.filter((a) => !keys.has(arrowKey(a)) && (keys.add(arrowKey(a)), true))];
    });
  };

  // ---------------------------------------------------------------- book
  const refreshInfo = async () => setInfo(await api.bookInfo());

  useEffect(() => {
    if (!isTauri()) return;
    void refreshInfo().catch(fail);
    void api.functionNames().then(setFunctions).catch(() => {});
    // 문서 복구: files an earlier CellX saved before it stopped.
    void api.recoveryList().then(setRecovered).catch(() => {});
    focusGrid();
  }, []);

  // A hidden sheet is never the one on screen.
  useEffect(() => {
    if (tab.kind !== "sheet" || !info?.tabs?.[tab.index]?.hidden) return;
    const next = info.tabs.findIndex((x) => !x.hidden);
    if (next >= 0) switchTab({ kind: "sheet", index: next });
  }, [info, tab]);

  // 자동 복구 저장: every 3 minutes while there are unsaved changes.
  const infoRef = useRef(info);
  infoRef.current = info;
  useEffect(() => {
    if (!isTauri()) return;
    const id = window.setInterval(() => {
      if (infoRef.current?.dirty) {
        void api.autosave().catch((error) => {
          // A failed recovery write must be visible; silently swallowing it
          // would give the user a false sense that unsaved work is protected.
          fail(`자동 복구 파일을 저장하지 못했습니다: ${errorText(error)}`);
        });
      }
    }, 3 * 60 * 1000);
    return () => window.clearInterval(id);
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

  // AutoFilter of the sheet on screen.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setFilter(null);
      return;
    }
    let live = true;
    api
      .autoFilter(tab.index)
      .then((f) => live && setFilter(f))
      .catch(() => live && setFilter(null));
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  // Data validation of the active cell: list arrow and input message.
  useEffect(() => {
    setListOpen(null);
    if (tab.kind !== "sheet") {
      setListItems(null);
      setInputTip(null);
      return;
    }
    const f = selection.focus;
    let live = true;
    api
      .validationAt(tab.index, f.r, f.c)
      .then(async (v) => {
        if (!live) return;
        setInputTip(v && v.show_input && (v.input_title || v.input_message) ? { title: v.input_title, message: v.input_message } : null);
        if (v && v.kind === "List" && v.in_cell_dropdown) {
          const items = await api.listItems(tab.index, f.r, f.c);
          if (live) setListItems(items);
        } else setListItems(null);
      })
      .catch(() => {
        setListItems(null);
        setInputTip(null);
      });
    return () => {
      live = false;
    };
  }, [selection.focus, tab, chartVersion]);

  // Outline, tables, protection and page setup of the sheet on screen.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setOutline(null);
      setTables([]);
      setProtection(null);
      setPageSetupState(null);
      return;
    }
    let live = true;
    const sheet = tab.index;
    Promise.all([api.outline(sheet), api.tablesOn(sheet), api.protection(sheet), api.pageSetup(sheet)])
      .then(([o, t, p, ps]) => {
        if (!live) return;
        const rows = new Map(o.rows);
        const cols = new Map(o.cols);
        setOutline(rows.size || cols.size ? { rows, cols, maxRow: Math.max(0, ...rows.values()), maxCol: Math.max(0, ...cols.values()) } : null);
        setTables(t);
        setProtection(p);
        setPageSetupState(ps);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  // Sparklines and slicers of the sheet on screen; they follow every recalculation.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setSparklines([]);
      setSlicers([]);
      return;
    }
    let live = true;
    void api.sparklines(tab.index).then((v) => live && setSparklines(v)).catch(() => live && setSparklines([]));
    void api.slicers(tab.index).then((v) => live && setSlicers(v)).catch(() => live && setSlicers([]));
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  // Shapes of the sheet on screen.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setShapes([]);
      return;
    }
    let live = true;
    api
      .shapes(tab.index)
      .then((p) => live && setShapes(p))
      .catch(() => live && setShapes([]));
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  // Pictures of the sheet on screen.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setPictures([]);
      return;
    }
    let live = true;
    api
      .pictures(tab.index)
      .then((p) => live && setPictures(p))
      .catch(() => live && setPictures([]));
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  // Charts of the sheet on screen.
  useEffect(() => {
    if (tab.kind !== "sheet") {
      setCharts([]);
      return;
    }
    let live = true;
    api
      .charts(tab.index)
      .then((c) => live && setCharts(c))
      .catch(() => live && setCharts([]));
    return () => {
      live = false;
    };
  }, [tab, info, chartVersion]);

  const afterChange = async (s?: RecalcStats) => {
    if (s) setStats(s);
    setChartVersion((v) => v + 1);
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
  /** On a protected sheet, cells are locked unless 잠금 was turned off for them. */
  const lockedCell = (p: Pos) => {
    const pr = protectionRef.current;
    if (!pr?.protected || tabRef.current.kind !== "sheet") return false;
    return !pr.unlocked.some((u) => p.r >= u.r0 && p.r <= u.r1 && p.c >= u.c0 && p.c <= u.c1);
  };

  const newSession = (text: string, mode: Session["mode"], origin: Session["origin"], original: string): Session | null => {
    const pos = selectionRef.current.focus;
    const src = sourceRef.current;
    if (tabRef.current.kind === "data" && !(src instanceof DataSource && src.rowId(pos.r) !== undefined)) return null;
    if (lockedCell(pos)) {
      setMsgBox(PROTECTED_MSG);
      return null;
    }
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
    if (s.tab.kind === "sheet" && !fill) {
      const failure = await api.validateInput(s.tab.index, s.pos.r, s.pos.c, s.text).catch(() => null);
      if (failure) {
        setVAlert({ failure, session: s });
        return;
      }
    }
    await writeSession(s, fill);
  };

  const writeSession = async (s: Session, fill: boolean) => {
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
    if (r.locked !== undefined) await api.setLocked(sheet, range(), r.locked).catch(fail);
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

  /** 「변경 내용을 저장하시겠습니까?」 before the open workbook goes away. False: stay. */
  const confirmLeave = async (): Promise<boolean> => {
    const cur = infoRef.current;
    if (!cur?.dirty) return true;
    const name = cur.path?.split(/[\\/]/).pop() ?? "통합 문서1";
    const choice = await new Promise<SaveChoice>((resolve) => setSavePrompt({ name, resolve }));
    setSavePrompt(null);
    if (choice === "cancel") return false;
    if (choice === "discard") return true;
    return save();
  };

  // Closing the window asks too.
  useEffect(() => {
    if (!isTauri()) return;
    const w = getCurrentWindow();
    let closing = false;
    const off = w.onCloseRequested(async (event) => {
      if (closing || !infoRef.current?.dirty) return;
      event.preventDefault();
      if (await confirmLeave()) {
        closing = true;
        await w.destroy();
      }
    });
    return () => {
      void off.then((f) => f());
    };
  }, []);

  const newBook = async () => {
    if (!(await confirmLeave())) return;
    const i = await run("새 문서", api.newBook);
    if (i) resetBook(i);
  };

  const openRecovered = async (id: string) => {
    if (!(await confirmLeave())) return;
    const i = await run("복구 중", () => api.recoveryOpen(id));
    if (!i) return;
    resetBook(i);
    setRecovered((list) => list.filter((f) => f.id !== id));
    say("복구한 파일입니다. 확인한 뒤 저장하세요.");
  };

  const openFile = async () => {
    if (!(await confirmLeave())) return;
    const path = await openDialog({
      multiple: false,
      filters: [
        { name: tr("Excel 통합 문서 / CellX 프로젝트"), extensions: ["xlsx", "cellx"] },
        { name: tr("모든 파일"), extensions: ["*"] },
      ],
    });
    if (!path || Array.isArray(path)) return;
    const t = performance.now();
    const i = await run("여는 중", () => api.openFile(path));
    if (!i) return;
    resetBook(i);
    say(`${path} (${Math.round(performance.now() - t)} ms)`);
  };

  /** Returns whether it saved. */
  const saveAs = async (): Promise<boolean> => {
    const path = await saveDialog({
      defaultPath: infoRef.current?.path ?? "통합 문서1.xlsx",
      filters: [
        { name: tr("Excel 통합 문서"), extensions: ["xlsx"] },
        { name: tr("CellX 프로젝트 (데이터 시트 포함)"), extensions: ["cellx"] },
      ],
    });
    if (!path) return false;
    const i = await run("저장 중", () => api.saveFile(path));
    if (!i) return false;
    setInfo(i);
    say(
      path.toLowerCase().endsWith(".xlsx") && i.tables.length > 0
        ? "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다"
        : `저장했습니다: ${path}`,
    );
    return true;
  };

  const save = async (): Promise<boolean> => {
    const path = infoRef.current?.path;
    if (!path) return saveAs();
    const i = await run("저장 중", () => api.saveFile(path));
    if (!i) return false;
    setInfo(i);
    say(`저장했습니다: ${path}`);
    return true;
  };

  const importData = async () => {
    const path = await openDialog({ multiple: false, filters: [{ name: tr("데이터 파일"), extensions: ["csv", "tsv", "txt", "parquet"] }] });
    if (!path || Array.isArray(path)) return;
    const base = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "data";
    const suggested = base.replace(/[^\p{L}\p{N}_]/gu, "_").replace(/^(\d)/, "_$1");
    const csv = !/\.parquet$/i.test(path);
    const choice = await new Promise<{ name: string; encoding: string | null } | null>((resolve) =>
      setDialog({ kind: "importData", path, suggested, csv, resolve }),
    );
    if (!choice) return;
    const { name, encoding } = choice;
    const t = performance.now();
    const table = await runDataJob("가져오는 중", () => api.importData(path, name, locale, encoding));
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
    let encoding: string | null = null;
    if (!/\.parquet$/i.test(path)) {
      const picked = await new Promise<string | null | undefined>((resolve) => setDialog({ kind: "exportEncoding", resolve }));
      if (picked === undefined) return;
      encoding = picked;
    }
    const ok = await runDataJob("내보내는 중", () => api.exportTable(src.info.name, path, locale, encoding));
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

  // ---------------------------------------------------------------- sheet tabs: 이동/복사, 탭 색, 숨기기
  const sheetOp = async (label: string, fn: () => Promise<BookInfo>, show?: (i: BookInfo) => number) => {
    if (sessionRef.current) await commit("none");
    const i = await run(label, fn);
    if (!i) return;
    sources.current.clear();
    setInfo(i);
    setChartVersion((v) => v + 1);
    const want = show?.(i);
    if (want !== undefined) switchTab({ kind: "sheet", index: want });
  };

  const moveSheet = (from: number, to: number) =>
    void sheetOp("시트 이동", () => api.moveSheet(from, to), () => to);

  const copySheet = (sheet: number, to: number) =>
    void sheetOp("시트 복사", () => api.copySheet(sheet, to), () => to);

  const hideSheet = (sheet: number) =>
    void sheetOp("시트 숨기기", () => api.setSheetHidden(sheet, true), (i) => {
      const t = tabRef.current;
      if (t.kind === "sheet" && t.index !== sheet) return t.index;
      const next = i.tabs.findIndex((x, k) => k > sheet && !x.hidden);
      return next >= 0 ? next : i.tabs.findIndex((x) => !x.hidden);
    });

  const unhideSheet = (sheet: number) => void sheetOp("숨기기 취소", () => api.setSheetHidden(sheet, false), () => sheet);

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

  const find = async (o: FindOptions) => {
    const t = tabRef.current;
    if (t.kind !== "sheet" || !o.find) return;
    const f = selectionRef.current.focus;
    const matches = async (sheet: number, at: [number, number]) => {
      if (!o.wholeCell) return true;
      const d = await api.cellDetail(sheet, at[0], at[1]);
      const eq = (a: string) => (o.matchCase ? a === o.find : a.toLowerCase() === o.find.toLowerCase());
      return eq(d.input) || eq(d.text);
    };
    // Whole-cell matches: step through the contains-matches of the sheet.
    const searchSheet = async (sheet: number, from: Pos, wrap: boolean): Promise<[number, number] | null> => {
      let cur = from;
      for (let guard = 0; guard < 2000; guard++) {
        const at = await api.find(sheet, o.find, cur.r, cur.c, o.matchCase).catch(() => null);
        if (!at) return null;
        const wrapped = at[0] < from.r || (at[0] === from.r && at[1] <= from.c);
        if (wrapped && !wrap) return null;
        if (await matches(sheet, at)) return at;
        if (guard > 0 && at[0] === from.r && at[1] === from.c) return null;
        cur = { r: at[0], c: at[1] };
      }
      return null;
    };
    const here = await searchSheet(t.index, f, !o.workbook);
    if (here) {
      moveTo({ r: here[0], c: here[1] });
      setFindMsg(null);
      return;
    }
    if (o.workbook) {
      const n = info?.sheets.length ?? 1;
      for (let k = 1; k <= n; k++) {
        const sh = (t.index + k) % n;
        const at = await searchSheet(sh, { r: 0, c: 0 }, false);
        if (at) {
          switchTab({ kind: "sheet", index: sh });
          setSelection({ anchor: { r: at[0], c: at[1] }, focus: { r: at[0], c: at[1] } });
          setFindMsg(null);
          return;
        }
      }
    }
    setFindMsg("찾는 내용이 없습니다.");
  };

  const replaceOne = async (o: FindOptions) => {
    const t = tabRef.current;
    if (t.kind !== "sheet" || !o.find) return;
    const f = selectionRef.current.focus;
    const done = await api.replaceInCell(t.index, f.r, f.c, o.find, o.replace, o.matchCase, o.wholeCell).catch(() => false);
    if (done) await afterChange();
    await find(o);
  };

  const replaceAll = async (o: FindOptions) => {
    const t = tabRef.current;
    if (!o.find) return;
    const r = await run("바꾸는 중", () => api.replaceAll(o.workbook ? null : t.kind === "sheet" ? t.index : 0, o.find, o.replace, o.matchCase, o.wholeCell));
    if (!r) return;
    await afterChange(r[1]);
    setFindMsg(r[0] ? `${formatCount(r[0])}개 항목이 바뀌었습니다.` : "찾는 내용이 없습니다.");
  };

  // ---------------------------------------------------------------- data tools
  const sheetOrSay = () => {
    const t = tabRef.current;
    if (t.kind !== "sheet") {
      say("이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.", true);
      return null;
    }
    return t.index;
  };

  /** Excel's guess: a first row of text above rows holding numbers (or in bold) is a header. */
  const guessHeader = (rg: Range): boolean => {
    if (filter && filter.range.r0 === rg.r0 && filter.range.c0 === rg.c0) return true;
    const src = sourceRef.current;
    if (!src || rg.r1 <= rg.r0) return false;
    let anyText = false;
    let bold = false;
    for (let c = rg.c0; c <= rg.c1; c++) {
      const cell = src.cell(rg.r0, c);
      if (cell?.numeric) return false;
      if (cell?.text) anyText = true;
      if (cell?.style?.bold) bold = true;
    }
    if (!anyText) return false;
    if (bold) return true;
    for (let r = rg.r0 + 1; r <= Math.min(rg.r1, rg.r0 + 20); r++)
      for (let c = rg.c0; c <= rg.c1; c++) if (src.cell(r, c)?.numeric) return true;
    return false;
  };

  const columnChoices = (rg: Range) => (hasHeader: boolean): ColumnChoice[] => {
    const src = sourceRef.current;
    const out: ColumnChoice[] = [];
    for (let c = rg.c0; c <= rg.c1; c++) {
      const head = hasHeader ? src?.cell(rg.r0, c)?.text : "";
      out.push({ col: c, label: head || `열 ${colName(c)}` });
    }
    return out;
  };

  /** The range a data command works on: the selection, or the current region for one cell. */
  const workRange = async (): Promise<Range> => {
    const rg = range();
    const t = tabRef.current;
    if (t.kind === "sheet" && rg.r0 === rg.r1 && rg.c0 === rg.c1) {
      if (filter && rg.r0 >= filter.range.r0 && rg.r0 <= filter.range.r1 && rg.c0 >= filter.range.c0 && rg.c0 <= filter.range.c1) return filter.range;
      return api.currentRegion(t.index, rg.r0, rg.c0);
    }
    return rg;
  };

  const quickSort = async (descending: boolean) => {
    const sheet = sheetOrSay();
    if (sheet === null) return;
    const rg = await workRange();
    const st = await run("정렬", () => api.sortRange(sheet, rg, [{ col: selectionRef.current.focus.c, descending }], guessHeader(rg)));
    if (st) await afterChange(st);
  };

  const toggleFilter = async () => {
    const sheet = sheetOrSay();
    if (sheet === null) return;
    const f = await run("필터", () => api.toggleAutoFilter(sheet, range()));
    if (f !== undefined) await afterChange();
  };

  const openFilterMenu = async (col: number, x: number, y: number) => {
    const sheet = sheetOrSay();
    if (sheet === null) return;
    const values = await api.filterValues(sheet, col).catch(fail);
    if (!values) return;
    const nonBlank = values.filter((v) => v.text !== "");
    const numeric = nonBlank.length > 0 && nonBlank.every((v) => !Number.isNaN(Number(v.text.replace(/,/g, ""))));
    setFilterMenu({ col, x, y, values, numeric });
  };

  const followLink = async (p: Pos) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const l = await api.linkAt(t.index, p.r, p.c).catch(() => null);
    if (!l) return;
    if (!l.internal) {
      await api.openLink(l.target).catch(fail);
      return;
    }
    goTo(l.target);
  };

  /** Jumps to `Sheet!A1`, `A1:B5` or a defined name (이름 상자, F5, internal links). */
  const goTo = async (ref: string): Promise<boolean> => {
    let text = ref.trim().replace(/^=/, "");
    const named = (await api.names().catch(() => [] as NameView[])).find((n) => n.name.toLowerCase() === text.toLowerCase());
    if (named) text = named.formula.replace(/^=/, "");
    const m = /^(?:'?(.+?)'?!)?(\$?[A-Za-z]{1,3}\$?\d+)(?::(\$?[A-Za-z]{1,3}\$?\d+))?$/.exec(text);
    if (!m) return false;
    const a = parseAddr(m[2].replace(/\$/g, ""));
    const b = m[3] ? parseAddr(m[3].replace(/\$/g, "")) : a;
    if (!a || !b) return false;
    if (m[1]) {
      const i = (info?.sheets ?? []).findIndex((n) => n.toLowerCase() === m[1].replace(/''/g, "'").toLowerCase());
      if (i < 0) return false;
      if (tabRef.current.kind !== "sheet" || tabRef.current.index !== i) switchTab({ kind: "sheet", index: i });
    }
    setSelection({ anchor: a, focus: b });
    focusGrid();
    return true;
  };

  /** Builds what to print: the used range (or the selection), sizes, merges and charts. */
  const buildPrintJob = async (selectionOnly: boolean): Promise<PrintJob | null> => {
    const src = sourceRef.current;
    const t = tabRef.current;
    if (!(src instanceof SheetSource) || t.kind !== "sheet") {
      say("일반 시트만 인쇄할 수 있습니다", true);
      return null;
    }
    const area = pageSetupState?.print_area ?? null;
    let rg = selectionOnly ? range() : area ?? { r0: 1, c0: 1, r1: Math.max(1, src.lastRow), c1: Math.max(1, src.lastCol) };
    if (!selectionOnly && !area) {
      // Charts and pictures outside the cells widen the printed area, as in Excel.
      for (const c of [...charts, ...pictures, ...shapes]) {
        let w = c.anchor.dx + c.anchor.width;
        let col = c.anchor.col;
        while (w > 0 && col < 16384) w -= src.cols.size(col++);
        let h = c.anchor.dy + c.anchor.height;
        let row = c.anchor.row;
        while (h > 0 && row < 1048576) h -= src.rows.size(row++);
        rg = { ...rg, r1: Math.max(rg.r1, row - 1), c1: Math.max(rg.c1, col - 1) };
      }
    }
    const r1 = Math.min(rg.r1, rg.r0 + 20000);
    const [vp, layout, chartData] = await Promise.all([
      api.viewport(t.index, rg.r0, rg.c0, r1, rg.c1),
      api.layout(t.index),
      Promise.all(charts.map((c) => api.chartData(c.id).then((data) => ({ chart: c, data })).catch(() => null))),
    ]);
    const pics = selectionOnly ? [] : await Promise.all(pictures.map((p) => pictureUrl(p.id).then((url) => ({ picture: p, url })).catch(() => null)));
    return {
      title: sheetName(t.index),
      file: infoRef.current?.path?.split(/[\\/]/).pop() ?? "통합 문서1",
      r0: rg.r0,
      c0: rg.c0,
      r1,
      c1: rg.c1,
      colWidth: (c) => src.cols.size(c),
      rowHeight: (r) => src.rows.size(r),
      cells: new Map(vp.cells.map((c) => [`${c.row}:${c.col}`, c])),
      merges: layout.merges.map(([r, c, h, w]) => ({ r, c, h, w })),
      font: src.font,
      charts: selectionOnly ? [] : chartData.filter((x): x is NonNullable<typeof x> => !!x),
      pictures: pics.filter((x): x is NonNullable<typeof x> => !!x),
      shapes: selectionOnly ? [] : shapes,
      sparklines,
    };
  };

  const openPrint = async (selectionOnly = false) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("일반 시트만 인쇄할 수 있습니다", true);
    const job = await buildPrintJob(selectionOnly);
    if (!job) return;
    const setup = await api.pageSetup(t.index);
    setPrintState({ job, setup, selection: selectionOnly });
  };

  const doPrint = () => {
    const ps = printState;
    if (!ps) return;
    setPrintJob(ps.job);
    setPrintState(null);
    setShowPageBreaks(true);
    // Two frames: the pages must be laid out before the print dialog takes them.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        api.printPage().catch(() => window.print());
      }),
    );
  };

  /** Page break lines for the grid (Excel shows them after printing or in page layout). */
  const pageBreakLines = useMemo(() => {
    const src = source;
    if (!showPageBreaks || !(src instanceof SheetSource) || !pageSetupState) return null;
    const job: PrintJob = {
      title: "",
      file: "",
      r0: 1,
      c0: 1,
      r1: Math.max(1, src.lastRow),
      c1: Math.max(1, src.lastCol),
      colWidth: (c) => src.cols.size(c),
      rowHeight: (r) => src.rows.size(r),
      cells: new Map(),
      merges: [],
      font: src.font,
      charts: [],
      pictures: [],
    };
    const pag = paginate(job, pageSetupState);
    return {
      rows: pag.rowPages.slice(1).map((p) => p[0]),
      cols: pag.colPages.slice(1).map((p) => p[0]),
      manualRows: pageSetupState.row_breaks,
      manualCols: pageSetupState.col_breaks,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showPageBreaks, source, pageSetupState, chartVersion]);

  const changePageSetup = async (ps: PageSetup) => {
    await api.setPageSetup(ps).catch(fail);
    setPageSetupState(ps);
    await refreshInfo();
  };

  const activeTable = tables.find((t) => t.range.r0 <= selection.focus.r && selection.focus.r <= t.range.r1 && t.range.c0 <= selection.focus.c && selection.focus.c <= t.range.c1) ?? null;

  const updateTable = async (t: TableView, change: Partial<{ new_name: string; style: string; stripes: boolean; totals: boolean }>) => {
    const st = await run("표", () =>
      api.updateTable({ name: t.name, new_name: change.new_name ?? t.name, style: change.style ?? t.style, stripes: change.stripes ?? t.stripes, totals: change.totals ?? t.totals }),
    );
    if (st) await afterChange(st);
  };

  // ---------------------------------------------------------------- sparklines, slicers
  /** 삽입 → 스파크라인: data from the selection (or asked), placed where the user says. */
  const insertSparklines = (kind: SparkKind) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("스파크라인은 일반 시트에 넣습니다", true);
    const rg = range();
    const sel = `${colName(rg.c0)}${rg.r0}${rg.r0 === rg.r1 && rg.c0 === rg.c1 ? "" : `:${colName(rg.c1)}${rg.r1}`}`;
    const loc = rg.r0 === rg.r1 && rg.c0 === rg.c1 ? "" : rg.r1 > rg.r0 ? `${colName(rg.c1 + 1)}${rg.r0}:${colName(rg.c1 + 1)}${rg.r1}` : `${colName(rg.c1 + 1)}${rg.r0}`;
    setDialog({ kind: "sparkCreate", spark: kind, data: sel, location: loc });
  };

  const createSparklines = async (kind: SparkKind, data: string, location: string) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const [a, b] = location.trim().replace(/\$/g, "").split(":");
    const p0 = parseAddr(a ?? "");
    const p1 = b ? parseAddr(b) : p0;
    if (!p0 || !p1) return say("위치 범위가 올바르지 않습니다 (예: F2:F10)", true);
    const [r0, c0, r1, c1] = [p0.r, p0.c, p1.r, p1.c];
    const g = await run("스파크라인 만들기", () => api.addSparklines(t.index, kind, data, { r0, c0, r1, c1 }));
    if (g) await afterChange();
  };

  const sparklineAt = (p: Pos) => sparklines.find((s) => s.row === p.r && s.col === p.c);

  const insertSlicer = async () => {
    const t = tabRef.current;
    if (t.kind !== "sheet" || !activeTable) return say("슬라이서를 만들 표 안의 셀을 선택하세요", true);
    setDialog({ kind: "slicerInsert", table: activeTable.name, columns: activeTable.columns });
  };

  const pickSlicer = async (id: number, values: string[] | null) => {
    const st = await run("슬라이서", () => api.slicerSelect(id, values));
    if (st) await afterChange(st);
  };

  // ---------------------------------------------------------------- shapes
  const deleteShape = async (id: number) => {
    await api.deleteShape(id).catch(fail);
    setSelectedShape(null);
    setSelectedSlicer(null);
    await afterChange();
  };

  const updateShape = async (s: Shape) => {
    await api.updateShape(s).catch(fail);
    await afterChange();
  };

  /** 삽입 → 도형 / 텍스트 상자, at the active cell. A text box is typed into right away. */
  const insertShape = async (kind: ShapeKind) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("도형은 일반 시트에 넣습니다", true);
    const f = selectionRef.current.focus;
    const s = await run("도형 삽입", () => api.addShape(t.index, f.r, f.c, kind));
    if (!s) return;
    setSelectedChart(null);
    setSelectedPicture(null);
    setSelectedShape(s.id);
    await afterChange();
    if (kind === "textBox") setEditingShape(s.id);
  };

  // ---------------------------------------------------------------- pictures
  const deletePicture = async (id: number) => {
    await api.deletePicture(id).catch(fail);
    setSelectedPicture(null);
    setSelectedShape(null);
    setSelectedSlicer(null);
    await afterChange();
  };

  const updatePicture = async (p: Picture) => {
    await api.updatePicture(p).catch(fail);
    await afterChange();
  };

  /** 삽입 → 그림: a picture from a file, at the active cell. */
  const insertPicture = async () => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("그림은 일반 시트에 넣습니다", true);
    const path = await openDialog({ multiple: false, filters: [{ name: tr("그림"), extensions: ["png", "jpg", "jpeg", "gif", "bmp"] }] });
    if (typeof path !== "string") return;
    const f = selectionRef.current.focus;
    const p = await run("그림 삽입", () => api.addPictureFile(t.index, f.r, f.c, path));
    if (p) {
      setSelectedChart(null);
      setSelectedPicture(p.id);
      await afterChange();
    }
  };

  /** A pasted image (a screenshot, an image copied from a browser). */
  const pastePicture = async (file: File) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return;
    const data = await new Promise<string>((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result));
      r.onerror = () => reject(r.error);
      r.readAsDataURL(file);
    });
    const f = selectionRef.current.focus;
    const p = await run("그림 붙여넣기", () => api.addPictureData(t.index, f.r, f.c, data));
    if (p) {
      setSelectedChart(null);
      setSelectedPicture(p.id);
      await afterChange();
    }
  };

  /** 원래 크기로: the image's own size, at most the size it was inserted at. */
  const resetPictureSize = async (p: Picture) => {
    const url = await pictureUrl(p.id);
    const img = new Image();
    img.src = url;
    await img.decode().catch(() => {});
    const w = img.naturalWidth || p.anchor.width;
    const h = img.naturalHeight || p.anchor.height;
    await updatePicture({ ...p, anchor: { ...p.anchor, width: w, height: h } });
  };

  // ---------------------------------------------------------------- charts, memos, pivots
  const deleteChart = async (id: number) => {
    await api.deleteChart(id).catch(fail);
    setSelectedChart(null);
    await afterChange();
  };

  const updateChart = async (c: Chart) => {
    await api.updateChart(c).catch(fail);
    await afterChange();
  };

  /** Excel charts the current region when a single cell is selected. */
  const dataRange = async (): Promise<Range> => {
    const rg = range();
    const t = tabRef.current;
    if (t.kind === "sheet" && rg.r0 === rg.r1 && rg.c0 === rg.c1) {
      return api.currentRegion(t.index, rg.r0, rg.c0);
    }
    return rg;
  };

  const insertChart = async (kind: ChartKind) => {
    const t = tabRef.current;
    if (t.kind !== "sheet") return say("차트는 일반 시트의 범위로 만듭니다", true);
    const rg = await dataRange();
    if (rg.r0 === rg.r1 && rg.c0 === rg.c1 && !(await currentInput())) return say("차트로 만들 데이터 범위를 선택하세요", true);
    const c = await run("차트 삽입", () => api.addChart(t.index, kind, rg));
    if (c) {
      setSelectedChart(c.id);
      await afterChange();
    }
  };

  const openPivotDialog = async () => {
    const t = tabRef.current;
    const sheetIdx = t.kind === "sheet" ? t.index : 0;
    let def: PivotDef = { id: 0, source: { kind: "Range", range: "" }, rows: [], columns: null, values: [], sheet: sheetIdx, row: 1, col: 1, extent: [0, 0] };
    if (t.kind === "data") def = { ...def, source: { kind: "Table", name: t.name } };
    else {
      const rg = await dataRange();
      def = { ...def, source: { kind: "Range", range: `${quoteSheet(sheetName(t.index))}!${addr({ r: rg.r0, c: rg.c0 })}:${addr({ r: rg.r1, c: rg.c1 })}` } };
    }
    setDialog({ kind: "pivot", def });
  };

  const memoAt = (p: Pos) => (sourceRef.current instanceof SheetSource ? sourceRef.current.memoAt(p.r, p.c) : undefined);

  const saveMemo = async (p: Pos, text: string) => {
    const t = tabRef.current;
    setEditMemo(null);
    if (t.kind !== "sheet") return;
    const old = memoAt(p)?.text ?? "";
    if (text === old) return focusGrid();
    const st = await run("메모", () => api.setMemo(t.index, p.r, p.c, text.trim() ? text : null));
    if (st) await afterChange(st);
    focusGrid();
  };

  const addCf = async (rule: Parameters<typeof api.addConditionalFormat>[2]) => {
    const st = await onSheetOnly("조건부 서식", (s) => api.addConditionalFormat(s, range(), rule));
    if (st) await afterChange(st);
  };

  const pasteSpecialNow = async (opts: PasteSpecial) => {
    if (!copyMark) return say("먼저 CellX에서 셀을 복사(Ctrl+C)하세요", true);
    const at = range();
    const st = await onSheetOnly("붙여넣는 중", (s) => api.pasteSpecial(s, at.r0, at.c0, opts));
    if (st) await afterChange(st);
    focusGrid();
  };

  // ---------------------------------------------------------------- keys on the grid (not editing)
  const gridKey = async (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const mod = e.ctrlKey || e.metaKey;
    const f = selectionRef.current.focus;
    const t = tabRef.current;
    if (selectedShape !== null) {
      // A selected shape takes Delete, Esc, Enter/F2 (type into it) and the arrow keys.
      const p = shapes.find((x) => x.id === selectedShape);
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        await deleteShape(selectedShape);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setSelectedShape(null);
    setSelectedSlicer(null);
        return;
      }
      if (p && (e.key === "Enter" || e.key === "F2") && p.kind !== "line" && p.kind !== "arrow") {
        e.preventDefault();
        setEditingShape(p.id);
        return;
      }
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
      if (p && nudge[e.key]) {
        e.preventDefault();
        const [x, y] = nudge[e.key];
        const a = { ...p.anchor, dx: Math.max(0, p.anchor.dx + x * (mod ? 1 : 4)), dy: Math.max(0, p.anchor.dy + y * (mod ? 1 : 4)) };
        await updateShape({ ...p, anchor: a });
        return;
      }
      if (!mod) return;
    }
    if (selectedPicture !== null) {
      // A selected picture takes Delete, Esc and the arrow keys (a nudge), like in Excel.
      const p = pictures.find((x) => x.id === selectedPicture);
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        await deletePicture(selectedPicture);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setSelectedPicture(null);
    setSelectedShape(null);
    setSelectedSlicer(null);
        return;
      }
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
      if (p && nudge[e.key]) {
        e.preventDefault();
        const [x, y] = nudge[e.key];
        const a = { ...p.anchor, dx: Math.max(0, p.anchor.dx + x * (mod ? 1 : 4)), dy: Math.max(0, p.anchor.dy + y * (mod ? 1 : 4)) };
        await updatePicture({ ...p, anchor: a });
        return;
      }
      if (!mod) return;
    }
    if (selectedChart !== null) {
      // A selected chart takes Delete and Esc, like a selected shape in Excel.
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        await deleteChart(selectedChart);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setSelectedChart(null);
        return;
      }
      setSelectedChart(null);
    }
    if (mod && e.altKey && e.key.toLowerCase() === "v") {
      e.preventDefault();
      actions.pasteSpecial();
      return;
    }
    if (e.key === "F2" && e.shiftKey) {
      e.preventDefault();
      actions.newMemo();
      return;
    }
    if (mod && e.altKey && e.key === "F5") {
      e.preventDefault();
      actions.refreshAll();
      return;
    }
    if (e.altKey && e.shiftKey && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      // Shift+Alt+→ groups, Shift+Alt+← ungroups (rows unless whole columns are selected).
      e.preventDefault();
      const rows = !wholeCols();
      if (e.key === "ArrowRight") actions.group(rows);
      else actions.ungroup(rows);
      return;
    }
    if (mod && !e.shiftKey && (e.key.toLowerCase() === "t" || e.key.toLowerCase() === "l")) {
      e.preventDefault();
      actions.formatAsTable("TableStyleMedium2");
      return;
    }
    if (e.altKey && e.key === "ArrowDown" && listItems) {
      // Alt+↓ opens the list of a validation rule, like the arrow next to the cell.
      e.preventDefault();
      const rect = document.querySelector(".cell-editor-host")?.getBoundingClientRect();
      if (rect) setListOpen({ x: rect.left, y: rect.bottom });
      return;
    }
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
    } else if (k === "Delete" && lockedCell(f)) {
      handled();
      setMsgBox(PROTECTED_MSG);
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
      if (lower === "l" && e.shiftKey) {
        handled();
        void toggleFilter();
        return;
      }
      if (lower === "e" && !e.shiftKey) {
        handled();
        actions.flashFill();
        return;
      }
      if (lower === "k") {
        handled();
        actions.hyperlink();
        return;
      }
      if (k === "F3") {
        handled();
        actions.nameManager();
        return;
      }
      if (code === "Backquote") {
        handled();
        toggleShowFormulas();
        return;
      }
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
        setFindTab(lower === "h" ? "바꾸기" : "찾기");
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
    setSelectedChart(null);
    setSelectedPicture(null);
    setSelectedShape(null);
    setSelectedSlicer(null);
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
      // A click on a hyperlink follows it; holding the button selects the cell (Excel).
      linkClick.current = sourceRef.current?.cell(h.r, h.c)?.link && !e.shiftKey && !e.ctrlKey ? { r: h.r, c: h.c, t: Date.now() } : null;
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

  const onContextMenu = async (h: Hit, x: number, y: number) => {
    setSelectedChart(null);
    setSelectedPicture(null);
    setSelectedShape(null);
    setSelectedSlicer(null);
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
    else {
      const at = selectionRef.current.focus;
      const memo = memoAt(at);
      const t = tabRef.current;
      const pivot = t.kind === "sheet" ? await api.pivotAt(t.index, at.r, at.c).catch(() => null) : null;
      const pivotItems: MenuItem[] = pivot
        ? [
            { label: "새로 고침", onClick: () => actions.refreshAll() },
            { label: "피벗 테이블 필드...", onClick: () => setDialog({ kind: "pivot", def: pivot }) },
            {
              label: "피벗 테이블 삭제",
              onClick: async () => {
                const st = await run("피벗 테이블 삭제", () => api.deletePivot(pivot.id));
                if (st) await afterChange(st);
              },
            },
            { separator: true },
          ]
        : [];
      items = [
        ...clip,
        { label: "선택하여 붙여넣기...", shortcut: "Ctrl+Alt+V", disabled: !copyMark, onClick: () => actions.pasteSpecial() },
        { separator: true },
        ...pivotItems,
        { label: "행 삽입", onClick: insertRows },
        { label: "열 삽입", onClick: insertColumns },
        { label: "행 삭제", onClick: deleteRows },
        { label: "열 삭제", onClick: deleteColumns },
        { label: "내용 지우기", shortcut: "Delete", onClick: () => clearRange("contents") },
        { separator: true },
        ...(sparklineAt(at)
          ? [
              {
                label: "스파크라인 편집...",
                onClick: async () => {
                  const v = sparklineAt(at);
                  const groups = t.kind === "sheet" ? await api.sparklineGroups(t.index).catch(() => []) : [];
                  const g = groups.find((x) => x.id === v?.group);
                  if (g) setDialog({ kind: "sparkStyle", group: g });
                },
              },
              {
                label: "스파크라인 지우기",
                onClick: async () => {
                  if (t.kind === "sheet" && (await run("스파크라인 지우기", () => api.deleteSparklines(t.index, range()))) !== null) await afterChange();
                },
              },
              { separator: true },
            ]
          : []),
        { label: memo ? "메모 편집" : "새 메모", shortcut: "Shift+F2", onClick: () => actions.newMemo() },
        ...(memo ? [{ label: "메모 삭제", onClick: () => actions.deleteMemo() }] : []),
        { separator: true },
        format,
      ];
    }
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
      } else if (mod && k === "p") {
        e.preventDefault();
        void openPrint();
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
    pasteAs: (kind) =>
      void pasteSpecialNow({
        what: kind === "Transpose" ? "All" : kind,
        op: "None",
        skip_blanks: false,
        transpose: kind === "Transpose",
      }),
    pasteSpecial: () => {
      if (!copyMark) return say("먼저 CellX에서 셀을 복사(Ctrl+C)하세요", true);
      setDialog({ kind: "pasteSpecial" });
    },
    highlight: (cf) => tabRef.current.kind === "sheet" && setDialog({ kind: "highlight", cf }),
    dataBar: (color, gradient) => void addCf(dataBarRule(color, gradient)),
    colorScale: (colors) => void addCf(colorScaleRule(colors)),
    iconSet: (i) => void addCf(iconSetRule(ICON_SETS[i].icons as [string, string][])),
    rating: () => void addCf(ratingRule()),
    clearCf: async (scope) => {
      const st = await onSheetOnly("규칙 지우기", (s) => api.clearConditionalFormats(s, scope === "selection" ? range() : null));
      if (st) await afterChange(st);
    },
    manageCf: () => tabRef.current.kind === "sheet" && setDialog({ kind: "cfManager" }),
    insertChart: (k) => void insertChart(k),
    insertPicture: () => void insertPicture(),
    insertShape: (k) => void insertShape(k),
    insertSparklines: (k) => insertSparklines(k),
    insertSlicer: () => void insertSlicer(),
    dataTable: () => {
      const rg = range();
      if (rg.r1 <= rg.r0 || rg.c1 <= rg.c0) return say("입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요", true);
      setDialog({ kind: "dataTable", range: rg });
    },
    scenarios: () => {
      const rg = range();
      const one = rg.r0 === rg.r1 && rg.c0 === rg.c1;
      setDialog({ kind: "scenarios", selection: `$${colName(rg.c0)}$${rg.r0}${one ? "" : `:$${colName(rg.c1)}$${rg.r1}`}` });
    },
    insertPivot: () => void openPivotDialog(),
    refreshAll: async () => {
      const st = await run("새로 고침", api.refreshPivots);
      if (st) {
        await afterChange(st);
        say("피벗 테이블을 새로 고쳤습니다");
      }
    },
    newMemo: () => {
      if (tabRef.current.kind !== "sheet") return say("메모는 일반 시트에 넣을 수 있습니다", true);
      setEditMemo(selectionRef.current.focus);
    },
    deleteMemo: async () => {
      const t = tabRef.current;
      const p = selectionRef.current.focus;
      if (t.kind !== "sheet" || !memoAt(p)) return;
      const st = await run("메모 삭제", () => api.setMemo(t.index, p.r, p.c, null));
      if (st) await afterChange(st);
    },
    toggleMemos: () => setShowMemos((v) => !v),
    sortAsc: () => void quickSort(false),
    sortDesc: () => void quickSort(true),
    sortDialog: async () => {
      if (sheetOrSay() === null) return;
      const rg = await workRange();
      setDialog({ kind: "sort", range: rg, header: guessHeader(rg) });
    },
    toggleFilter: () => void toggleFilter(),
    clearFilter: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const st = await run("필터 지우기", () => api.clearFilters(sheet));
      if (st) await afterChange(st);
    },
    reapplyFilter: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const st = await run("다시 적용", () => api.applyFilter(sheet));
      if (st) await afterChange(st);
    },
    hasFilter: () => !!filter,
    textToColumns: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const rg = range();
      if (rg.c0 !== rg.c1) return say("텍스트 나누기는 한 번에 한 열만 할 수 있습니다", true);
      const samples: string[] = [];
      for (let r = rg.r0; r <= Math.min(rg.r1, rg.r0 + 7); r++) samples.push(sourceRef.current?.cell(r, rg.c0)?.text ?? "");
      setDialog({ kind: "textToColumns", range: rg, samples });
    },
    flashFill: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const f = selectionRef.current.focus;
      const r = await run("빠른 채우기", () => api.flashFill(sheet, f.r, f.c));
      if (!r) return;
      await afterChange(r[1]);
      say(r[0] ? `빠른 채우기: ${formatCount(r[0])}개 셀을 채웠습니다` : "빠른 채우기: 채울 셀이 없습니다");
    },
    removeDuplicates: async () => {
      if (sheetOrSay() === null) return;
      const rg = await workRange();
      setDialog({ kind: "duplicates", range: rg, header: guessHeader(rg) });
    },
    validation: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const f = selectionRef.current.focus;
      setDialog({ kind: "validation", current: await api.validationAt(sheet, f.r, f.c).catch(() => null) });
    },
    goalSeek: () => sheetOrSay() !== null && setDialog({ kind: "goalSeek" }),
    replace: () => {
      setFindTab("바꾸기");
      setFindOpen(true);
    },
    goToDialog: () => nameBox.current?.focus(),
    hyperlink: async () => {
      const sheet = sheetOrSay();
      if (sheet === null) return;
      const f = selectionRef.current.focus;
      const current = await api.linkAt(sheet, f.r, f.c).catch(() => null);
      setDialog({ kind: "hyperlink", text: sourceRef.current?.cell(f.r, f.c)?.text ?? "", current });
    },
    tracePrecedents: () => void trace("prec"),
    traceDependents: () => void trace("dep"),
    removeArrows: () => {
      setArrows([]);
      traceFrontier.current = null;
    },
    toggleShowFormulas: () => toggleShowFormulas(),
    evaluateFormula: async () => {
      const t = tabRef.current;
      if (t.kind !== "sheet") return;
      const f = selectionRef.current.focus;
      const d = await api.cellDetail(t.index, f.r, f.c).catch(() => null);
      if (!d?.input.startsWith("=")) return say("수식이 있는 셀을 선택하세요", true);
      setDialog({ kind: "evaluate", sheet: t.index, row: f.r, col: f.c });
    },
    nameManager: async () => setDialog({ kind: "names", list: await api.names().catch(() => []), startNew: false }),
    defineName: async () => setDialog({ kind: "names", list: await api.names().catch(() => []), startNew: true }),
    print: () => void openPrint(),
    group: async (rows) => {
      const rg = range();
      const st = await onSheetOnly("그룹", (s) => (rows ? api.group(s, true, rg.r0, rg.r1) : api.group(s, false, rg.c0, rg.c1)));
      if (st) await afterChange(st);
    },
    ungroup: async (rows) => {
      const rg = range();
      const st = await onSheetOnly("그룹 해제", (s) => (rows ? api.ungroup(s, true, rg.r0, rg.r1) : api.ungroup(s, false, rg.c0, rg.c1)));
      if (st) await afterChange(st);
    },
    clearOutline: async () => {
      const st = await onSheetOnly("윤곽 지우기", (s) => api.clearOutline(s));
      if (st) await afterChange(st);
    },
    showDetail: async (show) => {
      const t = tabRef.current;
      const o = outline;
      if (t.kind !== "sheet" || !o) return;
      const f = selectionRef.current.focus;
      // The group of the active row (or the one ending just above it).
      const r = o.rows.has(f.r) ? f.r : o.rows.has(f.r - 1) ? f.r - 1 : null;
      if (r === null) return say("그룹 안의 셀을 선택하세요", true);
      const st = await run(show ? "펼치기" : "접기", () => api.toggleGroup(t.index, true, r, o.rows.get(r)!, !show));
      if (st) await afterChange(st);
    },
    subtotal: async () => {
      if (sheetOrSay() === null) return;
      setDialog({ kind: "subtotal", range: await workRange() });
    },
    formatAsTable: async (style) => {
      if (sheetOrSay() === null) return;
      if (activeTable) return void updateTable(activeTable, { style });
      const rg = await workRange();
      setDialog({ kind: "createTable", range: rg, header: guessHeader(rg) || true, style });
    },
    tableName: (name) => activeTable && void updateTable(activeTable, { new_name: name }),
    tableToggle: (key, value) => activeTable && void updateTable(activeTable, { [key]: value }),
    tableToRange: async () => {
      if (!activeTable) return;
      const st = await run("범위로 변환", () => api.convertTableToRange(activeTable.name));
      if (st) await afterChange(st);
    },
    protectBook: () => {
      if (!info?.book_protected) return setDialog({ kind: "bookProtect" });
      if (info.book_password) return setDialog({ kind: "bookUnprotect" });
      void run("통합 문서 보호 해제", () => api.unprotectBook(null)).then(() => refreshInfo());
    },
    customViews: () => setDialog({ kind: "customViews" }),
    date1904: () => {
      const on = !info?.date1904;
      const msg = on
        ? "1904 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 뒤의 날짜로 표시됩니다. 계속할까요?"
        : "1900 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 앞의 날짜로 표시됩니다. 계속할까요?";
      if (!window.confirm(msg)) return;
      void run("날짜 체계 변경", () => api.setDate1904(on)).then((st) => {
        if (st) void afterChange(st);
      });
    },
    protectSheet: () => {
      if (sheetOrSay() === null) return;
      if (protection?.protected) {
        if (protection.has_password) setDialog({ kind: "unprotect" });
        else void onSheetOnly("보호 해제", (s) => api.unprotectSheet(s, null)).then(() => afterChange());
      } else setDialog({ kind: "protect" });
    },
    orientation: (landscape) => pageSetupState && void changePageSetup({ ...pageSetupState, landscape }),
    fitWidth: (fit) => pageSetupState && void changePageSetup({ ...pageSetupState, fit }),
    printGrid: (v) => pageSetupState && void changePageSetup({ ...pageSetupState, gridlines: v }),
    printHeadings: (v) => pageSetupState && void changePageSetup({ ...pageSetupState, headings: v }),
    printArea: (set) => {
      if (!pageSetupState) return;
      void changePageSetup({ ...pageSetupState, print_area: set ? range() : null });
      say(set ? `인쇄 영역: ${rangeLabel(selection.anchor, selection.focus)}` : "인쇄 영역을 해제했습니다");
    },
    headerFooter: () => setSheetDialog({ kind: "headerFooter" }),
    printTitles: async () => {
      if (!pageSetupState) return;
      const cur = pageSetupState.title_rows;
      const v = await ask("인쇄 제목", "반복할 행 (예: 1:1, 비우면 없음)", cur ? `${cur[0]}:${cur[1]}` : "");
      if (v === null) return;
      const m = /^\$?(\d+)(?::\$?(\d+))?$/.exec(v.trim());
      void changePageSetup({ ...pageSetupState, title_rows: m ? [Number(m[1]), Number(m[2] ?? m[1])] : null });
    },
    pageBreak: async (kind) => {
      const t = tabRef.current;
      if (t.kind !== "sheet") return;
      const f = selectionRef.current.focus;
      if (kind === "insert") await api.insertPageBreak(t.index, f.r, f.c).catch(fail);
      else if (kind === "remove") await api.removePageBreak(t.index, f.r, f.c).catch(fail);
      else await api.resetPageBreaks(t.index).catch(fail);
      setShowPageBreaks(true);
      setPageSetupState(await api.pageSetup(t.index));
      await refreshInfo();
    },
    togglePageBreaks: () => setShowPageBreaks((v) => !v),
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
          이 화면은 데스크톱 앱 안에서 실행됩니다. <code>pnpm tauri dev</code>{tr("로 실행하세요.")}
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
        <label className="language-picker">
          <span>{t("language")}</span>
          <select value={locale} onChange={(event) => setLocale(event.target.value as Locale)} aria-label={t("language")}>
            <option value="ko-KR">{tr("한국어")}</option>
            <option value="en-US">English</option>
            <option value="ja-JP">日本語</option>
            <option value="zh-CN">简体中文</option>
            <option value="zh-TW">繁體中文</option>
          </select>
        </label>
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
        showMemos={showMemos}
        hasMemo={onSheetTab && !!memoAt(selection.focus)}
        filterOn={!!filter}
        table={activeTable}
        protectedSheet={!!protection?.protected}
        pageSetup={pageSetupState}
        showPageBreaks={showPageBreaks}
        showFormulas={sheetIndex >= 0 && formulaSheets.has(sheetIndex)}
        protectedBook={!!info?.book_protected}
        date1904={!!info?.date1904}
        hasArrows={arrows.length > 0}
      />

      <div className="formula-bar">
        <input
          ref={nameBox}
          className="name-box"
          defaultValue={
            pictures.find((p) => p.id === selectedPicture)?.name ?? shapes.find((p) => p.id === selectedShape)?.name ?? rangeLabel(selection.anchor, selection.focus)
          }
          key={`${tabKey(tab)}:${rangeLabel(selection.anchor, selection.focus)}:${pictures.find((p) => p.id === selectedPicture)?.name ?? ""}:${shapes.find((p) => p.id === selectedShape)?.name ?? ""}`}
          onKeyDown={(e) => {
            if (e.key === "Escape") return focusGrid();
            if (e.key !== "Enter") return;
            const v = (e.target as HTMLInputElement).value.trim();
            // With a picture selected, the name box renames it, as in Excel.
            const shape = shapes.find((p) => p.id === selectedShape);
            if (shape) {
              if (v && v !== shape.name) void updateShape({ ...shape, name: v });
              focusGrid();
              return;
            }
            const pic = pictures.find((p) => p.id === selectedPicture);
            if (pic) {
              if (v && v !== pic.name) void updatePicture({ ...pic, name: v });
              focusGrid();
              return;
            }
            void (async () => {
              if (await goTo(v)) return;
              // A new name in the name box names the selection, as in Excel.
              const t = tabRef.current;
              if (t.kind === "sheet" && /^[\p{L}_\\][\p{L}\p{N}_.]*$/u.test(v)) {
                const rg = range();
                const ref = `${quoteSheet(sheetName(t.index))}!$${colName(rg.c0)}$${rg.r0}${rg.r0 === rg.r1 && rg.c0 === rg.c1 ? "" : `:$${colName(rg.c1)}$${rg.r1}`}`;
                try {
                  await api.addName(v, null, `=${ref}`);
                  say(`이름 「${v}」을(를) 정의했습니다: ${ref}`);
                  await refreshInfo();
                } catch (err) {
                  fail(err);
                }
              } else say(`셀 주소나 이름이 올바르지 않습니다: ${v}`, true);
              focusGrid();
            })();
          }}
          onFocus={(e) => e.target.select()}
        />
        <div className="fb-buttons">
          <button title={tr("취소")} disabled={!session} onMouseDown={(e) => e.preventDefault()} onClick={cancelEdit}>
            ✕
          </button>
          <button title={tr("입력")} disabled={!session} onMouseDown={(e) => e.preventDefault()} onClick={() => void commit("none")}>
            ✓
          </button>
          <button title={tr("함수 삽입")} onMouseDown={(e) => e.preventDefault()} onClick={() => actions.insertFunction("SUM")}>
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
                      const text = e.clipboardData.getData("text/plain");
                      // An image alone on the clipboard (a screenshot) becomes a picture, as in Excel.
                      const image = Array.from(e.clipboardData.files).find((f) => f.type.startsWith("image/"));
                      if (image && !text) void pastePicture(image);
                      else void doPaste(text);
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
                const lc = linkClick.current;
                linkClick.current = null;
                const f = selectionRef.current.focus;
                if (lc && Date.now() - lc.t < 500 && f.r === lc.r && f.c === lc.c && selectionRef.current.anchor.r === lc.r) void followLink(lc);
              }}
              filterButtons={
                filter && onSheetTab ? { row: filter.range.r0, c0: filter.range.c0, c1: filter.range.c1, active: filter.columns.map((c) => c.col) } : null
              }
              onFilterButton={(c, x, y) => void openFilterMenu(c, x, y)}
              listButton={listItems && !session ? selection.focus : null}
              outline={outline}
              onOutlineToggle={async (rows, index, level, collapse) => {
                const st = await onSheetOnly("윤곽", (s) => api.toggleGroup(s, rows, index, level, collapse));
                if (st) await afterChange(st);
              }}
              onOutlineLevel={async (rows, level) => {
                const st = await onSheetOnly("윤곽", (s) => api.showOutlineLevel(s, rows, level));
                if (st) await afterChange(st);
              }}
              pageBreaks={pageBreakLines}
              onListButton={(x, y) => setListOpen({ x, y })}
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
              onHover={(h) => {
                const m = h && memoAt({ r: h.r, c: h.c }) ? { r: h.r, c: h.c } : null;
                setHoverMemo((cur) => (cur?.r === m?.r && cur?.c === m?.c ? cur : m));
              }}
              renderOverlay={(geo) => {
                const memoPositions: Pos[] = [];
                if (source instanceof SheetSource) {
                  if (showMemos) memoPositions.push(...source.memos().map((m) => ({ r: m.row, c: m.col })));
                  if (hoverMemo && !memoPositions.some((p) => p.r === hoverMemo.r && p.c === hoverMemo.c)) memoPositions.push(hoverMemo);
                }
                if (editMemo) {
                  const i = memoPositions.findIndex((p) => p.r === editMemo.r && p.c === editMemo.c);
                  if (i >= 0) memoPositions.splice(i, 1);
                }
                const box = (p: Pos) => {
                  const rc = geo.cellRect(p.r, p.c);
                  return { x: rc.x + rc.w + 6, y: Math.max(0, rc.y - 4) };
                };
                return (
                  <>
                    <SparklineLayer sparklines={sparklines} geo={geo} />
                    <SlicerLayer
                      slicers={slicers}
                      geo={geo}
                      selected={selectedSlicer}
                      onSelect={setSelectedSlicer}
                      onChange={async (s) => {
                        if ((await run("슬라이서", () => api.updateSlicer(s))) !== null) await afterChange();
                      }}
                      onPick={(id, v) => void pickSlicer(id, v)}
                      onContextMenu={(s, x, y) =>
                        setMenu({
                          x,
                          y,
                          items: [
                            { label: `"${s.caption}"에서 필터 해제`, onClick: () => void pickSlicer(s.id, null) },
                            { label: "슬라이서 설정...", onClick: () => setDialog({ kind: "slicerSettings", slicer: s }) },
                            { separator: true },
                            {
                              label: `"${s.caption}" 제거`,
                              onClick: async () => {
                                if ((await run("슬라이서 제거", () => api.deleteSlicer(s.id))) !== null) await afterChange();
                              },
                            },
                          ],
                        })
                      }
                    />
                    {source instanceof SheetSource && <TraceLayer arrows={arrows} sheet={source.sheet} geo={geo} />}
                    <ShapeLayer
                      shapes={shapes}
                      geo={geo}
                      selected={selectedShape}
                      editing={editingShape}
                      onSelect={(id) => {
                        if (sessionRef.current) void commit("none");
                        setSelectedChart(null);
                        setSelectedPicture(null);
                        setSelectedShape(id);
                        focusGrid();
                      }}
                      onEdit={(id) => {
                        setEditingShape(id);
                        if (id === null) focusGrid();
                      }}
                      onChange={(s) => void updateShape(s)}
                      onContextMenu={(s, x, y) =>
                        setMenu({
                          x,
                          y,
                          items: [
                            ...(s.kind === "line" || s.kind === "arrow" ? [] : [{ label: "텍스트 편집", onClick: () => setEditingShape(s.id) }]),
                            { label: "도형 서식...", onClick: () => setDialog({ kind: "shapeFormat", shape: s }) },
                            { separator: true },
                            { label: "삭제", shortcut: "Delete", onClick: () => void deleteShape(s.id) },
                          ],
                        })
                      }
                    />
                    <PictureLayer
                      pictures={pictures}
                      geo={geo}
                      selected={selectedPicture}
                      onSelect={(id) => {
                        if (sessionRef.current) void commit("none");
                        setSelectedChart(null);
                        setSelectedShape(null);
    setSelectedSlicer(null);
                        setSelectedPicture(id);
                        focusGrid();
                      }}
                      onChange={(p) => void updatePicture(p)}
                      onContextMenu={(p, x, y) =>
                        setMenu({
                          x,
                          y,
                          items: [
                            { label: "원래 크기로", onClick: () => void resetPictureSize(p) },
                            {
                              label: "가로세로 비율 맞추기",
                              onClick: () => void updatePicture({ ...p, anchor: { ...p.anchor, height: Math.round(p.anchor.width / (p.ratio || 1)) } }),
                            },
                            { separator: true },
                            { label: "삭제", shortcut: "Delete", onClick: () => void deletePicture(p.id) },
                          ],
                        })
                      }
                    />
                    <ChartLayer
                      charts={charts}
                      geo={geo}
                      version={chartVersion}
                      selected={selectedChart}
                      onSelect={(id) => {
                        if (sessionRef.current) void commit("none");
                        setSelectedPicture(null);
    setSelectedShape(null);
    setSelectedSlicer(null);
                        setSelectedChart(id);
                        focusGrid();
                      }}
                      onChange={(c) => void updateChart(c)}
                      onEdit={(c) => setDialog({ kind: "chart", chart: c })}
                      onContextMenu={(c, x, y) =>
                        setMenu({
                          x,
                          y,
                          items: [
                            { label: "차트 편집...", onClick: () => setDialog({ kind: "chart", chart: c }) },
                            { separator: true },
                            ...CHART_KINDS.map((k) => ({ label: k.label, checked: c.kind === k.kind, onClick: () => void updateChart({ ...c, kind: k.kind }) })),
                            { separator: true },
                            { label: "범례", checked: c.legend, onClick: () => void updateChart({ ...c, legend: !c.legend }) },
                            { label: "삭제", shortcut: "Delete", onClick: () => void deleteChart(c.id) },
                          ],
                        })
                      }
                    />
                    {memoPositions.map((p) => {
                      const m = memoAt(p);
                      if (!m) return null;
                      const b = box(p);
                      return <MemoBox key={`m${p.r}:${p.c}`} x={b.x} y={b.y} text={m.text} author={m.author} editing={false} />;
                    })}
                    {inputTip && !editMemo && !vAlert && (
                      <div
                        className="input-tip"
                        style={{
                          left: geo.cellRect(selection.focus.r, selection.focus.c).x + 12,
                          top: geo.cellRect(selection.focus.r, selection.focus.c).y + geo.cellRect(selection.focus.r, selection.focus.c).h + 4,
                        }}
                      >
                        {inputTip.title && <b>{inputTip.title}</b>}
                        <div>{inputTip.message}</div>
                      </div>
                    )}
                    {editMemo && (
                      <MemoBox
                        key={`edit${editMemo.r}:${editMemo.c}`}
                        x={box(editMemo).x}
                        y={box(editMemo).y}
                        text={memoAt(editMemo)?.text ?? ""}
                        author={memoAt(editMemo)?.author ?? ""}
                        editing
                        onSave={(text) => void saveMemo(editMemo, text)}
                        onCancel={() => {
                          setEditMemo(null);
                          focusGrid();
                        }}
                      />
                    )}
                  </>
                );
              }}
            />
          )}
          {findOpen && (
            <FindReplaceDialog
              initialTab={findTab}
              message={findMsg}
              onFind={(o) => void find(o)}
              onReplace={(o) => void replaceOne(o)}
              onReplaceAll={(o) => void replaceAll(o)}
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
            onExecuted={async (table) => {
              await refreshInfo();
              sources.current.delete(tabKey({ kind: "data", name: table.name }));
              switchTab({ kind: "data", name: table.name });
              say(`${table.name}: ${formatCount(table.rows)}행`);
            }}
            onCreate={async (sql, name) => {
              const table = await runDataJob("쿼리 실행 중", () => api.queryToTable(sql, name));
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
          {info?.sheets.map((name, i) => {
            const meta = info.tabs?.[i];
            if (meta?.hidden) return null;
            const on = tab.kind === "sheet" && tab.index === i;
            const visible = info.tabs?.filter((x) => !x.hidden).length ?? info.sheets.length;
            const anyHidden = info.tabs?.some((x) => x.hidden) ?? false;
            return (
            <button
              key={`s${i}`}
              className={`tab ${on ? "on" : ""} ${dragOver === i ? "dragover" : ""}`}
              style={meta?.color ? (on ? { boxShadow: `inset 0 -3px 0 ${meta.color}` } : { background: meta.color, color: "#fff" }) : undefined}
              draggable
              onDragStart={(e) => {
                dragTab.current = i;
                e.dataTransfer.effectAllowed = "copyMove";
              }}
              onDragOver={(e) => {
                if (dragTab.current === null) return;
                e.preventDefault();
                setDragOver(i);
              }}
              onDragLeave={() => setDragOver((d) => (d === i ? null : d))}
              onDrop={(e) => {
                e.preventDefault();
                const from = dragTab.current;
                dragTab.current = null;
                setDragOver(null);
                if (from === null) return;
                // Ctrl+drag copies the sheet, as in Excel.
                if (e.ctrlKey) copySheet(from, i);
                else if (from !== i) moveSheet(from, i);
              }}
              onDragEnd={() => {
                dragTab.current = null;
                setDragOver(null);
              }}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                const s = sessionRef.current;
                switchTab({ kind: "sheet", index: i }, !!s && pointable(s));
              }}
              onDoubleClick={() => renameSheet(i)}
              onContextMenu={(e) => {
                e.preventDefault();
                const at = { x: e.clientX, y: e.clientY - 240 };
                setMenu({
                  x: at.x,
                  y: at.y,
                  items: [
                    { label: "삽입", onClick: insertSheet },
                    { label: "삭제", disabled: visible <= 1, onClick: () => deleteSheet(i) },
                    { label: "이름 바꾸기", onClick: () => renameSheet(i) },
                    { label: "이동/복사...", onClick: () => setSheetDialog({ kind: "move", sheet: i }) },
                    { separator: true },
                    { label: "탭 색...", onClick: () => setTabColorAt({ sheet: i, x: at.x, y: at.y }) },
                    { separator: true },
                    { label: "숨기기", disabled: visible <= 1, onClick: () => hideSheet(i) },
                    { label: "숨기기 취소...", disabled: !anyHidden, onClick: () => setSheetDialog({ kind: "unhide" }) },
                  ],
                });
              }}
            >
              {info?.protected?.[i] && <span className="lock">🔒</span>}
              {name}
            </button>
            );
          })}
          <button className="tab add" onMouseDown={(e) => e.preventDefault()} onClick={insertSheet} title={tr("새 시트 (Shift+F11)")}>
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
        {stats && onSheetTab && stats.bridge_calls > 0 && <span title={tr("데이터 엔진 계산 시간")}>데이터 엔진 {stats.bridge_ms.toFixed(0)} ms</span>}
        {stats?.not_converged && (
          <span className="warn" title={tr("데이터 시트 결과와 셀이 서로를 참조해 계산이 끝나지 않았습니다. 일부 값이 최신이 아닐 수 있습니다. 순환 참조를 확인하세요.")}>
            ⚠ {tr("계산이 수렴하지 않음")}
          </span>
        )}
        {filter && filter.columns.length > 0 && source instanceof SheetSource && (
          <span>
            {formatCount(filter.range.r1 - filter.range.r0)}개 중 {formatCount(
              Array.from({ length: filter.range.r1 - filter.range.r0 }, (_, i) => filter.range.r0 + 1 + i).filter((r) => source.rows.size(r) > 0).length,
            )}
            {tr("개의 레코드가 있습니다")}
          </span>
        )}
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
      <DataJobProgress active={dataJob} />
      {dialog?.kind === "importData" && (
        <ImportDataDialog
          path={dialog.path}
          suggested={dialog.suggested}
          csv={dialog.csv}
          preview={(encoding) => api.previewDataFile(dialog.path, locale, encoding)}
          onDone={(v) => {
            dialog.resolve(v);
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "exportEncoding" && (
        <ExportEncodingDialog
          onDone={(v) => {
            dialog.resolve(v);
            setDialog(null);
            focusGrid();
          }}
        />
      )}
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
      {filterMenu && filter && (
        <FilterMenu
          key={filterMenu.col}
          x={filterMenu.x}
          y={filterMenu.y}
          col={filterMenu.col}
          values={filterMenu.values}
          numeric={filterMenu.numeric}
          current={filter.columns.find((c) => c.col === filterMenu.col) ?? null}
          onClose={() => {
            setFilterMenu(null);
            focusGrid();
          }}
          onSort={async (descending) => {
            setFilterMenu(null);
            const st = await onSheetOnly("정렬", (s) => api.sortRange(s, filter.range, [{ col: filterMenu.col, descending }], true));
            if (st) await afterChange(st);
            focusGrid();
          }}
          onApply={async (f) => {
            const col = filterMenu.col;
            setFilterMenu(null);
            const st = await onSheetOnly("필터", (s) => api.setFilterColumn(s, f ?? { col, values: null, conditions: [], and: true }));
            if (st) await afterChange(st);
            focusGrid();
          }}
        />
      )}
      {listOpen && listItems && (
        <ListDropdown
          x={listOpen.x}
          y={listOpen.y}
          items={listItems}
          current={detail?.text ?? ""}
          onClose={() => {
            setListOpen(null);
            focusGrid();
          }}
          onPick={async (v) => {
            setListOpen(null);
            const f = selectionRef.current.focus;
            const st = await onSheetOnly("입력", (s) => api.setCell(s, f.r, f.c, v));
            if (st) await afterChange(st);
            focusGrid();
          }}
        />
      )}
      {vAlert && (
        <ValidationAlert
          failure={vAlert.failure}
          onDone={async (choice) => {
            const s = vAlert.session;
            setVAlert(null);
            if (tabKey(tabRef.current) !== tabKey(s.tab)) switchTab(s.tab, true);
            if (choice === "accept") {
              await writeSession(s, false);
              return;
            }
            setSelection({ anchor: s.pos, focus: s.pos });
            if (choice === "retry") {
              // Back into the cell with the rejected entry selected, as in Excel.
              setSession({ ...s, mode: "edit", point: null });
              requestAnimationFrame(() => {
                const el = cellInput.current;
                el?.focus({ preventScroll: true });
                el?.setSelectionRange(0, s.text.length);
              });
            } else focusGrid();
          }}
        />
      )}
      {dialog?.kind === "sort" && (
        <SortDialog
          columns={columnChoices(dialog.range)}
          initialHeader={dialog.header}
          initialCol={selection.focus.c}
          onDone={async (r) => {
            const rg = dialog.range;
            setDialog(null);
            focusGrid();
            if (!r) return;
            const st = await onSheetOnly("정렬", (s) => api.sortRange(s, rg, r.keys, r.hasHeader));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "textToColumns" && (
        <TextToColumnsDialog
          samples={dialog.samples}
          dest={addr({ r: dialog.range.r0, c: dialog.range.c0 })}
          onDone={async (o) => {
            const rg = dialog.range;
            setDialog(null);
            focusGrid();
            if (!o) return;
            const d = parseAddr(o.destText.replace(/\$/g, "").split("!").pop() ?? "") ?? { r: rg.r0, c: rg.c0 };
            const { destText: _ignored, ...opts } = o;
            const st = await onSheetOnly("텍스트 나누기", (s) => api.textToColumns(s, rg, { ...opts, dest_row: d.r, dest_col: d.c }));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "duplicates" && (
        <DuplicatesDialog
          columns={columnChoices(dialog.range)}
          initialHeader={dialog.header}
          onDone={async (r) => {
            const rg = dialog.range;
            setDialog(null);
            focusGrid();
            if (!r) return;
            const res = await onSheetOnly("중복 제거", (s) => api.removeDuplicates(s, rg, r.cols, r.hasHeader));
            if (!res) return;
            await afterChange();
            say(
              res.removed
                ? `중복된 값 ${formatCount(res.removed)}개를 찾아 제거했습니다. 고유한 값 ${formatCount(res.remaining)}개가 남아 있습니다.`
                : "중복된 값이 없습니다.",
            );
          }}
        />
      )}
      {dialog?.kind === "validation" && (
        <ValidationDialog
          initial={dialog.current}
          onDone={async (v) => {
            setDialog(null);
            focusGrid();
            if (!v) return;
            const st =
              v === "clear" || v.kind === "Any"
                ? await onSheetOnly("유효성 검사", (s) => api.clearValidation(s, range()))
                : await onSheetOnly("유효성 검사", (s) => api.setValidation(s, range(), v));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "names" && (
        <NameManagerDialog
          names={dialog.list}
          sheets={info?.sheets ?? []}
          startNew={dialog.startNew}
          suggestion={(() => {
            const rg = range();
            const t = tabRef.current;
            const sn = t.kind === "sheet" ? quoteSheet(sheetName(t.index)) : "Sheet1";
            return `=${sn}!$${colName(rg.c0)}$${rg.r0}${rg.r0 === rg.r1 && rg.c0 === rg.c1 ? "" : `:$${colName(rg.c1)}$${rg.r1}`}`;
          })()}
          onAdd={async (name, scope, formula) => {
            try {
              await api.addName(name, scope, formula);
              setDialog({ kind: "names", list: await api.names(), startNew: false });
              await afterChange();
              return true;
            } catch (e) {
              fail(e);
              return false;
            }
          }}
          onUpdate={async (old, name, scope, formula) => {
            try {
              await api.updateName(old.name, old.scope, name, scope, formula);
              setDialog({ kind: "names", list: await api.names(), startNew: false });
              await afterChange();
              return true;
            } catch (e) {
              fail(e);
              return false;
            }
          }}
          onDelete={async (n) => {
            await api.deleteName(n.name, n.scope).catch(fail);
            setDialog({ kind: "names", list: await api.names(), startNew: false });
            await afterChange();
          }}
          onClose={() => {
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "hyperlink" && (
        <HyperlinkDialog
          text={dialog.text}
          current={dialog.current}
          sheets={info?.sheets ?? []}
          onDone={async (r) => {
            setDialog(null);
            focusGrid();
            if (!r) return;
            const f = selectionRef.current.focus;
            const st =
              r === "remove"
                ? await onSheetOnly("링크 제거", (s) => api.deleteLink(s, f.r, f.c))
                : await onSheetOnly("링크", (s) => api.setLink(s, f.r, f.c, r.target, r.internal, r.text || null));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "bookProtect" && (
        <BookProtectDialog
          onDone={async (pw) => {
            setDialog(null);
            focusGrid();
            if (pw === undefined) return;
            if ((await run("통합 문서 보호", () => api.protectBook(pw))) !== null) await refreshInfo();
          }}
        />
      )}
      {dialog?.kind === "bookUnprotect" && (
        <PasswordDialog
          title={tr("통합 문서 보호 해제")}
          onDone={async (pw) => {
            setDialog(null);
            focusGrid();
            if (pw === null) return;
            if ((await run("통합 문서 보호 해제", () => api.unprotectBook(pw))) !== null) await refreshInfo();
          }}
        />
      )}
      {dialog?.kind === "customViews" && (
        <CustomViewsDialog
          active={sheetIndex >= 0 ? sheetIndex : 0}
          onShown={async (s) => {
            setDialog(null);
            await afterChange();
            switchTab({ kind: "sheet", index: s });
            focusGrid();
          }}
          onError={(e) => say(String(e), true)}
          onClose={() => {
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "dataTable" && (
        <DataTableDialog
          onDone={async (r) => {
            const rg = dialog.range;
            setDialog(null);
            focusGrid();
            const t = tabRef.current;
            if (!r || t.kind !== "sheet") return;
            const st = await run("데이터 표", () => api.addDataTable(t.index, rg, r.row.trim() || null, r.col.trim() || null));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "scenarios" && tab.kind === "sheet" && (
        <ScenarioDialog
          sheet={tab.index}
          selection={dialog.selection}
          onShown={() => void afterChange()}
          onSummary={async (s) => {
            setDialog(null);
            await refreshInfo();
            await afterChange();
            switchTab({ kind: "sheet", index: s });
          }}
          onError={(e) => say(String(e), true)}
          onClose={() => {
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "sparkCreate" && (
        <SparklineCreateDialog
          kind={dialog.spark}
          data={dialog.data}
          location={dialog.location}
          onDone={(r) => {
            const k = dialog.spark;
            setDialog(null);
            focusGrid();
            if (r) void createSparklines(k, r.data, r.location);
          }}
        />
      )}
      {dialog?.kind === "sparkStyle" && (
        <SparklineStyleDialog
          group={dialog.group}
          onDone={async (g) => {
            setDialog(null);
            focusGrid();
            if (g && (await run("스파크라인 디자인", () => api.updateSparklineGroup(g))) !== null) await afterChange();
          }}
        />
      )}
      {dialog?.kind === "slicerInsert" && (
        <SlicerInsertDialog
          table={dialog.table}
          columns={dialog.columns}
          onDone={async (cols) => {
            const table = dialog.table;
            setDialog(null);
            focusGrid();
            const t = tabRef.current;
            if (!cols || t.kind !== "sheet") return;
            for (const c of cols) await run("슬라이서 삽입", () => api.addSlicer(table, c, t.index));
            await afterChange();
          }}
        />
      )}
      {dialog?.kind === "slicerSettings" && (
        <SlicerSettingsDialog
          slicer={dialog.slicer}
          onDone={async (s) => {
            setDialog(null);
            focusGrid();
            if (s && (await run("슬라이서 설정", () => api.updateSlicer(s))) !== null) await afterChange();
          }}
        />
      )}
      {dialog?.kind === "shapeFormat" && (
        <ShapeFormatDialog
          shape={dialog.shape}
          onDone={(s) => {
            setDialog(null);
            focusGrid();
            if (s) void updateShape(s);
          }}
        />
      )}
      {dialog?.kind === "evaluate" && (
        <EvaluateDialog
          sheet={dialog.sheet}
          sheetName={sheetName(dialog.sheet)}
          row={dialog.row}
          col={dialog.col}
          onClose={() => {
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "goalSeek" && (
        <GoalSeekDialog
          setCell={addr(selection.focus)}
          onDone={async (r) => {
            setDialog(null);
            focusGrid();
            if (!r) return;
            const a = parseAddr(r.setCell.replace(/\$/g, ""));
            const b = parseAddr(r.changing.replace(/\$/g, ""));
            if (!a || !b) return say("셀 주소가 올바르지 않습니다", true);
            const res = await onSheetOnly("목표값 찾기", (s) => api.goalSeek(s, [a.r, a.c], r.target, [b.r, b.c]));
            if (!res) return;
            await afterChange();
            say(
              res.found
                ? `셀 ${r.setCell.toUpperCase()}에 대한 목표값 찾기: 답을 찾았습니다. 목표값 ${r.target}, 현재값 ${formatNumber(res.result)} (${r.changing.toUpperCase()} = ${formatNumber(res.value)})`
                : `셀 ${r.setCell.toUpperCase()}의 목표값을 찾지 못했습니다. 가장 가까운 값: ${formatNumber(res.result)}`,
              !res.found,
            );
          }}
        />
      )}
      {printState && (
        <PrintPreviewDialog
          job={printState.job}
          setup={printState.setup}
          selectionOnly={printState.selection}
          hasSelection={(() => {
            const rg = range();
            return rg.r0 !== rg.r1 || rg.c0 !== rg.c1;
          })()}
          onScope={async (sel) => {
            const job = await buildPrintJob(sel);
            if (job) setPrintState((p) => (p ? { ...p, job, selection: sel } : p));
          }}
          onChange={(setup) => {
            setPrintState((p) => (p ? { ...p, setup } : p));
            void changePageSetup(setup);
          }}
          onPrint={doPrint}
          onClose={() => {
            setPrintState(null);
            focusGrid();
          }}
        />
      )}
      {printJob && pageSetupState && <PrintView job={printJob} setup={pageSetupState} />}
      {savePrompt && <SaveChangesDialog name={savePrompt.name} onChoose={savePrompt.resolve} />}
      {sheetDialog?.kind === "move" && info && (
        <MoveCopyDialog
          tabs={info.tabs}
          sheet={sheetDialog.sheet}
          onClose={() => setSheetDialog(null)}
          onOk={(to, copy) => {
            setSheetDialog(null);
            if (copy) copySheet(sheetDialog.sheet, to);
            else moveSheet(sheetDialog.sheet, to);
          }}
        />
      )}
      {sheetDialog?.kind === "unhide" && info && (
        <UnhideDialog
          tabs={info.tabs}
          onClose={() => setSheetDialog(null)}
          onOk={(sheet) => {
            setSheetDialog(null);
            unhideSheet(sheet);
          }}
        />
      )}
      {sheetDialog?.kind === "headerFooter" && pageSetupState && (
        <HeaderFooterDialog
          setup={pageSetupState}
          onClose={() => setSheetDialog(null)}
          onOk={(ps) => {
            setSheetDialog(null);
            void changePageSetup(ps);
          }}
        />
      )}
      {tabColorAt && (
        <>
          <div className="menu-backdrop" style={{ position: "fixed", inset: 0, zIndex: 49 }} onMouseDown={() => setTabColorAt(null)} />
          <div className="sheet-color-pop" style={{ left: tabColorAt.x, top: Math.max(8, tabColorAt.y) }}>
            <ColorPicker
              noneLabel={tr("색 없음")}
              onPick={(c) => {
                const sheet = tabColorAt.sheet;
                setTabColorAt(null);
                void sheetOp("탭 색", () => api.setSheetColor(sheet, c));
              }}
            />
          </div>
        </>
      )}
      {recovered.length > 0 && !savePrompt && (
        <RecoveryDialog
          files={recovered}
          onOpen={(id) => void openRecovered(id)}
          onDiscard={(id) => {
            void api.recoveryDiscard(id).catch(fail);
            setRecovered((list) => list.filter((f) => f.id !== id));
          }}
          onClose={() => setRecovered([])}
        />
      )}
      {msgBox && (
        <MessageBox
          message={msgBox}
          onClose={() => {
            setMsgBox(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "subtotal" && (
        <SubtotalDialog
          columns={columnChoices(dialog.range)(true)}
          initialGroup={dialog.range.c0}
          onDone={async (r) => {
            const rg = dialog.range;
            setDialog(null);
            focusGrid();
            if (!r) return;
            const st =
              r === "removeAll"
                ? await onSheetOnly("부분합 제거", (s) => api.removeSubtotals(s, rg))
                : await onSheetOnly("부분합", (s) => api.subtotal(s, rg, { group_col: r.groupCol, func: r.func, value_cols: r.valueCols, replace: r.replace }));
            if (st) await afterChange(st);
          }}
        />
      )}
      {dialog?.kind === "createTable" && (
        <CreateTableDialog
          range={`$${colName(dialog.range.c0)}$${dialog.range.r0}:$${colName(dialog.range.c1)}$${dialog.range.r1}`}
          header={dialog.header}
          onDone={async (r) => {
            const style = dialog.style;
            setDialog(null);
            focusGrid();
            if (!r) return;
            const [a, b] = r.range.replace(/\$/g, "").split(":");
            const pa = parseAddr(a ?? "");
            const pb = parseAddr(b ?? a ?? "");
            if (!pa || !pb) return say("범위가 올바르지 않습니다", true);
            const t = await onSheetOnly("표 만들기", (s) => api.createTable(s, normalize(pa, pb), r.header, style));
            if (t) {
              await afterChange();
              say(`${t.name}을(를) 만들었습니다. 수식에서 ${t.name}[${t.columns[t.columns.length - 1] ?? "열"}]처럼 쓸 수 있습니다.`);
            }
          }}
        />
      )}
      {dialog?.kind === "protect" && (
        <ProtectDialog
          onDone={async (r) => {
            setDialog(null);
            focusGrid();
            if (!r) return;
            await onSheetOnly("시트 보호", (s) => api.protectSheet(s, r.password, r.options));
            await afterChange();
            await refreshInfo();
          }}
        />
      )}
      {dialog?.kind === "unprotect" && (
        <PasswordDialog
          title={tr("시트 보호 해제")}
          onDone={async (pw) => {
            setDialog(null);
            focusGrid();
            if (pw === null) return;
            const ok = await onSheetOnly("보호 해제", (s) => api.unprotectSheet(s, pw).then(() => true));
            if (ok) {
              await afterChange();
              await refreshInfo();
            }
          }}
        />
      )}
      {dialog?.kind === "pasteSpecial" && (
        <PasteSpecialDialog
          onDone={(o) => {
            setDialog(null);
            focusGrid();
            if (o) void pasteSpecialNow(o);
          }}
        />
      )}
      {dialog?.kind === "highlight" && (
        <HighlightDialog
          kind={dialog.cf}
          onDone={(r) => {
            const k = dialog.cf;
            setDialog(null);
            focusGrid();
            if (r) void addCf(highlightRule(k, r.a, r.b, r.style));
          }}
        />
      )}
      {dialog?.kind === "cfManager" && onSheetTab && (
        <CfManagerDialog
          sheet={sheetIndex}
          onChanged={() => void afterChange()}
          onClose={() => {
            setDialog(null);
            focusGrid();
          }}
        />
      )}
      {dialog?.kind === "pivot" && (
        <PivotDialog
          initial={dialog.def}
          tables={(info?.tables ?? []).map((t) => t.name)}
          sheetName={sheetName}
          onDone={async (def, newSheet) => {
            setDialog(null);
            if (!def) return focusGrid();
            const saved = await run("피벗 테이블", () => api.savePivot(def, newSheet));
            if (!saved) return;
            await refreshInfo();
            await afterChange();
            if (tabRef.current.kind !== "sheet" || tabRef.current.index !== saved.sheet) switchTab({ kind: "sheet", index: saved.sheet });
            setSelection({ anchor: { r: saved.row, c: saved.col }, focus: { r: saved.row, c: saved.col } });
          }}
        />
      )}
      {dialog?.kind === "chart" && (
        <ChartDialog
          chart={dialog.chart}
          onDone={(c) => {
            setDialog(null);
            focusGrid();
            if (c) void updateChart(c);
          }}
        />
      )}
      {dialog?.kind === "format" && detail && (
        <FormatCellsDialog
          style={detail.style}
          locked={!protection?.unlocked.some((u) => selection.focus.r >= u.r0 && selection.focus.r <= u.r1 && selection.focus.c >= u.c0 && selection.focus.c <= u.c1)}
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
      <span className="label">{tr("필터")}</span>
      <select value={column} onChange={(e) => setColumn(e.target.value)}>
        {source.info.columns.map((c) => (
          <option key={c.name}>{c.name}</option>
        ))}
      </select>
      <input
        placeholder={tr("조건: 서울, >=100, *카드*, <>취소")}
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
        {tr("수식에서")} <code>{source.info.name}[열]</code> {tr("· 수식 입력 중 열을 클릭하면 참조가 들어갑니다")}
      </span>
    </div>
  );
}

function SqlPanel(props: { tables: TableInfo[]; onCreate(sql: string, name: string): Promise<void>; onExecuted(table: TableInfo): Promise<void> }) {
  const first = props.tables[0];
  const groupCol = first?.columns.find((c) => c.kind === "Text") ?? first?.columns[0];
  const [sql, setSql] = useState(
    first ? `SELECT ${groupCol?.name ?? "*"}, COUNT(*) AS cnt\nFROM ${first.name}\nGROUP BY 1\nORDER BY 2 DESC` : "SELECT 1 AS x",
  );
  const [name, setName] = useState("result");
  return (
    <aside className="side">
      <h3>SQL</h3>
      <p className="muted">{tr("SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.")}</p>
      <textarea value={sql} onChange={(e) => setSql(e.target.value)} spellCheck={false} />
      <label>
        {tr("결과 시트 이름")}
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <button className="primary" onClick={() => props.onCreate(sql, name)}>
        {tr("실행")}
      </button>
      <h4>{tr("데이터 시트")}</h4>
      {props.tables.length === 0 && <p className="muted">{tr("아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.")}</p>}
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
      <h4>{tr("시트에서 쓰는 법")}</h4>
      <ul className="muted help">
        <li>{tr("=SUMIFS(표[금액], 표[지역], \"서울\")")}</li>
        <li>{tr("=COUNTIFS(표[상태], \"&lt;&gt;취소\")")}</li>
        <li>{tr("=XLOOKUP(A2, 표[주문번호], 표[금액])")}</li>
        <li>{tr("=DQUERY(\"SELECT ... FROM 표\")")}</li>
      </ul>
      <TransformPanel tables={props.tables} onExecuted={props.onExecuted} />
    </aside>
  );
}

function TransformPanel(props: { tables: TableInfo[]; onExecuted(table: TableInfo): Promise<void> }) {
  const [source, setSource] = useState(props.tables[0]?.name ?? "");
  const [columnsText, setColumnsText] = useState(props.tables[0]?.columns.map((c) => c.name).join(", ") ?? "");
  const [filterColumn, setFilterColumn] = useState("");
  const [filterValue, setFilterValue] = useState("");
  const [joinTable, setJoinTable] = useState("");
  const [joinLeft, setJoinLeft] = useState("");
  const [joinRight, setJoinRight] = useState("");
  const [groupColumnsText, setGroupColumnsText] = useState("");
  const [aggregateColumn, setAggregateColumn] = useState("");
  const [aggregateName, setAggregateName] = useState("total");
  const [aggregateFunction, setAggregateFunction] = useState<AggregateFunction>("sum");
  const [name, setName] = useState("정제 파이프라인");
  const [output, setOutput] = useState("clean_result");
  const [saved, setSaved] = useState<SavedPipeline[]>([]);
  const [connectors, setConnectors] = useState<SavedConnector[]>([]);
  const [connectorStatuses, setConnectorStatuses] = useState<ConnectorStatus[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [preview, setPreview] = useState<TransformPreview | null>(null);
  const [profile, setProfile] = useState<TableProfile | null>(null);
  const [job, setJob] = useState<TransformJobSnapshot | null>(null);
  const [jobHistory, setJobHistory] = useState<TransformJobSnapshot[]>([]);
  const [stepPreviews, setStepPreviews] = useState<{ step: number; preview: TransformPreview }[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const current = props.tables.find((t) => t.name === source);
  const joinedTable = props.tables.find((t) => t.name === joinTable);
  useEffect(() => {
    if (!props.tables.some((t) => t.name === source)) {
      const next = props.tables[0];
      setSource(next?.name ?? "");
      setColumnsText(next?.columns.map((c) => c.name).join(", ") ?? "");
    }
  }, [props.tables, source]);
  useEffect(() => {
    void api.transformPipelines().then(setSaved).catch((e) => setError(errorText(e)));
    void api.dataConnectors().then(setConnectors).catch((e) => setError(errorText(e)));
    void api.dataConnectorStatuses().then(setConnectorStatuses).catch((e) => setError(errorText(e)));
    void api.listTransformJobs().then(setJobHistory).catch((e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    const stored = saved.find((item) => item.id === selectedId && item.pipeline.source_table === source);
    if (!selectedId) {
      setGroupColumnsText("");
      setAggregateColumn("");
      setAggregateFunction("sum");
      setAggregateName("total");
    }
    const selected = stored?.pipeline.steps.find((step) => step.type === "select_columns");
    setColumnsText(selected?.type === "select_columns"
      ? selected.columns.join(", ")
      : current?.columns.map((c) => c.name).join(", ") ?? "");
    const filter = stored?.pipeline.steps.find((step) => step.type === "filter");
    if (filter?.type === "filter" && filter.predicate.op === "compare") {
      setFilterColumn(filter.predicate.column);
      setFilterValue(filter.predicate.value.type === "text" ? filter.predicate.value.value : "");
    } else {
      setFilterColumn(current?.columns[0]?.name ?? "");
      setFilterValue("");
    }
    const join = stored?.pipeline.steps.find((step) => step.type === "join");
    if (join?.type === "join") {
      setJoinTable(join.table);
      setJoinLeft(join.left_column);
      setJoinRight(join.right_column);
    } else {
      setJoinTable("");
      setJoinLeft("");
      setJoinRight("");
    }
  }, [source, selectedId, saved]);
  const buildPipeline = (): TransformPipeline => {
    if (!source) throw new Error("원본 데이터 시트를 선택하세요.");
    const steps: TransformPipeline["steps"] = [];
    if (filterColumn && filterValue.trim()) {
      steps.push({
        type: "filter",
        predicate: { op: "compare", column: filterColumn, operator: "equal", value: { type: "text", value: filterValue } },
      });
    }
    if (joinTable) {
      if (!joinLeft || !joinRight) throw new Error("조인 기준 열을 모두 선택하세요.");
      steps.push({ type: "join", table: joinTable, left_column: joinLeft, right_column: joinRight, kind: "left", suffix: joinTable });
    }
    const groups = groupColumnsText.split(",").map((v) => v.trim()).filter(Boolean);
    const aggregates = (aggregateColumn.trim() || aggregateFunction === "count") && aggregateName.trim()
      ? [{ function: aggregateFunction, column: aggregateColumn.trim() || null, as_name: aggregateName.trim() }]
      : [];
    if (groups.length || aggregates.length) {
      steps.push({ type: "group_by", columns: groups, aggregates });
    } else {
      const selected = columnsText.split(",").map((v) => v.trim()).filter(Boolean);
      if (selected.length) steps.push({ type: "select_columns", columns: selected });
    }
    return { source_table: source, steps };
  };
  const runBusy = async (work: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try { await work(); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const addConnector = () => runBusy(async () => {
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "CSV / TSV / Parquet", extensions: ["csv", "tsv", "parquet"] }],
    });
    if (typeof picked !== "string") return;
    const fileName = picked.split(/[\\/]/).pop() ?? "source.csv";
    const baseName = fileName.replace(/\.[^.]+$/, "") || "source";
    const safeName = `data_${baseName.toLowerCase().replace(/[^a-z0-9_]/g, "_")}`.slice(0, 64);
    const connector: SavedConnector = {
      id: crypto.randomUUID(),
      name: baseName.slice(0, 120),
      path: picked,
      table_name: safeName,
      locale: getActiveLocale(),
      schema: [],
      fingerprint: "",
    };
    const table = await api.saveDataConnector(connector);
    setConnectors(await api.dataConnectors());
    setConnectorStatuses(await api.dataConnectorStatuses());
    await props.onExecuted(table);
  });
  const profileNow = () => runBusy(async () => { setProfile(await api.profileDataTable(source)); });
  const previewNow = () => runBusy(async () => { setStepPreviews([]); setPreview(await api.previewTransform(buildPipeline())); });
  const previewStepsNow = () => runBusy(async () => {
    const pipeline = buildPipeline();
    if (!pipeline.steps.length) throw new Error("단계가 없습니다. 필터, 열 선택, 조인 또는 그룹 집계를 추가하세요.");
    setPreview(null);
    setStepPreviews([]);
    for (let index = 0; index < pipeline.steps.length; index += 1) {
      const partial = { source_table: pipeline.source_table, steps: pipeline.steps.slice(0, index + 1) };
      const result = await api.previewTransform(partial);
      setStepPreviews((previous) => [...previous, { step: index + 1, preview: result }]);
    }
  });
  const saveNow = () => runBusy(async () => {
    const pipeline = buildPipeline();
    const id = selectedId || crypto.randomUUID();
    const list = await api.saveTransformPipeline({ id, name: name.trim() || "새 파이프라인", pipeline });
    setSaved(list);
    setSelectedId(id);
  });
  const executeNow = () => runBusy(async () => {
    if (!selectedId) throw new Error("먼저 파이프라인을 저장하세요.");
    const started = await api.startTransformJob(selectedId, output.trim());
    setJob(started);
    let latest = started;
    while (latest.status === "queued" || latest.status === "running" || latest.status === "cancelling") {
      await new Promise((resolve) => window.setTimeout(resolve, 500));
      latest = await api.getTransformJob(started.id);
      setJob(latest);
    }
    if (latest.status === "failed") throw new Error(latest.error || "데이터 변환 작업에 실패했습니다.");
    if (latest.status === "cancelled") return;
    if (!latest.result) throw new Error("작업 결과를 찾을 수 없습니다.");
    await props.onExecuted(latest.result);
    setJobHistory(await api.listTransformJobs());
  });
  const loadSaved = (id: string) => {
    setSelectedId(id);
    const item = saved.find((p) => p.id === id);
    if (!item) return;
    setName(item.name);
    setSource(item.pipeline.source_table);
    const select = item.pipeline.steps.find((step) => step.type === "select_columns");
    if (select?.type === "select_columns") setColumnsText(select.columns.join(", "));
    const filter = item.pipeline.steps.find((step) => step.type === "filter");
    if (filter?.type === "filter" && filter.predicate.op === "compare" && filter.predicate.value.type === "text") {
      setFilterColumn(filter.predicate.column);
      setFilterValue(filter.predicate.value.value);
    } else {
      setFilterValue("");
    }
    const join = item.pipeline.steps.find((step) => step.type === "join");
    if (join?.type === "join") {
      setJoinTable(join.table); setJoinLeft(join.left_column); setJoinRight(join.right_column);
    } else {
      setJoinTable(""); setJoinLeft(""); setJoinRight("");
    }
    const group = item.pipeline.steps.find((step) => step.type === "group_by");
    if (group?.type === "group_by") {
      setGroupColumnsText(group.columns.join(", "));
      const aggregate = group.aggregates[0];
      if (aggregate) {
        setAggregateFunction(aggregate.function);
        setAggregateColumn(aggregate.column ?? "");
        setAggregateName(aggregate.as_name);
      }
    } else {
      setGroupColumnsText(""); setAggregateColumn("");
    }
    setPreview(null);
  };
  return (
    <section style={{ borderTop: "1px solid var(--border)", marginTop: 16, paddingTop: 12 }}>
      <h4>로컬 파일 연결</h4>
      <p className="muted">CSV, TSV, Parquet 파일을 로컬에서 가져오고 필요할 때 다시 불러옵니다. 파일 경로는 .cellx 프로젝트에 이 PC의 로컬 경로로 저장됩니다. CellX가 자동 전송하지 않지만 프로젝트 파일을 공유하면 경로 문자열도 함께 전달될 수 있습니다.</p>
      <button disabled={busy} onClick={() => void addConnector()}>로컬 파일 연결 추가</button>
      {connectors.map((connector) => (
        <div key={connector.id} style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6 }}>
          <span style={{ flex: 1 }}>{connector.name} <span className="muted">→ {connector.table_name}</span>
            <span className="muted"> · {(() => {
              const status = connectorStatuses.find((item) => item.id === connector.id);
              return !status ? "상태 확인 중" : !status.available ? "원본 파일 없음" : status.changed ? "파일 변경 감지" : "최신 상태";
            })()}</span>
          </span>
          <button disabled={busy} onClick={() => void runBusy(async () => {
            let table: TableInfo;
            try {
              table = await api.refreshDataConnector(connector.id, false);
            } catch (cause) {
              const message = errorText(cause);
              if (!message.includes("SCHEMA_DRIFT")) throw cause;
              const detail = message.replace(/^SCHEMA_DRIFT:\\s*/, "");
              if (!window.confirm(detail)) return;
              table = await api.refreshDataConnector(connector.id, true);
            }
            setConnectors(await api.dataConnectors());
            setConnectorStatuses(await api.dataConnectorStatuses());
            await props.onExecuted(table);
          })}>새로 고침</button>
          <button disabled={busy} onClick={() => void runBusy(async () => {
            setConnectors(await api.deleteDataConnector(connector.id));
          })}>연결 해제</button>
        </div>
      ))}
      <h4>데이터 변환 파이프라인</h4>
      <label>저장된 파이프라인
        <select value={selectedId} onChange={(e) => loadSaved(e.target.value)}>
          <option value="">새 파이프라인</option>
          {saved.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
      </label>
      <label>파이프라인 이름
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>원본 데이터 시트
        <select value={source} onChange={(e) => { setSource(e.target.value); setSelectedId(""); setPreview(null); }}>
          {props.tables.map((t) => <option key={t.name} value={t.name}>{t.name}</option>)}
        </select>
      </label>
      <label>출력 열 (쉼표로 구분)
        <input value={columnsText} onChange={(e) => setColumnsText(e.target.value)} placeholder="열 이름, 열 이름" />
      </label>
      <label>같음 조건 열
        <select value={filterColumn} onChange={(e) => setFilterColumn(e.target.value)}>
          {(current?.columns ?? []).map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
        </select>
      </label>
      <label>필터 값
        <input value={filterValue} onChange={(e) => setFilterValue(e.target.value)} placeholder="비우면 필터 미적용" />
      </label>
      <details>
        <summary>테이블 조인</summary>
        <label>조인할 데이터 시트
          <select value={joinTable} onChange={(e) => {
            const table = e.target.value;
            setJoinTable(table);
            setJoinLeft(current?.columns[0]?.name ?? "");
            setJoinRight(props.tables.find((t) => t.name === table)?.columns[0]?.name ?? "");
          }}>
            <option value="">조인 안 함</option>
            {props.tables.filter((t) => t.name !== source).map((t) => <option key={t.name} value={t.name}>{t.name}</option>)}
          </select>
        </label>
        {joinTable && <>
          <label>원본 조인 열
            <select value={joinLeft} onChange={(e) => setJoinLeft(e.target.value)}>
              {(current?.columns ?? []).map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
            </select>
          </label>
          <label>대상 조인 열
            <select value={joinRight} onChange={(e) => setJoinRight(e.target.value)}>
              {(joinedTable?.columns ?? []).map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
            </select>
          </label>
        </>}
      </details>
      <details>
        <summary>그룹 및 집계</summary>
        <label>그룹 기준 열 (쉼표 구분)
          <input value={groupColumnsText} onChange={(e) => setGroupColumnsText(e.target.value)} placeholder="지역, 상품군" />
        </label>
        <label>집계 함수
          <select value={aggregateFunction} onChange={(e) => setAggregateFunction(e.target.value as AggregateFunction)}>
            <option value="sum">SUM</option><option value="count">COUNT</option><option value="average">AVERAGE</option>
            <option value="min">MIN</option><option value="max">MAX</option>
          </select>
        </label>
        <label>집계 대상 열
          <select value={aggregateColumn} onChange={(e) => setAggregateColumn(e.target.value)}>
            <option value="">COUNT(*) 또는 미사용</option>
            {[...(current?.columns ?? []), ...(joinedTable?.columns ?? [])].map((c, i) => <option key={i} value={c.name}>{c.name}</option>)}
          </select>
        </label>
        <label>집계 결과 열 이름
          <input value={aggregateName} onChange={(e) => setAggregateName(e.target.value)} />
        </label>
      </details>
      <label>결과 데이터 시트 이름
        <input value={output} onChange={(e) => setOutput(e.target.value)} />
      </label>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
        <button disabled={busy || !source} onClick={() => void profileNow()}>데이터 프로파일</button>
        <button disabled={busy || !source} onClick={() => void previewNow()}>미리보기</button>
        <button disabled={busy || !source} onClick={() => void previewStepsNow()}>단계별 미리보기</button>
        <button disabled={busy || !source} onClick={() => void saveNow()}>저장</button>
        <button disabled={busy || !selectedId} onClick={() => void executeNow()}>새 데이터 시트로 실행</button>
        {job && (job.status === "queued" || job.status === "running" || job.status === "cancelling") && <button onClick={() => {
          void api.cancelTransformJob(job.id).then(setJob).catch((e) => setError(errorText(e)));
        }}>실행 취소</button>}
        {selectedId && <button disabled={busy} onClick={() => void runBusy(async () => {
          const list = await api.deleteTransformPipeline(selectedId);
          setSaved(list); setSelectedId(""); setPreview(null);
        })}>삭제</button>}
      </div>
      {error && <p role="alert" style={{ color: "var(--danger, #b42318)" }}>{error}</p>}
      {job && <p className="muted" role="status">백그라운드 작업: {job.phase}{job.status === "failed" && job.error ? ` · ${job.error}` : ""}</p>}
      {jobHistory.length > 0 && <details>
        <summary>최근 변환 실행 이력 ({jobHistory.length})</summary>
        <div style={{ overflow: "auto", maxHeight: 180 }}>
          <table>
            <thead><tr><th>작업 ID</th><th>상태</th><th>결과 행</th><th>오류</th></tr></thead>
            <tbody>{jobHistory.map((item) => <tr key={item.id}>
              <td>{item.id}</td><td>{item.status}</td>
              <td>{item.result?.rows.toLocaleString() ?? "—"}</td>
              <td>{item.error ?? "—"}</td>
            </tr>)}</tbody>
          </table>
        </div>
      </details>}
      {profile && profile.table === source && <div>
        <p className="muted">데이터 프로파일 · {profile.row_count.toLocaleString()}행 · {profile.columns.length}열</p>
        <div style={{ overflow: "auto", maxHeight: 220 }}>
          <table>
            <thead><tr><th>열</th><th>형식</th><th>NULL</th><th>빈 문자열</th><th>고유값</th><th>최솟값</th><th>최댓값</th></tr></thead>
            <tbody>{profile.columns.map((column) => {
              const display = (value: typeof column.min) => value == null || value.t === "Empty" ? "—" : value.t === "Number" || value.t === "Bool" ? String(value.v) : value.v;
              return <tr key={column.name}>
                <td>{column.name}</td><td>{column.sql_type}</td>
                <td>{column.null_count.toLocaleString()}</td><td>{column.empty_count.toLocaleString()}</td>
                <td>{column.distinct_count.toLocaleString()}</td><td>{display(column.min)}</td><td>{display(column.max)}</td>
              </tr>;
            })}</tbody>
          </table>
        </div>
      </div>}
      {stepPreviews.length > 0 && <div>
        <p className="muted">단계별 결과</p>
        {stepPreviews.map(({ step, preview: result }) => (
          <details key={step}>
            <summary>단계 {step} · {result.rows.length}행{result.truncated ? " 이상" : ""} · {result.columns.length}열</summary>
            <div style={{ overflow: "auto", maxHeight: 160 }}>
              <table><thead><tr>{result.columns.map((column) => <th key={column}>{column}</th>)}</tr></thead>
                <tbody>{result.rows.slice(0, 10).map((row, rowIndex) => <tr key={rowIndex}>{row.map((value, colIndex) => <td key={colIndex}>{value.t === "Empty" ? "" : value.t === "Number" ? value.v : value.t === "Bool" ? String(value.v) : value.v}</td>)}</tr>)}</tbody>
              </table>
            </div>
          </details>
        ))}
      </div>}
      {preview && <>
        <p className="muted">미리보기: {preview.rows.length}행{preview.truncated ? " 이상" : ""} · {preview.columns.length}열</p>
        <div style={{ overflow: "auto", maxHeight: 220 }}>
          <table><thead><tr>{preview.columns.map((c) => <th key={c}>{c}</th>)}</tr></thead>
            <tbody>{preview.rows.map((row, i) => <tr key={i}>{row.map((v, j) => <td key={j}>{v.t === "Empty" ? "" : v.t === "Number" ? v.v : v.t === "Bool" ? String(v.v) : v.v}</td>)}</tr>)}</tbody>
          </table>
        </div>
      </>}
    </section>
  );
}
