/** Select least-recently-used cached pages, preserving the current viewport. */
export function selectPagesToEvict(
  access: Map<number, number>,
  protectedPages: Set<number>,
  maxPages: number,
): number[] {
  const excess = Math.max(0, access.size - Math.max(0, maxPages));
  if (!excess) return [];
  return [...access.entries()]
    .filter(([page]) => !protectedPages.has(page))
    .sort((a, b) => a[1] - b[1])
    .slice(0, excess)
    .map(([page]) => page);
}

/** A response may update cache state only while both generation and request token match. */
export function isCurrentPageRequest(
  currentGeneration: number,
  requestGeneration: number,
  activeToken: number | undefined,
  requestToken: number,
): boolean {
  return currentGeneration === requestGeneration && activeToken === requestToken;
}


/**
 * Deterministic estimate of retained page payload. This is deliberately an
 * estimate, not an exact JavaScript heap measurement.
 */
export function estimatePageBytes(page: {
  columns: string[];
  row_ids: number[];
  rows: { t: string; v?: string | number | boolean }[][];
}): number {
  let bytes = 128 + page.row_ids.length * 16;
  for (const column of page.columns) bytes += 40 + column.length * 2;
  for (const row of page.rows) {
    bytes += 24 + row.length * 8;
    for (const value of row) {
      bytes += 16;
      if (value.t === "Text" || value.t === "Error") {
        bytes += typeof value.v === "string" ? value.v.length * 2 : 0;
      } else if (value.t === "Number") {
        bytes += 8;
      } else if (value.t === "Bool") {
        bytes += 4;
      }
    }
  }
  return bytes;
}

/** Evict LRU pages until both limits fit; visible pages are never evicted. */
export function selectPagesToEvictByBudget(
  access: Map<number, number>,
  pageBytes: Map<number, number>,
  protectedPages: Set<number>,
  maxPages: number,
  maxBytes: number,
): number[] {
  let pages = access.size;
  let bytes = 0;
  for (const page of access.keys()) bytes += pageBytes.get(page) ?? 0;
  const candidates = [...access.entries()]
    .filter(([page]) => !protectedPages.has(page))
    .sort((a, b) => a[1] - b[1]);
  const evicted: number[] = [];
  const pageLimit = Math.max(0, maxPages);
  const byteLimit = Math.max(0, maxBytes);
  for (const [page] of candidates) {
    if (pages <= pageLimit && bytes <= byteLimit) break;
    evicted.push(page);
    pages--;
    bytes -= pageBytes.get(page) ?? 0;
  }
  return evicted;
}
