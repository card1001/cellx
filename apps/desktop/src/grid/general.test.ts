import { describe, expect, it } from "vitest";
import { fitGeneral } from "./general";

const width = (n: number) => (t: string) => t.length <= n;

describe("fitGeneral", () => {
  it("shows every digit when the column is wide", () => {
    expect(fitGeneral(123456789012, width(20))).toBe("123456789012");
    expect(fitGeneral(9876543210123, width(20))).toBe("9876543210123");
    expect(fitGeneral(12345678.123456, width(20))).toBe("12345678.123456");
    expect(fitGeneral(0.1 + 0.2, width(20))).toBe("0.3");
  });
  it("drops decimals, then switches to scientific notation", () => {
    expect(fitGeneral(12345678.123456, width(11))).toBe("12345678.12");
    expect(fitGeneral(123456789012, width(11))).toBe("1.23457E+11");
    expect(fitGeneral(123456789012, width(7))).toBe("1.2E+11");
    expect(fitGeneral(-1234567890123, width(9))).toBe("-1.23E+12");
  });
  it("keeps 15 significant digits at most", () => {
    expect(fitGeneral(2 ** 53, width(30))).toBe("9.00719925474099E+15");
    expect(fitGeneral(1 / 3, width(30))).toBe("0.333333333333333");
  });
  it("gives up when nothing fits", () => {
    expect(fitGeneral(123456789012, width(4))).toBeNull();
  });
});
