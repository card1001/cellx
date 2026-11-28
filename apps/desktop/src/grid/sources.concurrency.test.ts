import { afterEach, describe, expect, it, vi } from "vitest";
import { api, type Page, type TableInfo } from "../api";
import { DataSource } from "./sources";

vi.mock("../api", () => ({
  api: {
    dataPage: vi.fn(),
    dataPageAfterRid: vi.fn(),
  },
}));

const info: TableInfo = {
  name: "items",
  rows: 512,
  columns: [{ name: "value", kind: "Text", sql_type: "VARCHAR" }],
};

function page(value: string): Page {
  return {
    columns: ["value"],
    row_ids: [1],
    rows: [[{ t: "Text", v: value }]],
    total: 512,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

afterEach(() => vi.clearAllMocks());

describe("DataSource page request concurrency", () => {
  it("deduplicates concurrent requests for the same page", async () => {
    const pending = deferred<Page>();
    vi.mocked(api.dataPageAfterRid).mockReturnValue(pending.promise);
    const source = new DataSource(info, ["Arial", 10]);

    source.request(1, 1, 100);
    source.request(1, 1, 100);

    expect(api.dataPageAfterRid).toHaveBeenCalledTimes(1);
    pending.resolve(page("single"));
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("single"));
  });

  it("does not let an old view response overwrite the newer view", async () => {
    const oldRequest = deferred<Page>();
    const newRequest = deferred<Page>();
    vi.mocked(api.dataPageAfterRid).mockReturnValueOnce(oldRequest.promise);
    vi.mocked(api.dataPage).mockReturnValueOnce(newRequest.promise);
    const source = new DataSource(info, ["Arial", 10]);

    source.request(1, 1, 100);
    source.setView({ column: "value", descending: true }, []);

    oldRequest.resolve(page("stale"));
    await Promise.resolve();
    expect(source.cell(1, 1)).toBeUndefined();

    newRequest.resolve(page("current"));
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("current"));
  });

  it("does not allow an older request finalizer to clear a newer in-flight token", async () => {
    const first = deferred<Page>();
    const second = deferred<Page>();
    vi.mocked(api.dataPageAfterRid)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const source = new DataSource(info, ["Arial", 10]);

    source.request(1, 1, 100);
    source.reload();
    first.resolve(page("old"));
    await Promise.resolve();

    expect(source.getDiagnosticsSnapshot().inflightPages).toBe(1);
    second.resolve(page("new"));
    await vi.waitFor(() => expect(source.cell(1, 1)?.text).toBe("new"));
    expect(source.getDiagnosticsSnapshot().inflightPages).toBe(0);
  });
});
