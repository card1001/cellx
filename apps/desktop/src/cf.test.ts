import { describe, expect, it } from "vitest";
import { cfOperand, colorScaleRule, CF_STYLES, describeRule, highlightRule, iconSetRule, ICON_SETS } from "./cf";
import { niceScale } from "./components/Chart";

describe("conditional formatting rules", () => {
  it("quotes text but keeps numbers and references", () => {
    expect(cfOperand("100")).toBe("100");
    expect(cfOperand("=$B$1")).toBe("$B$1");
    expect(cfOperand('서울 "본사"')).toBe('"서울 ""본사"""');
  });

  it("builds IronCalc rule inputs", () => {
    const r = highlightRule("Between", "10", "20", CF_STYLES[0]);
    expect(r).toMatchObject({ type: "CellIs", operator: "Between", formula: "10", formula2: "20" });
    expect(colorScaleRule(["#F8696B", "#FFEB84", "#63BE7B"]).thresholds).toHaveLength(3);
    const icons = iconSetRule(ICON_SETS[0].icons as [string, string][]);
    expect((icons.thresholds as { cfvo: unknown }[]).map((t) => t.cfvo)).toEqual([{ Percent: 0 }, { Percent: 33 }, { Percent: 67 }]);
  });

  it("describes stored rules", () => {
    expect(describeRule({ type: "CellIs", operator: "GreaterThan", formula: "60" })).toBe("셀 값 > 60");
    expect(describeRule({ type: "Top10", rank: 5, percent: false })).toBe("상위 5개");
  });
});

describe("chart axis", () => {
  it("rounds like Excel's automatic scale", () => {
    expect(niceScale(0, 97)).toEqual({ min: 0, max: 100, step: 20 });
    expect(niceScale(-30, 120).min).toBeLessThanOrEqual(-30);
    expect(niceScale(5, 5).max).toBeGreaterThanOrEqual(5);
  });
});
