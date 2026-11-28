import { afterEach, describe, expect, it, vi } from "vitest";
import { api, type KeysetPage, type Page, type Sort, type SortCursor, type TableInfo } from "../api";
import { DataSource } from "./sources";

vi.mock("../api", () => ({
  api: {
    dataPage: vi.fn(),
    dataPageAfterRid: vi.fn(),
    dataPageAfterSort: vi.fn(),
  },
}));

const info: TableInfo = {
  name: "items",
  rows: 1024,
  columns: [{ name: "value", kind: "Text", sql_type: "VARCHAR" }],
};
const sort: Sort = { column: "value", descending: false };
const cursor: SortCursor = { value: "first", rid: 10, query_id: "query-v1" };

function page(value: string): Page {
  return {
    columns: ["value"],
    row_ids: [10],
    rows: [[{ t: "Text", v: value }]],
    total: 1024,
  };
}

function keysetPage(value: string, next_cursor: SortCursor | null): KeysetPage {
  return { page: page(value), next_cursor };
}

afterEach(() => vi.clearAllMocks());

describe("DataSource sorted keyset paging", () => {
  it("uses the previous page cursor for sequential sorted navigation", async () => {
    vi.mocked(api.dataPageAfterSort)
      .mockResolvedValueOnce(keysetPage("first", cursor))
      .mockResolvedValueOnce(keysetPage("second", null));
    const source = new DataSource(info, ["Arial", 10]);

    source.setView(sort, []);
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("first"));

    source.request(257, 1, 512);
    await vi.waitFor(() => expect(api.dataPageAfterSort).toHaveBeenCalledTimes(2));

    expect(api.dataPageAfterSort).toHaveBeenNthCalledWith(1, "items", null, 256, sort, []);
    expect(api.dataPageAfterSort).toHaveBeenNthCalledWith(2, "items", cursor, 256, sort, []);
    await vi.waitFor(() => expect(source.cell(257, 1)?.text).toBe("second"));
  });

  it("clears sorted cursors on reload and starts again without a stale cursor", async () => {
    vi.mocked(api.dataPageAfterSort)
      .mockResolvedValueOnce(keysetPage("before reload", cursor))
      .mockResolvedValueOnce(keysetPage("after reload", cursor));
    const source = new DataSource(info, ["Arial", 10]);

    source.setView(sort, []);
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("before reload"));

    source.reload();
    await vi.waitFor(() => expect(api.dataPageAfterSort).toHaveBeenCalledTimes(2));

    expect(api.dataPageAfterSort).toHaveBeenNthCalledWith(2, "items", null, 256, sort, []);
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("after reload"));
  });
});
