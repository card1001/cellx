/**
 * Excel's General number format fitted to a column: as many digits as the
 * width allows, up to 15 significant digits. A number that does not fit loses
 * decimals first, then switches to scientific notation with a shorter
 * mantissa; `null` when not even that fits (the grid then shows ####).
 */
export function fitGeneral(value: number, fits: (text: string) => boolean): string | null {
  if (!Number.isFinite(value)) return null;
  for (const text of generalCandidates(value)) {
    if (fits(text)) return text;
  }
  return null;
}

/** The General texts of a number, from the most to the least precise. */
export function generalCandidates(value: number): string[] {
  const out: string[] = [];
  const abs = Math.abs(value);
  if (abs === 0) return ["0"];
  if (abs < 1e15 && abs >= 1e-9) {
    const intDigits = abs < 1 ? 1 : Math.floor(Math.log10(abs)) + 1;
    // Significant digits after the integer part (15 in all), and for a number
    // below 1 its leading zeros on top.
    const lead = abs < 1 ? -Math.floor(Math.log10(abs)) - 1 : 0;
    const maxDecimals = Math.min(20, abs < 1 ? 15 + lead : Math.max(0, 15 - intDigits));
    let last = "";
    for (let d = maxDecimals; d >= 0; d--) {
      const t = trimZeros(value.toFixed(d));
      if (t === last) continue;
      // Rounding must not turn the number into 0.
      if (t === "0" || t === "-0") break;
      out.push(t);
      last = t;
    }
  }
  let last = "";
  for (let d = 14; d >= 0; d--) {
    const t = excelExponent(value.toExponential(d));
    if (t !== last) out.push(t);
    last = t;
  }
  return out;
}

function trimZeros(t: string): string {
  return t.includes(".") ? t.replace(/0+$/, "").replace(/\.$/, "") : t;
}

/** `1.2345e+11` → `1.2345E+11`, with at least two exponent digits. */
function excelExponent(t: string): string {
  const [m, e] = t.split("e");
  const exp = Number(e);
  const sign = exp < 0 ? "-" : "+";
  return `${trimZeros(m)}E${sign}${String(Math.abs(exp)).padStart(2, "0")}`;
}
