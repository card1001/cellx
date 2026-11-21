import { describe, expect, it } from "vitest";
import { Axis } from "./axis";

describe("Axis", () => {
  it("uses the default size", () => {
    const a = new Axis(100, 22);
    expect(a.offset(1)).toBe(0);
    expect(a.offset(11)).toBe(220);
    expect(a.indexAt(221)).toBe(11);
    expect(a.total()).toBe(2200);
  });
  it("accounts for custom and hidden rows", () => {
    const a = new Axis(1_048_576, 22, [[3, 40]], [5]);
    expect(a.offset(4)).toBe(22 + 22 + 40);
    expect(a.offset(6)).toBe(4 * 22 + 18); // row 5 hidden
    expect(a.indexAt(22 * 2 + 39)).toBe(3);
    expect(a.indexAt(a.offset(6))).toBe(6);
    expect(a.hidden(5)).toBe(true);
  });
  it("finds the last rows of a huge sheet quickly", () => {
    const a = new Axis(1_048_576, 22);
    expect(a.indexAt(a.total() - 1)).toBe(1_048_576);
  });
});
