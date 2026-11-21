import { describe, expect, it } from "vitest";
import {
  canPointAt,
  colourPieces,
  completeFunctions,
  cycleAbsolute,
  functionPrefixAt,
  insertRef,
  refSpans,
} from "./formula";

describe("reference spans", () => {
  it("finds cells and ranges, skipping strings, functions and table columns", () => {
    const s = refSpans('=SUM(A1:B3)+LOG10(C4)&"D5"+orders[E6]+Sheet2!F7');
    expect(s.map((x) => [x.r0, x.c0, x.r1, x.c1, x.sheet])).toEqual([
      [1, 1, 3, 2, null],
      [4, 3, 4, 3, null],
      [7, 6, 7, 6, "Sheet2"],
    ]);
  });
  it("gives the same reference the same colour", () => {
    const s = refSpans("=A1+B2+$A$1");
    expect(s[0].color).toBe(s[2].color);
    expect(s[1].color).not.toBe(s[0].color);
  });
  it("splits text for colouring", () => {
    expect(colourPieces("=A1+1").map((p) => p.text)).toEqual(["=", "A1", "+1"]);
  });
});

describe("F4", () => {
  it("cycles like Excel", () => {
    let r = cycleAbsolute("=SUM(A1", 7)!;
    expect(r).toEqual({ text: "=SUM($A$1", caret: 9 });
    r = cycleAbsolute(r.text, r.caret)!;
    expect(r.text).toBe("=SUM(A$1");
    r = cycleAbsolute(r.text, r.caret)!;
    expect(r.text).toBe("=SUM($A1");
    r = cycleAbsolute(r.text, r.caret)!;
    expect(r.text).toBe("=SUM(A1");
  });
  it("does nothing away from a reference", () => {
    expect(cycleAbsolute("=1+2", 3)).toBeNull();
  });
});

describe("point mode", () => {
  it("knows where a reference can be inserted", () => {
    expect(canPointAt("=", 1)).toBe(true);
    expect(canPointAt("=SUM(", 5)).toBe(true);
    expect(canPointAt("=A1+", 4)).toBe(true);
    expect(canPointAt("=A1", 3)).toBe(false);
    expect(canPointAt('="a+', 4)).toBe(false);
    expect(canPointAt("abc", 3)).toBe(false);
  });
  it("inserts then replaces the pointed reference", () => {
    let r = insertRef("=SUM(", 5, null, { r: 1, c: 1 }, { r: 1, c: 1 }, null);
    expect(r.text).toBe("=SUM(A1");
    r = insertRef(r.text, r.caret, r.point, { r: 1, c: 1 }, { r: 3, c: 2 }, null);
    expect(r.text).toBe("=SUM(A1:B3");
    r = insertRef(r.text + ")", r.caret, r.point, { r: 2, c: 2 }, { r: 2, c: 2 }, "My Sheet");
    expect(r.text).toBe("=SUM('My Sheet'!B2)");
  });
});

describe("autocomplete", () => {
  it("finds the function being typed", () => {
    expect(functionPrefixAt("=A1+su", 6)).toEqual({ start: 4, prefix: "su" });
    expect(functionPrefixAt("=A1", 3)).toBeNull();
    expect(functionPrefixAt('="su', 4)).toBeNull();
    expect(completeFunctions(["SUM", "SUMIF", "AVERAGE"], "su")).toEqual(["SUM", "SUMIF"]);
  });
});
