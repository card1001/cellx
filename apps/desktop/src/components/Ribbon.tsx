// The ribbon, laid out like Korean Excel's: 파일 · 홈 · 삽입 · 수식 · 데이터 · 보기.
import { tr } from "../i18n";
import { SHAPE_KINDS } from "./Shape";
import { SPARK_KINDS } from "./Sparkline";
import { useState, type ReactNode } from "react";
import type { ChartKind, CurrentStyle, FitMode, PageSetup, TableView } from "../api";
import { TABLE_STYLES, TableStyleSwatch } from "./DataDialogs";
import { COLOR_SCALES, DATA_BARS, HIGHLIGHT, type HighlightKind, ICON_SETS } from "../cf";
import { CHART_KINDS } from "./Chart";
import { FORMAT_CHOICES, formatLabel } from "../formats";
import { ColorPicker } from "./ColorPicker";
import { FONTS, SIZES } from "./Dialogs";
import { Dropdown, type MenuItem } from "./Menu";

export interface RibbonActions {
  newBook(): void;
  open(): void;
  save(): void;
  saveAs(): void;
  undo(): void;
  redo(): void;
  cut(): void;
  copy(): void;
  paste(): void;
  toggle(path: "font.b" | "font.i" | "font.u" | "font.strike"): void;
  style(path: string, value: string): void;
  growFont(delta: number): void;
  border(kind: string): void;
  align(h: string): void;
  valign(v: string): void;
  wrap(): void;
  mergeCenter(): void;
  incDec(delta: number): void;
  formatCells(): void;
  insertRows(): void;
  insertColumns(): void;
  deleteRows(): void;
  deleteColumns(): void;
  insertSheet(): void;
  deleteSheet(): void;
  rowHeight(): void;
  columnWidth(): void;
  autofitColumns(): void;
  hide(rows: boolean, hidden: boolean): void;
  autosum(fn: string): void;
  fill(dir: "down" | "right"): void;
  clear(what: "all" | "formats" | "contents"): void;
  find(): void;
  insertFunction(name: string): void;
  importData(): void;
  toggleSql(): void;
  exportTable(): void;
  freeze(kind: "panes" | "row" | "col" | "none"): void;
  toggleGrid(): void;
  zoom(z: number): void;
  zoomToSelection(): void;
  pasteAs(kind: "Values" | "Formulas" | "Formats" | "Transpose"): void;
  pasteSpecial(): void;
  highlight(kind: HighlightKind): void;
  dataBar(color: string, gradient: boolean): void;
  colorScale(colors: string[]): void;
  iconSet(index: number): void;
  rating(): void;
  clearCf(scope: "selection" | "sheet"): void;
  manageCf(): void;
  insertChart(kind: ChartKind): void;
  insertPicture(): void;
  insertShape(kind: import("../api").ShapeKind): void;
  insertSparklines(kind: import("../api").SparkKind): void;
  insertSlicer(): void;
  insertPivot(): void;
  refreshAll(): void;
  newMemo(): void;
  deleteMemo(): void;
  toggleMemos(): void;
  sortAsc(): void;
  sortDesc(): void;
  sortDialog(): void;
  toggleFilter(): void;
  clearFilter(): void;
  reapplyFilter(): void;
  hasFilter(): boolean;
  textToColumns(): void;
  flashFill(): void;
  removeDuplicates(): void;
  validation(): void;
  goalSeek(): void;
  scenarios(): void;
  dataTable(): void;
  replace(): void;
  goToDialog(): void;
  hyperlink(): void;
  nameManager(): void;
  tracePrecedents(): void;
  traceDependents(): void;
  removeArrows(): void;
  toggleShowFormulas(): void;
  evaluateFormula(): void;
  defineName(): void;
  print(): void;
  group(rows: boolean): void;
  ungroup(rows: boolean): void;
  clearOutline(): void;
  showDetail(show: boolean): void;
  subtotal(): void;
  formatAsTable(style: string): void;
  tableName(name: string): void;
  tableToggle(key: "stripes" | "totals", value: boolean): void;
  tableToRange(): void;
  protectSheet(): void;
  protectBook(): void;
  date1904(): void;
  customViews(): void;
  orientation(landscape: boolean): void;
  fitWidth(fit: FitMode): void;
  printGrid(v: boolean): void;
  printHeadings(v: boolean): void;
  printTitles(): void;
  printArea(set: boolean): void;
  headerFooter(): void;
  pageBreak(kind: "insert" | "remove" | "reset"): void;
  togglePageBreaks(): void;
}

interface Props {
  actions: RibbonActions;
  style: CurrentStyle | null;
  canUndo: boolean;
  canRedo: boolean;
  showGrid: boolean;
  sqlOpen: boolean;
  onDataSheet: boolean;
  functions: string[];
  showMemos: boolean;
  hasMemo: boolean;
  filterOn: boolean;
  table: TableView | null;
  protectedSheet: boolean;
  pageSetup: PageSetup | null;
  showPageBreaks: boolean;
  showFormulas: boolean;
  protectedBook: boolean;
  hasArrows: boolean;
  date1904: boolean;
}

const TABS = ["파일", "홈", "삽입", "페이지 레이아웃", "수식", "데이터", "검토", "보기"] as const;
type Tab = (typeof TABS)[number] | "표 디자인";

function Group(props: { label: string; children: ReactNode }) {
  return (
    <div className="rgroup">
      <div className="rgroup-body">{props.children}</div>
      <div className="rgroup-label">{props.label}</div>
    </div>
  );
}

function Btn(props: {
  children: ReactNode;
  title: string;
  onClick(): void;
  on?: boolean;
  big?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      className={`rb ${props.big ? "big" : ""} ${props.on ? "on" : ""}`}
      title={props.title}
      disabled={props.disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

const FN_CATEGORIES: [string, string[]][] = [
  ["재무", ["PMT", "PV", "FV", "NPV", "IRR", "RATE", "NPER", "IPMT", "PPMT"]],
  ["논리", ["IF", "IFS", "IFERROR", "AND", "OR", "NOT", "XOR", "SWITCH"]],
  ["텍스트", ["CONCAT", "TEXTJOIN", "LEFT", "RIGHT", "MID", "LEN", "FIND", "SUBSTITUTE", "TRIM", "UPPER", "LOWER", "TEXT", "VALUE"]],
  ["날짜 및 시간", ["TODAY", "NOW", "DATE", "YEAR", "MONTH", "DAY", "EOMONTH", "EDATE", "WEEKDAY", "NETWORKDAYS", "DATEDIF"]],
  ["찾기/참조", ["XLOOKUP", "VLOOKUP", "HLOOKUP", "INDEX", "MATCH", "XMATCH", "OFFSET", "INDIRECT", "FILTER", "SORT", "UNIQUE"]],
  ["수학/삼각", ["SUM", "SUMIF", "SUMIFS", "SUMPRODUCT", "ROUND", "ROUNDUP", "ROUNDDOWN", "INT", "MOD", "ABS", "POWER", "SQRT"]],
  ["통계", ["AVERAGE", "AVERAGEIF", "AVERAGEIFS", "COUNT", "COUNTA", "COUNTIF", "COUNTIFS", "MAX", "MIN", "MEDIAN", "STDEV.S", "RANK.EQ"]],
  ["데이터 시트", ["SUMIFS", "COUNTIFS", "AVERAGEIFS", "MINIFS", "MAXIFS", "XLOOKUP", "DQUERY"]],
];

export function Ribbon(props: Props) {
  const [tab, setTab] = useState<Tab>("홈");
  // The contextual 표 디자인 tab goes away when the active cell leaves the table.
  if (tab === "표 디자인" && !props.table) setTab("홈");
  const a = props.actions;
  const s = props.style;
  const fnItems = (names: string[]): MenuItem[] =>
    names.filter((n) => props.functions.length === 0 || props.functions.includes(n)).map((n) => ({ label: n, onClick: () => a.insertFunction(n) }));

  const borderItems: MenuItem[] = [
    { label: tr("아래쪽 테두리"), onClick: () => a.border("Bottom") },
    { label: tr("위쪽 테두리"), onClick: () => a.border("Top") },
    { label: tr("왼쪽 테두리"), onClick: () => a.border("Left") },
    { label: tr("오른쪽 테두리"), onClick: () => a.border("Right") },
    { separator: true },
    { label: tr("테두리 없음"), onClick: () => a.border("None") },
    { label: tr("모든 테두리"), onClick: () => a.border("All") },
    { label: tr("바깥쪽 테두리"), onClick: () => a.border("Outer") },
    { label: tr("굵은 바깥쪽 테두리"), onClick: () => a.border("OuterThick") },
    { label: tr("안쪽 테두리"), onClick: () => a.border("Inner") },
  ];

  return (
    <div className="ribbon">
      <div className="rtabs">
        {TABS.map((t) => (
          <button
            key={t}
            className={`rtab ${t === "파일" ? "file" : ""} ${t === tab ? "on" : ""}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setTab(t)}
          >
            {tr(t)}
          </button>
        ))}
        {props.table && (
          <button className={`rtab contextual ${tab === "표 디자인" ? "on" : ""}`} onMouseDown={(e) => e.preventDefault()} onClick={() => setTab("표 디자인")}>
            {tr("표 디자인")}
          </button>
        )}
        <div className="qat">
          <button className="rb" title={tr("저장 (Ctrl+S)")} onClick={a.save} onMouseDown={(e) => e.preventDefault()}>
            💾
          </button>
          <button className="rb" title={tr("실행 취소 (Ctrl+Z)")} disabled={!props.canUndo} onClick={a.undo} onMouseDown={(e) => e.preventDefault()}>
            ↶
          </button>
          <button className="rb" title={tr("다시 실행 (Ctrl+Y)")} disabled={!props.canRedo} onClick={a.redo} onMouseDown={(e) => e.preventDefault()}>
            ↷
          </button>
        </div>
      </div>
      <div className="rbody">
        {tab === "파일" && (
          <>
            <Group label={tr("파일")}>
              <Btn big title={tr("새 통합 문서 (Ctrl+N)")} onClick={a.newBook}>
                <span className="ic">📄</span>{tr("새로 만들기")}
              </Btn>
              <Btn big title={tr("열기 (Ctrl+O)")} onClick={a.open}>
                <span className="ic">📂</span>{tr("열기")}
              </Btn>
              <Btn big title={tr("저장 (Ctrl+S)")} onClick={a.save}>
                <span className="ic">💾</span>{tr("저장")}
              </Btn>
              <Btn big title={tr("다른 이름으로 저장 (F12)")} onClick={a.saveAs}>
                <span className="ic">🖫</span>{tr("다른 이름으로 저장")}
              </Btn>
              <Btn big title={tr("인쇄 / PDF로 저장 (Ctrl+P)")} onClick={a.print}>
                <span className="ic">🖨</span>{tr("인쇄")}
              </Btn>
            </Group>
            <Group label={tr("옵션")}>
              <Btn
                big
                title={tr("1904 날짜 체계: 일련번호 0을 1904-01-01로 셉니다(Mac용 옛 Excel 파일). 바꾸면 날짜 셀이 1462일(4년 1일)씩 다르게 표시됩니다")}
                on={props.date1904}
                onClick={a.date1904}
              >
                <span className="ic">📅</span>{tr("1904 날짜 체계")}
              </Btn>
            </Group>
            <Group label={tr("정보")}>
              <div className="about">
                <b>CellX</b>
                <span>{tr("버전")} {__APP_VERSION__}</span>
                <span>{tr("엑셀 호환 고성능 스프레드시트")}</span>
              </div>
            </Group>
          </>
        )}
        {tab === "홈" && (
          <>
            <Group label={tr("클립보드")}>
              <div className="split">
                <Btn big title={tr("붙여넣기 (Ctrl+V)")} onClick={a.paste}>
                  <span className="ic">📋</span>{tr("붙여넣기")}
                </Btn>
                <Dropdown
                  label=""
                  title={tr("붙여넣기 옵션")}
                  items={[
                    { label: tr("붙여넣기"), shortcut: "Ctrl+V", onClick: a.paste },
                    { label: tr("수식"), onClick: () => a.pasteAs("Formulas") },
                    { label: tr("값"), onClick: () => a.pasteAs("Values") },
                    { label: tr("서식"), onClick: () => a.pasteAs("Formats") },
                    { label: tr("행/열 바꿈"), onClick: () => a.pasteAs("Transpose") },
                    { separator: true },
                    { label: tr("선택하여 붙여넣기..."), shortcut: "Ctrl+Alt+V", onClick: a.pasteSpecial },
                  ]}
                />
              </div>
              <div className="col">
                <Btn title={tr("잘라내기 (Ctrl+X)")} onClick={a.cut}>
                  {tr("✂ 잘라내기")}
                </Btn>
                <Btn title={tr("복사 (Ctrl+C)")} onClick={a.copy}>
                  {tr("⧉ 복사")}
                </Btn>
              </div>
            </Group>
            <Group label={tr("글꼴")}>
              <div className="col">
                <div className="row">
                  <select
                    className="font-name"
                    value={s?.font ?? "맑은 고딕"}
                    onChange={(e) => a.style("font.name", e.target.value)}
                    title={tr("글꼴")}
                  >
                    {[...new Set([s?.font ?? "맑은 고딕", ...FONTS])].map((f) => (
                      <option key={f} style={{ fontFamily: f }}>
                        {f}
                      </option>
                    ))}
                  </select>
                  <select
                    className="font-size"
                    value={String(s?.size ?? 11)}
                    onChange={(e) => a.style("font.size", e.target.value)}
                    title={tr("글꼴 크기")}
                  >
                    {[...new Set([s?.size ?? 11, ...SIZES])].sort((x, y) => x - y).map((n) => (
                      <option key={n}>{n}</option>
                    ))}
                  </select>
                  <Btn title={tr("글꼴 크기 크게 (Ctrl+Shift+>)")} onClick={() => a.growFont(1)}>
                    {tr("가<sup>▲</sup>")}
                  </Btn>
                  <Btn title={tr("글꼴 크기 작게 (Ctrl+Shift+<)")} onClick={() => a.growFont(-1)}>
                    {tr("가<sup>▼</sup>")}
                  </Btn>
                </div>
                <div className="row">
                  <Btn title={tr("굵게 (Ctrl+B)")} on={s?.bold} onClick={() => a.toggle("font.b")}>
                    <b>{tr("가")}</b>
                  </Btn>
                  <Btn title={tr("기울임꼴 (Ctrl+I)")} on={s?.italic} onClick={() => a.toggle("font.i")}>
                    <i>{tr("가")}</i>
                  </Btn>
                  <Btn title={tr("밑줄 (Ctrl+U)")} on={s?.underline} onClick={() => a.toggle("font.u")}>
                    <u>{tr("가")}</u>
                  </Btn>
                  <Btn title={tr("취소선 (Ctrl+5)")} on={s?.strike} onClick={() => a.toggle("font.strike")}>
                    <s>{tr("가")}</s>
                  </Btn>
                  <Dropdown label="⊞" title={tr("테두리")} items={borderItems} />
                  <Dropdown
                    label={<span className="fill-ic">◧</span>}
                    title={tr("채우기 색")}
                    panel={(close) => (
                      <ColorPicker
                        noneLabel={tr("채우기 없음")}
                        onPick={(c) => {
                          close();
                          a.style("fill.fg_color", c ?? "");
                        }}
                      />
                    )}
                  />
                  <Dropdown
                    label={<span className="color-ic">{tr("가")}</span>}
                    title={tr("글꼴 색")}
                    panel={(close) => (
                      <ColorPicker
                        noneLabel={tr("자동")}
                        onPick={(c) => {
                          close();
                          a.style("font.color", c ?? "#000000");
                        }}
                      />
                    )}
                  />
                </div>
              </div>
            </Group>
            <Group label={tr("맞춤")}>
              <div className="col">
                <div className="row">
                  <Btn title={tr("위쪽 맞춤")} on={s?.valign === "top"} onClick={() => a.valign("top")}>
                    ⤒
                  </Btn>
                  <Btn title={tr("가운데 맞춤")} on={s?.valign === "center"} onClick={() => a.valign("center")}>
                    ⇕
                  </Btn>
                  <Btn title={tr("아래쪽 맞춤")} on={s?.valign === "bottom"} onClick={() => a.valign("bottom")}>
                    ⤓
                  </Btn>
                  <Btn title={tr("자동 줄 바꿈")} on={s?.wrap} onClick={a.wrap}>
                    {tr("↵ 자동 줄 바꿈")}
                  </Btn>
                </div>
                <div className="row">
                  <Btn title={tr("왼쪽 맞춤")} on={s?.align === "left"} onClick={() => a.align("left")}>
                    ☰⇤
                  </Btn>
                  <Btn title={tr("가운데 맞춤")} on={s?.align === "center"} onClick={() => a.align("center")}>
                    ☰
                  </Btn>
                  <Btn title={tr("오른쪽 맞춤")} on={s?.align === "right"} onClick={() => a.align("right")}>
                    ⇥☰
                  </Btn>
                  <Btn title={tr("병합하고 가운데 맞춤")} on={s?.merged} onClick={a.mergeCenter}>
                    {tr("⇔ 병합하고 가운데 맞춤")}
                  </Btn>
                </div>
              </div>
            </Group>
            <Group label={tr("표시 형식")}>
              <div className="col">
                <select
                  className="num-fmt"
                  value={formatLabel(s?.num_fmt ?? "general")}
                  onChange={(e) => {
                    const f = FORMAT_CHOICES.find((x) => x.label === e.target.value);
                    if (f) a.style("num_fmt", f.code);
                    else a.formatCells();
                  }}
                  title={tr("표시 형식")}
                >
                  {FORMAT_CHOICES.map((f) => (
                    <option key={f.code}>{tr(f.label)}</option>
                  ))}
                  <option>{tr("사용자 지정")}</option>
                </select>
                <div className="row">
                  <Btn title={tr("통화 (₩)")} onClick={() => a.style("num_fmt", "₩#,##0")}>
                    ₩
                  </Btn>
                  <Btn title={tr("백분율 스타일 (Ctrl+Shift+%)")} onClick={() => a.style("num_fmt", "0%")}>
                    %
                  </Btn>
                  <Btn title={tr("쉼표 스타일")} onClick={() => a.style("num_fmt", "#,##0")}>
                    ,
                  </Btn>
                  <Btn title={tr("자릿수 늘림")} onClick={() => a.incDec(1)}>
                    .0→.00
                  </Btn>
                  <Btn title={tr("자릿수 줄임")} onClick={() => a.incDec(-1)}>
                    .00→.0
                  </Btn>
                </div>
              </div>
            </Group>
            <Group label={tr("스타일")}>
              <Dropdown
                label={<span className="dd-big"><span className="ic">▦</span>{tr("조건부 서식")}</span>}
                title={tr("조건부 서식")}
                className="big-dd"
                items={[
                  { label: tr("셀 강조 규칙"), disabled: true },
                  ...(["GreaterThan", "LessThan", "Between", "Equal", "TextContains", "Duplicate"] as HighlightKind[]).map((k) => ({
                    label: `   ${HIGHLIGHT[k].title}...`,
                    onClick: () => a.highlight(k),
                  })),
                  { separator: true },
                  { label: tr("상위/하위 규칙"), disabled: true },
                  ...(["Top10", "Bottom10", "AboveAverage", "BelowAverage"] as HighlightKind[]).map((k) => ({
                    label: `   ${HIGHLIGHT[k].title}...`,
                    onClick: () => a.highlight(k),
                  })),
                  { separator: true },
                  { label: tr("데이터 막대"), disabled: true },
                  ...DATA_BARS.map((d) => ({
                    label: `   ${d.label}`,
                    icon: <span className="swatch" style={{ background: d.color }} />,
                    onClick: () => a.dataBar(d.color, d.gradient),
                  })),
                  { separator: true },
                  { label: tr("색조"), disabled: true },
                  ...COLOR_SCALES.map((c) => ({
                    label: `   ${c.label}`,
                    icon: <span className="swatch" style={{ background: `linear-gradient(90deg, ${[...c.colors].reverse().join(",")})` }} />,
                    onClick: () => a.colorScale(c.colors),
                  })),
                  { separator: true },
                  { label: tr("아이콘 집합"), disabled: true },
                  ...ICON_SETS.map((s, i) => ({ label: `   ${tr(s.label)}`, onClick: () => a.iconSet(i) })),
                  { label: tr("   별점 5개"), onClick: a.rating },
                  { separator: true },
                  { label: tr("선택한 셀의 규칙 지우기"), onClick: () => a.clearCf("selection") },
                  { label: tr("시트 전체에서 규칙 지우기"), onClick: () => a.clearCf("sheet") },
                  { label: tr("규칙 관리..."), onClick: a.manageCf },
                ]}
              />
              <Dropdown
                label={<span className="dd-big"><span className="ic">▤</span>{tr("표 서식")}</span>}
                title={tr("표 서식 (Ctrl+T)")}
                className="big-dd"
                panel={(close) => (
                  <div className="table-styles">
                    {TABLE_STYLES.map((t) => (
                      <button
                        key={t.name}
                        title={tr(t.label)}
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => {
                          close();
                          a.formatAsTable(t.name);
                        }}
                      >
                        <TableStyleSwatch name={t.name} />
                        <span>{tr(t.label)}</span>
                      </button>
                    ))}
                  </div>
                )}
              />
            </Group>
            <Group label={tr("셀")}>
              <Dropdown
                label={tr("삽입")}
                items={[
                  { label: tr("시트 행 삽입"), onClick: a.insertRows },
                  { label: tr("시트 열 삽입"), onClick: a.insertColumns },
                  { label: tr("시트 삽입"), shortcut: "Shift+F11", onClick: a.insertSheet },
                ]}
              />
              <Dropdown
                label={tr("삭제")}
                items={[
                  { label: tr("시트 행 삭제"), onClick: a.deleteRows },
                  { label: tr("시트 열 삭제"), onClick: a.deleteColumns },
                  { label: tr("시트 삭제"), onClick: a.deleteSheet },
                ]}
              />
              <Dropdown
                label={tr("서식")}
                items={[
                  { label: tr("행 높이..."), onClick: a.rowHeight },
                  { label: tr("열 너비..."), onClick: a.columnWidth },
                  { label: tr("열 너비 자동 맞춤"), onClick: a.autofitColumns },
                  { separator: true },
                  { label: tr("행 숨기기"), shortcut: "Ctrl+9", onClick: () => a.hide(true, true) },
                  { label: tr("열 숨기기"), shortcut: "Ctrl+0", onClick: () => a.hide(false, true) },
                  { label: tr("행 숨기기 취소"), onClick: () => a.hide(true, false) },
                  { label: tr("열 숨기기 취소"), onClick: () => a.hide(false, false) },
                  { separator: true },
                  { label: tr("셀 서식..."), shortcut: "Ctrl+1", onClick: a.formatCells },
                ]}
              />
            </Group>
            <Group label={tr("편집")}>
              <div className="col">
                <div className="row">
                  <Btn title={tr("자동 합계 (Alt+=)")} onClick={() => a.autosum("SUM")}>
                    {tr("Σ 자동 합계")}
                  </Btn>
                  <Dropdown
                    label=""
                    title={tr("자동 합계 함수")}
                    items={[
                      { label: tr("합계"), onClick: () => a.autosum("SUM") },
                      { label: tr("평균"), onClick: () => a.autosum("AVERAGE") },
                      { label: tr("숫자 개수"), onClick: () => a.autosum("COUNT") },
                      { label: tr("최대값"), onClick: () => a.autosum("MAX") },
                      { label: tr("최소값"), onClick: () => a.autosum("MIN") },
                    ]}
                  />
                </div>
                <div className="row">
                  <Dropdown
                    label={tr("⬇ 채우기")}
                    items={[
                      { label: tr("아래로"), shortcut: "Ctrl+D", onClick: () => a.fill("down") },
                      { label: tr("오른쪽"), shortcut: "Ctrl+R", onClick: () => a.fill("right") },
                    ]}
                  />
                  <Dropdown
                    label={tr("⌫ 지우기")}
                    items={[
                      { label: tr("모두 지우기"), onClick: () => a.clear("all") },
                      { label: tr("서식 지우기"), onClick: () => a.clear("formats") },
                      { label: tr("내용 지우기"), shortcut: "Delete", onClick: () => a.clear("contents") },
                    ]}
                  />
                  <Dropdown
                    label={tr("⇅ 정렬 및 필터")}
                    items={[
                      { label: tr("텍스트 오름차순 정렬"), icon: "↑", onClick: a.sortAsc },
                      { label: tr("텍스트 내림차순 정렬"), icon: "↓", onClick: a.sortDesc },
                      { label: tr("사용자 지정 정렬..."), onClick: a.sortDialog },
                      { separator: true },
                      { label: tr("필터"), shortcut: "Ctrl+Shift+L", checked: props.filterOn, onClick: a.toggleFilter },
                      { label: tr("지우기"), disabled: !props.filterOn, onClick: a.clearFilter },
                      { label: tr("다시 적용"), disabled: !props.filterOn, onClick: a.reapplyFilter },
                    ]}
                  />
                  <Dropdown
                    label={tr("🔍 찾기 및 선택")}
                    items={[
                      { label: tr("찾기..."), shortcut: "Ctrl+F", onClick: a.find },
                      { label: tr("바꾸기..."), shortcut: "Ctrl+H", onClick: a.replace },
                      { label: tr("이동..."), shortcut: "Ctrl+G", onClick: a.goToDialog },
                    ]}
                  />
                </div>
              </div>
            </Group>
          </>
        )}
        {tab === "삽입" && (
          <>
            <Group label={tr("셀")}>
              <Btn big title={tr("시트 행 삽입")} onClick={a.insertRows}>
                <span className="ic">⊞</span>{tr("행 삽입")}
              </Btn>
              <Btn big title={tr("시트 열 삽입")} onClick={a.insertColumns}>
                <span className="ic">⊞</span>{tr("열 삽입")}
              </Btn>
              <Btn big title={tr("시트 삽입 (Shift+F11)")} onClick={a.insertSheet}>
                <span className="ic">▭</span>{tr("시트 삽입")}
              </Btn>
            </Group>
            <Group label={tr("표")}>
              <Btn big title={tr("피벗 테이블: 선택한 범위를 요약합니다")} onClick={a.insertPivot}>
                <span className="ic">⊞</span>{tr("피벗 테이블")}
              </Btn>
              <Btn big title={tr("표 (Ctrl+T)")} onClick={() => a.formatAsTable("TableStyleMedium2")}>
                <span className="ic">▤</span>{tr("표")}
              </Btn>
            </Group>
            <Group label={tr("일러스트레이션")}>
              <Btn big title={tr("그림 삽입 (PNG, JPEG, GIF, BMP). 스크린샷은 Ctrl+V로 붙여넣을 수 있습니다.")} onClick={a.insertPicture}>
                <span className="ic">🖼</span>{tr("그림")}
              </Btn>
              <Dropdown
                label={
                  <span className="dd-big">
                    <span className="ic">⬠</span>{tr("도형")}
                  </span>
                }
                className="big-dd"
                title={tr("도형 삽입")}
                panel={(close) => (
                  <div className="shape-menu">
                    {SHAPE_KINDS.map((k) => (
                      <button
                        key={k.kind}
                        title={tr(k.label)}
                        onClick={() => {
                          close();
                          a.insertShape(k.kind);
                        }}
                      >
                        {k.icon}
                      </button>
                    ))}
                  </div>
                )}
              />
              <Btn big title={tr("텍스트 상자 삽입")} onClick={() => a.insertShape("textBox")}>
                <span className="ic">🗛</span>{tr("텍스트 상자")}
              </Btn>
            </Group>
            <Group label={tr("차트")}>
              {CHART_KINDS.map((k) => (
                <Btn big key={k.kind} title={`${tr(k.label)} ${tr("차트 삽입")}`} onClick={() => a.insertChart(k.kind)}>
                  <span className="ic">{k.icon}</span>
                  {tr(k.label)}
                </Btn>
              ))}
            </Group>
            <Group label={tr("스파크라인")}>
              {SPARK_KINDS.map((k) => (
                <Btn big key={k.kind} title={`${tr(k.label)} ${tr("스파크라인 삽입")}`} onClick={() => a.insertSparklines(k.kind)}>
                  <span className="ic">{k.icon}</span>
                  {k.label}
                </Btn>
              ))}
            </Group>
            <Group label={tr("필터")}>
              <Btn big title={tr("표의 열로 필터링하는 슬라이서를 삽입합니다")} disabled={!props.table} onClick={a.insertSlicer}>
                <span className="ic">▦</span>{tr("슬라이서")}
              </Btn>
            </Group>
            <Group label={tr("링크")}>
              <Btn big title={tr("하이퍼링크 (Ctrl+K)")} onClick={a.hyperlink}>
                <span className="ic">🔗</span>{tr("링크")}
              </Btn>
            </Group>
            <Group label={tr("메모")}>
              <Btn big title={tr("새 메모 (Shift+F2)")} onClick={a.newMemo}>
                <span className="ic">🗨</span>{tr("메모")}
              </Btn>
            </Group>
            <Group label={tr("데이터")}>
              <Btn big title={tr("CSV·Parquet을 데이터 시트로")} onClick={a.importData}>
                <span className="ic">🗄</span>{tr("데이터 시트")}
              </Btn>
            </Group>
          </>
        )}
        {tab === "페이지 레이아웃" && (
          <>
            <Group label={tr("페이지 설정")}>
              <Dropdown
                label={<span className="dd-big"><span className="ic">⎙</span>{tr("용지 방향")}</span>}
                className="big-dd"
                items={[
                  { label: tr("세로"), checked: !props.pageSetup?.landscape, onClick: () => a.orientation(false) },
                  { label: tr("가로"), checked: !!props.pageSetup?.landscape, onClick: () => a.orientation(true) },
                ]}
              />
              <Dropdown
                label={<span className="dd-big"><span className="ic">⤓</span>{tr("나누기")}</span>}
                className="big-dd"
                items={[
                  { label: tr("페이지 나누기 삽입"), onClick: () => a.pageBreak("insert") },
                  { label: tr("페이지 나누기 제거"), onClick: () => a.pageBreak("remove") },
                  { label: tr("페이지 나누기 모두 원래대로"), onClick: () => a.pageBreak("reset") },
                ]}
              />
              <Dropdown
                label={<span className="dd-big"><span className="ic">▣</span>{tr("인쇄 영역")}</span>}
                className="big-dd"
                items={[
                  { label: tr("인쇄 영역 설정"), onClick: () => a.printArea(true) },
                  { label: tr("인쇄 영역 해제"), disabled: !props.pageSetup?.print_area, onClick: () => a.printArea(false) },
                ]}
              />
              <Btn big title={tr("모든 페이지에 반복할 행")} onClick={a.printTitles}>
                <span className="ic">☰</span>{tr("인쇄 제목")}
              </Btn>
              <Btn big title={tr("머리글과 바닥글 (페이지 번호, 날짜, 시트 이름…)")} onClick={a.headerFooter}>
                <span className="ic">▤</span>{tr("머리글/바닥글")}
              </Btn>
            </Group>
            <Group label={tr("크기 조정")}>
              <select
                className="num-fmt"
                value={props.pageSetup?.fit ?? "None"}
                onChange={(e) => a.fitWidth(e.target.value as FitMode)}
                title={tr("배율")}
              >
                <option value="None">{tr("현재 크기")}</option>
                <option value="Width">{tr("너비: 1페이지")}</option>
                <option value="Page">{tr("한 페이지에 맞추기")}</option>
              </select>
            </Group>
            <Group label={tr("시트 옵션")}>
              <div className="col">
                <label className="rcheck">
                  <input type="checkbox" checked={!!props.pageSetup?.gridlines} onChange={(e) => a.printGrid(e.target.checked)} /> {tr("눈금선 인쇄")}
                </label>
                <label className="rcheck">
                  <input type="checkbox" checked={!!props.pageSetup?.headings} onChange={(e) => a.printHeadings(e.target.checked)} /> {tr("제목 인쇄")}
                </label>
                <label className="rcheck">
                  <input type="checkbox" checked={props.showPageBreaks} onChange={a.togglePageBreaks} /> {tr("페이지 나누기 보기")}
                </label>
              </div>
            </Group>
            <Group label={tr("인쇄")}>
              <Btn big title={tr("인쇄 미리 보기와 인쇄 (Ctrl+P)")} onClick={a.print}>
                <span className="ic">🖨</span>{tr("인쇄")}
              </Btn>
            </Group>
          </>
        )}
        {tab === "표 디자인" && props.table && (
          <>
            <Group label={tr("속성")}>
              <div className="col">
                <span className="muted">{tr("표 이름:")}</span>
                <input
                  key={props.table.name}
                  className="font-name"
                  defaultValue={props.table.name}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") a.tableName((e.target as HTMLInputElement).value);
                  }}
                  onBlur={(e) => e.target.value !== props.table?.name && a.tableName(e.target.value)}
                />
              </div>
            </Group>
            <Group label={tr("도구")}>
              <Btn big title={tr("표를 일반 범위로 바꿉니다(서식은 남습니다)")} onClick={a.tableToRange}>
                <span className="ic">⇲</span>{tr("범위로 변환")}
              </Btn>
              <Btn big title={tr("부분합")} onClick={a.subtotal}>
                <span className="ic">Σ</span>{tr("부분합")}
              </Btn>
            </Group>
            <Group label={tr("표 스타일 옵션")}>
              <div className="col">
                <label className="rcheck">
                  <input type="checkbox" checked={props.table.totals} onChange={(e) => a.tableToggle("totals", e.target.checked)} /> {tr("요약 행")}
                </label>
                <label className="rcheck">
                  <input type="checkbox" checked={props.table.stripes} onChange={(e) => a.tableToggle("stripes", e.target.checked)} /> {tr("줄무늬 행")}
                </label>
                <label className="rcheck">
                  <input type="checkbox" checked={props.filterOn} onChange={a.toggleFilter} /> {tr("필터 단추")}
                </label>
              </div>
            </Group>
            <Group label={tr("표 스타일")}>
              <div className="table-styles inline">
                {TABLE_STYLES.map((t) => (
                  <button
                    key={t.name}
                    className={props.table?.style === t.name ? "on" : ""}
                    title={t.label}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => a.formatAsTable(t.name)}
                  >
                    <TableStyleSwatch name={t.name} />
                  </button>
                ))}
              </div>
            </Group>
          </>
        )}
        {tab === "수식" && (
          <>
            <Group label={tr("함수 라이브러리")}>
              <Btn big title={tr("자동 합계 (Alt+=)")} onClick={() => a.autosum("SUM")}>
                <span className="ic">Σ</span>{tr("자동 합계")}
              </Btn>
              {FN_CATEGORIES.map(([label, names]) => (
                <Dropdown key={label} label={tr(label)} items={fnItems(names)} />
              ))}
            </Group>
            <Group label={tr("정의된 이름")}>
              <Btn big title={tr("이름 관리자 (Ctrl+F3)")} onClick={a.nameManager}>
                <span className="ic">🏷</span>{tr("이름 관리자")}
              </Btn>
              <Btn big title={tr("선택 영역에 이름을 붙입니다")} onClick={a.defineName}>
                <span className="ic">✎</span>{tr("이름 정의")}
              </Btn>
            </Group>
            <Group label={tr("수식 분석")}>
              <div className="col">
                <Btn title={tr("현재 셀의 값에 영향을 주는 셀을 화살표로 표시합니다")} onClick={a.tracePrecedents}>
                  {tr("⇥ 참조되는 셀 추적")}
                </Btn>
                <Btn title={tr("현재 셀의 값을 사용하는 셀을 화살표로 표시합니다")} onClick={a.traceDependents}>
                  {tr("⇤ 참조하는 셀 추적")}
                </Btn>
              </div>
              <div className="col">
                <Btn title={tr("추적 화살표를 모두 지웁니다")} disabled={!props.hasArrows} onClick={a.removeArrows}>
                  {tr("✕ 연결선 제거")}
                </Btn>
                <Btn title={tr("각 셀에 결과 대신 수식을 표시합니다 (Ctrl+`)")} on={props.showFormulas} onClick={a.toggleShowFormulas}>
                  {tr("ƒx 수식 표시")}
                </Btn>
              </div>
              <Btn big title={tr("수식을 단계별로 계산합니다")} onClick={a.evaluateFormula}>
                <span className="ic">ⓕ</span>{tr("수식 계산")}
              </Btn>
            </Group>
          </>
        )}
        {tab === "데이터" && (
          <>
            <Group label={tr("데이터 가져오기")}>
              <Btn big title={tr("CSV, TSV, Parquet 파일을 데이터 시트로 엽니다")} onClick={a.importData}>
                <span className="ic">🗄</span>{tr("데이터 가져오기")}
              </Btn>
              <Btn big title={tr("SELECT 결과를 새 데이터 시트로")} on={props.sqlOpen} onClick={a.toggleSql}>
                <span className="ic">⌘</span>SQL
              </Btn>
              <Btn big title={tr("데이터 시트를 CSV·Parquet으로")} disabled={!props.onDataSheet} onClick={a.exportTable}>
                <span className="ic">⇪</span>{tr("내보내기")}
              </Btn>
            </Group>
            <Group label={tr("정렬 및 필터")}>
              <div className="col">
                <Btn title={tr("텍스트 오름차순 정렬")} onClick={a.sortAsc}>
                  {tr("↑ 오름차순")}
                </Btn>
                <Btn title={tr("텍스트 내림차순 정렬")} onClick={a.sortDesc}>
                  {tr("↓ 내림차순")}
                </Btn>
              </div>
              <Btn big title={tr("정렬 기준을 여러 개 지정합니다")} onClick={a.sortDialog}>
                <span className="ic">⇅</span>{tr("정렬")}
              </Btn>
              <Btn big title={tr("필터 (Ctrl+Shift+L)")} on={props.filterOn} onClick={a.toggleFilter}>
                <span className="ic">⏷</span>{tr("필터")}
              </Btn>
              <div className="col">
                <Btn title={tr("필터 지우기")} disabled={!props.filterOn} onClick={a.clearFilter}>
                  {tr("⊘ 지우기")}
                </Btn>
                <Btn title={tr("다시 적용")} disabled={!props.filterOn} onClick={a.reapplyFilter}>
                  {tr("⟳ 다시 적용")}
                </Btn>
              </div>
            </Group>
            <Group label={tr("데이터 도구")}>
              <Btn big title={tr("한 열의 텍스트를 여러 열로 나눕니다")} onClick={a.textToColumns}>
                <span className="ic">⫼</span>{tr("텍스트 나누기")}
              </Btn>
              <Btn big title={tr("빠른 채우기 (Ctrl+E)")} onClick={a.flashFill}>
                <span className="ic">⚡</span>{tr("빠른 채우기")}
              </Btn>
              <Btn big title={tr("중복된 행을 삭제합니다")} onClick={a.removeDuplicates}>
                <span className="ic">⧉</span>{tr("중복된 항목 제거")}
              </Btn>
              <Btn big title={tr("셀에 입력할 수 있는 값을 제한합니다")} onClick={a.validation}>
                <span className="ic">☑</span>{tr("데이터 유효성 검사")}
              </Btn>
            </Group>
            <Group label={tr("예측")}>
              <Dropdown label={<span className="dd-big"><span className="ic">❓</span>{tr("가상 분석")}</span>} className="big-dd" items={[
                  { label: tr("시나리오 관리자..."), onClick: a.scenarios },
                  { label: tr("목표값 찾기..."), onClick: a.goalSeek },
                  { label: tr("데이터 표..."), onClick: a.dataTable },
                ]}
              />
            </Group>
            <Group label={tr("개요")}>
              <Dropdown
                label={<span className="dd-big"><span className="ic">⊟</span>{tr("그룹")}</span>}
                className="big-dd"
                items={[
                  { label: tr("행 그룹"), shortcut: "Shift+Alt+→", onClick: () => a.group(true) },
                  { label: tr("열 그룹"), onClick: () => a.group(false) },
                ]}
              />
              <Dropdown
                label={<span className="dd-big"><span className="ic">⊞</span>{tr("그룹 해제")}</span>}
                className="big-dd"
                items={[
                  { label: tr("행 그룹 해제"), shortcut: "Shift+Alt+←", onClick: () => a.ungroup(true) },
                  { label: tr("열 그룹 해제"), onClick: () => a.ungroup(false) },
                  { separator: true },
                  { label: tr("개요 지우기"), onClick: a.clearOutline },
                ]}
              />
              <Btn big title={tr("그룹별 SUBTOTAL 행을 넣습니다")} onClick={a.subtotal}>
                <span className="ic">Σ</span>{tr("부분합")}
              </Btn>
              <div className="col">
                <Btn title={tr("세부 정보 표시")} onClick={() => a.showDetail(true)}>
                  {tr("⊞ 세부 정보 표시")}
                </Btn>
                <Btn title={tr("세부 정보 숨기기")} onClick={() => a.showDetail(false)}>
                  {tr("⊟ 세부 정보 숨기기")}
                </Btn>
              </div>
            </Group>
            <Group label={tr("쿼리 및 연결")}>
              <Btn big title={tr("피벗 테이블을 모두 새로 고침 (Ctrl+Alt+F5)")} onClick={a.refreshAll}>
                <span className="ic">⟳</span>{tr("모두 새로 고침")}
              </Btn>
            </Group>
          </>
        )}
        {tab === "검토" && (
          <>
            <Group label={tr("메모")}>
              <Btn big title={tr(props.hasMemo ? "메모 편집 (Shift+F2)" : "새 메모 (Shift+F2)") } onClick={a.newMemo}>
                <span className="ic">🗨</span>
                {tr(props.hasMemo ? "메모 편집" : "새 메모")}
              </Btn>
              <Btn big title={tr("선택한 셀의 메모 삭제")} disabled={!props.hasMemo} onClick={a.deleteMemo}>
                <span className="ic">🗑</span>{tr("삭제")}
              </Btn>
              <Btn big title={tr("모든 메모 표시")} on={props.showMemos} onClick={a.toggleMemos}>
                <span className="ic">🗪</span>{tr("메모 모두 표시")}
              </Btn>
            </Group>
            <Group label={tr("보호")}>
              <Btn big title={tr(props.protectedSheet ? "시트 보호 해제" : "시트 보호")} on={props.protectedSheet} onClick={a.protectSheet}>
                <span className="ic">🔒</span>
                {tr(props.protectedSheet ? "시트 보호 해제" : "시트 보호")}
              </Btn>
              <Btn big title={tr("통합 문서 보호: 시트 추가·삭제·이름 바꾸기·이동·숨기기를 막습니다")} on={props.protectedBook} onClick={a.protectBook}>
                <span className="ic">📕</span>{tr("통합 문서 보호")}
              </Btn>
            </Group>
          </>
        )}
        {tab === "보기" && (
          <>
            <Group label={tr("통합 문서 보기")}>
              <Btn big title={tr("사용자 지정 보기: 숨긴 행·열과 필터를 이름으로 저장하고 다시 표시합니다")} onClick={a.customViews}>
                <span className="ic">▣</span>{tr("사용자 지정 보기")}
              </Btn>
            </Group>
            <Group label={tr("표시")}>
              <label className="rcheck">
                <input type="checkbox" checked={props.showGrid} onChange={a.toggleGrid} /> {tr("눈금선")}
              </label>
              <label className="rcheck">
                <input type="checkbox" checked={props.showPageBreaks} onChange={a.togglePageBreaks} /> {tr("페이지 나누기")}
              </label>
            </Group>
            <Group label={tr("확대/축소")}>
              <Btn big title={tr("확대/축소 100%")} onClick={() => a.zoom(1)}>
                <span className="ic">🔍</span>100%
              </Btn>
              <Btn big title={tr("선택 영역 확대/축소")} onClick={a.zoomToSelection}>
                <span className="ic">⤢</span>{tr("선택 영역")}
              </Btn>
            </Group>
            <Group label={tr("창")}>
              <Dropdown
                label={tr("틀 고정")}
                items={[
                  { label: tr("틀 고정"), onClick: () => a.freeze("panes") },
                  { label: tr("첫 행 고정"), onClick: () => a.freeze("row") },
                  { label: tr("첫 열 고정"), onClick: () => a.freeze("col") },
                  { label: tr("틀 고정 취소"), onClick: () => a.freeze("none") },
                ]}
              />
            </Group>
          </>
        )}
      </div>
    </div>
  );
}
