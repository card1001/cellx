import { describe, expect, it } from "vitest";
import { isoToSerial, serialToIso, splitPreview } from "./components/DataDialogs";

const base = { delimiters: ",", consecutive: false, qualifier: '"', widths: null, dest_row: 1, dest_col: 1 };

describe("text to columns preview", () => {
  it("matches the engine's split", () => {
    expect(splitPreview('a,"b,c",,d', base)).toEqual(["a", "b,c", "", "d"]);
    expect(splitPreview("홍  길동", { ...base, delimiters: " ", consecutive: true })).toEqual(["홍", "길동"]);
    expect(splitPreview("2026-11-22", { ...base, widths: [4, 6] })).toEqual(["2026", "-1", "1-22"]);
  });
});

describe("validation dates", () => {
  it("converts between yyyy-mm-dd and Excel serials", () => {
    expect(isoToSerial("2026-01-01")).toBe(46023);
    expect(serialToIso(46023)).toBe("2026-01-01");
    expect(isoToSerial("2026/1/5")).toBe(46027);
  });
});
