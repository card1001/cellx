// Number formats as offered by Korean Excel's Home tab and Format Cells dialog.

export interface FormatChoice {
  label: string;
  code: string;
  sample: string;
}

export const FORMAT_CHOICES: FormatChoice[] = [
  { label: "일반", code: "general", sample: "1234.5" },
  { label: "숫자", code: "#,##0.00", sample: "1,234.50" },
  { label: "통화", code: "₩#,##0", sample: "₩1,235" },
  { label: "회계", code: '_-₩* #,##0_-;-₩* #,##0_-;_-₩* "-"_-;_-@_-', sample: "₩ 1,235" },
  { label: "간단한 날짜", code: "yyyy-mm-dd", sample: "2026-11-21" },
  { label: "자세한 날짜", code: 'yyyy"년" m"월" d"일"', sample: "2026년 11월 21일" },
  { label: "시간", code: "h:mm:ss", sample: "13:30:55" },
  { label: "백분율", code: "0.00%", sample: "12.50%" },
  { label: "분수", code: "# ?/?", sample: "1234 1/2" },
  { label: "지수", code: "0.00E+00", sample: "1.23E+03" },
  { label: "텍스트", code: "@", sample: "1234.5" },
];

export function formatLabel(code: string): string {
  const c = code.trim();
  if (!c || c.toLowerCase() === "general") return "일반";
  return FORMAT_CHOICES.find((f) => f.code === c)?.label ?? "사용자 지정";
}

/** Excel's "자릿수 늘림": one more decimal place in every numeric section. */
export function increaseDecimals(code: string): string {
  if (!code || code.toLowerCase() === "general") return "0.0";
  return code
    .split(";")
    .map((section) => {
      if (/0\.0*/.test(section)) return section.replace(/(0\.0*)/, "$10");
      return section.replace(/0(?![^"]*"(?:[^"]*"[^"]*")*[^"]*$)/, "0.0");
    })
    .join(";");
}

/** Excel's "자릿수 줄임". */
export function decreaseDecimals(code: string): string {
  if (!code || code.toLowerCase() === "general") return "0";
  return code
    .split(";")
    .map((section) => section.replace(/0\.(0*)0/, (_m, rest: string) => (rest.length ? `0.${rest}` : "0")))
    .join(";");
}
