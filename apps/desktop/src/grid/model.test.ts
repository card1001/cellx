import { describe, expect, it } from "vitest";
import {
  colName,
  firstRowAt,
  parseAddr,
  parseTsv,
  rangeLabel,
  scrollHeightFor,
  scrollTopFor,
  toTsv,
} from "./model";

describe("addresses", () => {
  it("names columns like Excel", () => {
    expect(colName(1)).toBe("A");
    expect(colName(26)).toBe("Z");
    expect(colName(27)).toBe("AA");
    expect(colName(16384)).toBe("XFD");
  });
  it("parses addresses", () => {
    expect(parseAddr("b12")).toEqual({ r: 12, c: 2 });
    expect(parseAddr("$XFD$1048576")).toEqual({ r: 1048576, c: 16384 });
    expect(parseAddr("XFE1")).toBeNull();
    expect(parseAddr("A0")).toBeNull();
  });
  it("labels ranges", () => {
    expect(rangeLabel({ r: 3, c: 2 }, { r: 1, c: 1 })).toBe("A1:B3");
    expect(rangeLabel({ r: 1, c: 1 }, { r: 1, c: 1 })).toBe("A1");
  });
});

describe("clipboard", () => {
  it("round-trips tabs, newlines and quotes", () => {
    const rows = [["a", "b\tc"], ['say "hi"', "line1\nline2"]];
    expect(parseTsv(toTsv(rows))).toEqual(rows);
  });
  it("reads Excel's CRLF output", () => {
    expect(parseTsv("1\t2\r\n3\t4\r\n")).toEqual([["1", "2"], ["3", "4"]]);
  });
});

describe("compressed scrolling", () => {
  it("maps both ends of a 10M-row sheet", () => {
    const rows = 10_000_000;
    const h = 24;
    const viewport = 600;
    const max = scrollHeightFor(rows, h) - viewport;
    expect(firstRowAt(0, rows, h, viewport)).toBe(0);
    const last = firstRowAt(max, rows, h, viewport);
    expect(Math.round(last + viewport / h)).toBe(rows);
    expect(scrollTopFor(last, rows, h, viewport)).toBeCloseTo(max, 3);
  });
  it("is exact for small sheets", () => {
    expect(firstRowAt(240, 1000, 24, 600)).toBe(10);
  });
});
