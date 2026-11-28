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
