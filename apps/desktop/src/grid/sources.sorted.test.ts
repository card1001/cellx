import { afterEach, describe, expect, it, vi } from "vitest";
import { api, type KeysetPage, type Page, type Sort, type SortCursor, type TableInfo } from "../api";
import { DataSource } from "./sources";
import { estimatePageBytes } from "./performance";

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


  it("does not let an in-flight sorted response overwrite a newer sorted view", async () => {
    let resolveOld!: (value: KeysetPage) => void;
    let resolveNew!: (value: KeysetPage) => void;
    const oldRequest = new Promise<KeysetPage>((resolve) => { resolveOld = resolve; });
    const newRequest = new Promise<KeysetPage>((resolve) => { resolveNew = resolve; });
    vi.mocked(api.dataPageAfterSort)
      .mockReturnValueOnce(oldRequest)
      .mockReturnValueOnce(newRequest);
    const source = new DataSource(info, ["Arial", 10]);

    source.setView(sort, []);
    source.setView({ column: "value", descending: true }, []);

    resolveOld(keysetPage("stale", cursor));
    await Promise.resolve();
    expect(source.cell(1, 1)).toBeUndefined();

    resolveNew(keysetPage("current", cursor));
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("current"));
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


describe("DataSource page-cache accounting", () => {
  it("tracks retained bytes and resets accounting on reload", async () => {
    const firstPage = page("payload");
    vi.mocked(api.dataPageAfterSort).mockResolvedValueOnce({
      page: firstPage,
      next_cursor: cursor,
    });
    const source = new DataSource(info, ["Arial", 10]);
    source.setView(sort, []);
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("payload"));

    expect(source.getDiagnosticsSnapshot()).toMatchObject({
      cachedPages: 1,
      cachedBytes: estimatePageBytes(firstPage),
      maxCachedBytes: 64 * 1024 * 1024,
    });

    source.reload();
    expect(source.getDiagnosticsSnapshot()).toMatchObject({
      cachedPages: 0,
      cachedBytes: 0,
    });
  });

  it("keeps forward cursor checkpoints when an old page is evicted", async () => {
    const requests: (SortCursor | null)[] = [];
    let sequence = 0;
    vi.mocked(api.dataPageAfterSort).mockImplementation(async (_table, previous) => {
      requests.push(previous);
      const current = sequence++;
      return keysetPage(`row-${current}`, {
        value: `value-${current}`,
        rid: current + 1,
        query_id: "stable-query",
      });
    });
    const source = new DataSource({ ...info, rows: 20_000 }, ["Arial", 10]);
    source.setView(sort, []);
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("row-0"));

    for (let pageIndex = 1; pageIndex <= 64; pageIndex++) {
      source.request(pageIndex * 256 + 1, 1, pageIndex * 256 + 256);
      await vi.waitFor(() => expect(source.cell(pageIndex * 256 + 1, 1)?.text).toBe(`row-${pageIndex}`));
    }

    expect(source.getDiagnosticsSnapshot().cachedPages).toBe(64);
    expect(source.cell(1, 1)).toBeUndefined();

    source.request(65 * 256 + 1, 1, 66 * 256);
    await vi.waitFor(() => expect(requests).toHaveLength(66));
    expect(requests[65]).toEqual({
      value: "value-64",
      rid: 65,
      query_id: "stable-query",
    });
  });
});
