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

/** Approximate retained payload size; this is not a JavaScript heap measurement. */
export function estimateSerializedBytes(value: unknown): number {
  try {
    return JSON.stringify(value).length * 2;
  } catch {
    return 0;
  }
}
