import { describe, expect, it } from "vitest";
import { decreaseDecimals, formatLabel, increaseDecimals } from "./formats";

describe("decimal buttons", () => {
  it("adds and removes decimal places", () => {
    expect(increaseDecimals("general")).toBe("0.0");
    expect(increaseDecimals("#,##0")).toBe("#,##0.0");
    expect(increaseDecimals("0.00%")).toBe("0.000%");
    expect(decreaseDecimals("0.00%")).toBe("0.0%");
    expect(decreaseDecimals("#,##0.0")).toBe("#,##0");
    expect(decreaseDecimals("#,##0")).toBe("#,##0");
  });
  it("names known formats", () => {
    expect(formatLabel("general")).toBe("일반");
    expect(formatLabel("yyyy-mm-dd")).toBe("간단한 날짜");
    expect(formatLabel("0.000")).toBe("사용자 지정");
  });
});
