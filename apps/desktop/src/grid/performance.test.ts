import { describe, expect, it } from "vitest";
import { estimatePageBytes, isCurrentPageRequest, selectPagesToEvict, selectPagesToEvictByBudget } from "./performance";

describe("grid cache performance helpers", () => {
  it("evicts least recently used pages but preserves visible pages", () => {
    const access = new Map([[0, 1], [1, 2], [2, 3], [3, 4]]);
    expect(selectPagesToEvict(access, new Set([3]), 2)).toEqual([0, 1]);
  });

  it("does not evict when the cache is within its bound", () => {
    expect(selectPagesToEvict(new Map([[0, 1], [1, 2]]), new Set(), 2)).toEqual([]);
  });

});

describe("page request ownership", () => {
  it("rejects responses from an old generation or superseded request", () => {
    expect(isCurrentPageRequest(3, 3, 8, 8)).toBe(true);
    expect(isCurrentPageRequest(4, 3, 8, 8)).toBe(false);
    expect(isCurrentPageRequest(3, 3, 9, 8)).toBe(false);
  });
});


describe("memory-budgeted page cache", () => {
  it("estimates payload growth for longer text values", () => {
    const small = estimatePageBytes({ columns: ["a"], row_ids: [1], rows: [[{ t: "Text", v: "x" }]] });
    const large = estimatePageBytes({ columns: ["a"], row_ids: [1], rows: [[{ t: "Text", v: "x".repeat(1000) }]] });
    expect(large - small).toBe(1998);
  });

  it("evicts LRU pages until both page-count and byte limits fit", () => {
    const access = new Map([[0, 1], [1, 2], [2, 3]]);
    const sizes = new Map([[0, 40], [1, 40], [2, 40]]);
    expect(selectPagesToEvictByBudget(access, sizes, new Set([2]), 3, 50)).toEqual([0, 1]);
  });

  it("keeps visible pages even when they exceed the byte budget", () => {
    const access = new Map([[0, 1], [1, 2]]);
    const sizes = new Map([[0, 80], [1, 80]]);
    expect(selectPagesToEvictByBudget(access, sizes, new Set([0, 1]), 64, 64)).toEqual([]);
  });

  it("does not count orphan byte entries outside the page index", () => {
    const access = new Map([[0, 1]]);
    const sizes = new Map([[0, 10], [99, 1_000]]);
    expect(selectPagesToEvictByBudget(access, sizes, new Set(), 1, 10)).toEqual([]);
  });
});
