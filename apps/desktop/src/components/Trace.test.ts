import { describe, expect, it } from "vitest";
import { stepParts } from "./Trace";

describe("수식 계산 display", () => {
  it("underlines the next part and italicises the last result", () => {
    const prev = { text: "=IF(\u0002A1\u0003>2,1,0)", value: "3" };
    const cur = { text: "=IF(\u00023>2\u0003,1,0)", value: "TRUE" };
    const parts = stepParts(cur, prev);
    expect(parts.map((p) => p.text).join("")).toBe("=IF(3>2,1,0)");
    expect(parts.filter((p) => p.under).map((p) => p.text).join("")).toBe("3>2");
    expect(parts.filter((p) => p.italic).map((p) => p.text).join("")).toBe("3");
    expect(stepParts({ text: "=14", value: "14" }).map((p) => p.under)).toEqual([false]);
  });
});
