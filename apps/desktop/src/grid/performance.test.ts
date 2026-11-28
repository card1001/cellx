import { describe, expect, it } from "vitest";
import { estimateSerializedBytes, selectPagesToEvict } from "./performance";

describe("grid cache performance helpers", () => {
  it("evicts least recently used pages but preserves visible pages", () => {
    const access = new Map([[0, 1], [1, 2], [2, 3], [3, 4]]);
    expect(selectPagesToEvict(access, new Set([3]), 2)).toEqual([0, 1]);
  });

  it("does not evict when the cache is within its bound", () => {
    expect(selectPagesToEvict(new Map([[0, 1], [1, 2]]), new Set(), 2)).toEqual([]);
  });

  it("estimates serialized payload bytes without throwing", () => {
    const value = { value: "abc" };
    expect(estimateSerializedBytes(value)).toBe(JSON.stringify(value).length * 2);
    expect(estimateSerializedBytes(undefined)).toBe(0);
  });
});
