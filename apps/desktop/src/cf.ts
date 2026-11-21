// Conditional formatting presets, built like Excel's 홈 → 조건부 서식 menu.
import type { CfRule } from "./api";

export interface CfStyle {
  label: string;
  font?: string;
  fill?: string;
}

/** Excel's preset formats for highlight rules. */
export const CF_STYLES: CfStyle[] = [
  { label: "진한 빨강 텍스트가 있는 연한 빨강 채우기", font: "#9C0006", fill: "#FFC7CE" },
  { label: "진한 노랑 텍스트가 있는 노랑 채우기", font: "#9C5700", fill: "#FFEB9C" },
  { label: "진한 녹색 텍스트가 있는 녹색 채우기", font: "#006100", fill: "#C6EFCE" },
  { label: "연한 빨강 채우기", fill: "#FFC7CE" },
  { label: "빨강 텍스트", font: "#9C0006" },
];

export function dxf(style: CfStyle) {
  return {
    font: style.font ? { color: style.font } : null,
    fill: style.fill ? { color: style.fill } : null,
    border: null,
    num_fmt: null,
    alignment: null,
  };
}

/** Rules that ask for a value (or two) and a format. */
export type HighlightKind =
  | "GreaterThan"
  | "LessThan"
  | "Between"
  | "Equal"
  | "TextContains"
  | "Duplicate"
  | "Top10"
  | "Bottom10"
  | "AboveAverage"
  | "BelowAverage";

export const HIGHLIGHT: Record<HighlightKind, { title: string; prompt: string; values: 0 | 1 | 2 }> = {
  GreaterThan: { title: "보다 큼", prompt: "다음 값보다 큰 셀의 서식 지정:", values: 1 },
  LessThan: { title: "보다 작음", prompt: "다음 값보다 작은 셀의 서식 지정:", values: 1 },
  Between: { title: "해당 범위", prompt: "다음 값 사이에 있는 셀의 서식 지정:", values: 2 },
  Equal: { title: "같음", prompt: "다음 값과 같은 셀의 서식 지정:", values: 1 },
  TextContains: { title: "텍스트 포함", prompt: "다음 텍스트를 포함하는 셀의 서식 지정:", values: 1 },
  Duplicate: { title: "중복 값", prompt: "다음 값을 포함하는 셀의 서식 지정: 중복", values: 0 },
  Top10: { title: "상위 10개 항목", prompt: "상위 순위에 속하는 셀의 서식 지정 (개수):", values: 1 },
  Bottom10: { title: "하위 10개 항목", prompt: "하위 순위에 속하는 셀의 서식 지정 (개수):", values: 1 },
  AboveAverage: { title: "평균 초과", prompt: "선택한 범위에서 평균보다 큰 셀의 서식 지정", values: 0 },
  BelowAverage: { title: "평균 미만", prompt: "선택한 범위에서 평균보다 작은 셀의 서식 지정", values: 0 },
};

/** Values typed in the dialog become formulas: numbers and references stay, text is quoted. */
export function cfOperand(v: string): string {
  const t = v.trim();
  if (t.startsWith("=")) return t.slice(1);
  if (t !== "" && !Number.isNaN(Number(t))) return t;
  return `"${t.replace(/"/g, '""')}"`;
}

export function highlightRule(kind: HighlightKind, a: string, b: string, style: CfStyle): CfRule {
  const format = dxf(style);
  const base = { format, stop_if_true: false };
  switch (kind) {
    case "GreaterThan":
    case "LessThan":
    case "Equal":
      return { type: "CellIs", operator: kind, formula: cfOperand(a), formula2: null, ...base };
    case "Between":
      return { type: "CellIs", operator: "Between", formula: cfOperand(a), formula2: cfOperand(b), ...base };
    case "TextContains":
      return { type: "Text", operator: "Contains", value: a, ...base };
    case "Duplicate":
      return { type: "DuplicateValues", ...base };
    case "Top10":
      return { type: "Top10", rank: Math.max(1, Math.round(Number(a) || 10)), percent: false, ...base };
    case "Bottom10":
      return { type: "Bottom10", rank: Math.max(1, Math.round(Number(a) || 10)), percent: false, ...base };
    case "AboveAverage":
      return { type: "AboveAverage", ...base };
    case "BelowAverage":
      return { type: "BelowAverage", ...base };
  }
}

export const DATA_BARS: { label: string; color: string; gradient: boolean }[] = [
  { label: "파랑 데이터 막대 (그라데이션)", color: "#638EC6", gradient: true },
  { label: "녹색 데이터 막대 (그라데이션)", color: "#63C384", gradient: true },
  { label: "빨강 데이터 막대 (그라데이션)", color: "#FF555A", gradient: true },
  { label: "주황 데이터 막대 (그라데이션)", color: "#FFB628", gradient: true },
  { label: "파랑 데이터 막대 (단색)", color: "#638EC6", gradient: false },
  { label: "녹색 데이터 막대 (단색)", color: "#63C384", gradient: false },
];

export function dataBarRule(color: string, gradient: boolean): CfRule {
  return {
    type: "DataBar",
    min: null,
    max: null,
    positive_color: color,
    negative_color: "#FF0000",
    is_gradient: gradient,
    show_value: true,
  };
}

export const COLOR_SCALES: { label: string; colors: string[] }[] = [
  { label: "녹색 - 노랑 - 빨강 색조", colors: ["#F8696B", "#FFEB84", "#63BE7B"] },
  { label: "빨강 - 노랑 - 녹색 색조", colors: ["#63BE7B", "#FFEB84", "#F8696B"] },
  { label: "녹색 - 흰색 - 빨강 색조", colors: ["#F8696B", "#FCFCFF", "#63BE7B"] },
  { label: "파랑 - 흰색 - 빨강 색조", colors: ["#F8696B", "#FCFCFF", "#5A8AC6"] },
  { label: "흰색 - 빨강 색조", colors: ["#FCFCFF", "#F8696B"] },
  { label: "녹색 - 흰색 색조", colors: ["#FCFCFF", "#63BE7B"] },
];

/** Colors listed from the lowest value to the highest. */
export function colorScaleRule(colors: string[]): CfRule {
  const cfvo = colors.length === 3 ? ["Min", { Percentile: 50 }, "Max"] : ["Min", "Max"];
  return { type: "ColorScale", thresholds: colors.map((color, i) => ({ cfvo: cfvo[i], color })) };
}

type IconSpec = [string, string];
export const ICON_SETS: { label: string; icons: IconSpec[] }[] = [
  { label: "3방향 화살표 (컬러)", icons: [["ArrowDown", "#E43400"], ["ArrowRight", "#FFC000"], ["ArrowUp", "#84CB1F"]] },
  { label: "3색 신호등", icons: [["Circle", "#F8696B"], ["Circle", "#FFEB84"], ["Circle", "#63BE7B"]] },
  { label: "3가지 기호", icons: [["Cross", "#F8696B"], ["Exclamation", "#FFC000"], ["Check", "#63BE7B"]] },
  { label: "3개 삼각형", icons: [["TriangleDown", "#F8696B"], ["FlatRectangle", "#FFC000"], ["TriangleUp", "#63BE7B"]] },
  { label: "5방향 화살표", icons: [["ArrowDown", "#E43400"], ["ArrowAngleDown", "#FFC000"], ["ArrowRight", "#FFC000"], ["ArrowAngleUp", "#FFC000"], ["ArrowUp", "#84CB1F"]] },
];

/** Icons listed from the lowest bucket up; thresholds split 0..100 percent evenly. */
export function iconSetRule(icons: IconSpec[]): CfRule {
  const n = icons.length;
  return {
    type: "IconSet",
    show_value: true,
    thresholds: icons.map(([icon, color], i) => ({
      icon,
      color,
      cfvo: { Percent: Math.round((i * 100) / n) },
      is_strict: false,
    })),
  };
}

export function ratingRule(): CfRule {
  return {
    type: "IconRating",
    icon: "Star",
    color: "#FFC000",
    thresholds: [
      [{ Percent: 80 }, false],
      [{ Percent: 60 }, false],
      [{ Percent: 40 }, false],
      [{ Percent: 20 }, false],
    ],
    show_value: true,
  };
}

/** A one-line description of a stored rule for the rule manager. */
export function describeRule(rule: unknown): string {
  if (!rule || typeof rule !== "object") return String(rule);
  const body = rule as Record<string, unknown>;
  const kind = String(body.type ?? "");
  const op: Record<string, string> = {
    GreaterThan: ">",
    GreaterThanOrEqual: ">=",
    LessThan: "<",
    LessThanOrEqual: "<=",
    Equal: "=",
    NotEqual: "<>",
    Between: "사이",
    NotBetween: "사이 아님",
  };
  switch (kind) {
    case "CellIs":
      return `셀 값 ${op[String(body.operator)] ?? body.operator} ${body.formula}${body.formula2 ? ` ~ ${body.formula2}` : ""}`;
    case "Text":
      return `텍스트 포함 "${body.value}"`;
    case "DuplicateValues":
      return "중복 값";
    case "UniqueValues":
      return "고유 값";
    case "Top10":
      return `상위 ${body.rank}${body.percent ? "%" : "개"}`;
    case "Bottom10":
      return `하위 ${body.rank}${body.percent ? "%" : "개"}`;
    case "AboveAverage":
      return "평균 초과";
    case "BelowAverage":
      return "평균 미만";
    case "DataBar":
      return "데이터 막대";
    case "ColorScale":
      return "색조";
    case "IconSet":
      return "아이콘 집합";
    case "IconRating":
      return "별점";
    case "Formula":
      return `수식: =${body.formula}`;
    default:
      return kind;
  }
}
