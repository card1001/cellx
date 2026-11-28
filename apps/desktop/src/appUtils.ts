import { formatLocaleNumber, getActiveLocale } from "./i18n";

const measureCtx = typeof document !== "undefined" ? document.createElement("canvas").getContext("2d") : null;

/** The cell editor grows to the right with its text, as in Excel. */
export function editorWidth(text: string, font: string, min: number): number {
  if (!measureCtx) return min;
  measureCtx.font = font;
  const longest = Math.max(...text.split("\n").map((line) => measureCtx.measureText(line).width));
  return Math.max(min, Math.ceil(longest + 14));
}

export function formatCount(n: number): string {
  return formatLocaleNumber(n, getActiveLocale(), { maximumFractionDigits: 0 });
}

export function formatNumber(n: number): string {
  // Status-bar formatting follows the UI locale; workbook cell formats remain unchanged.
  const options = Number.isInteger(n) ? undefined : { maximumSignificantDigits: 10 };
  return formatLocaleNumber(n, getActiveLocale(), options);
}
