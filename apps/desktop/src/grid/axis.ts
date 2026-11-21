// Row/column geometry with a default size, custom sizes and hidden entries.
// Positions are computed with a sorted list of exceptions, so a sheet with a million
// rows and a few custom heights costs almost nothing.

export class Axis {
  private keys: number[] = [];
  private prefix: number[] = [0]; // prefix[k] = sum of (size - default) for keys[0..k)
  private sizes = new Map<number, number>();

  constructor(
    public count: number,
    public readonly defaultSize: number,
    custom: Iterable<[number, number]> = [],
    hidden: Iterable<number> = [],
  ) {
    for (const [i, s] of custom) this.sizes.set(i, s);
    for (const i of hidden) this.sizes.set(i, 0);
    this.rebuild();
  }

  private rebuild() {
    this.keys = [...this.sizes.keys()].sort((a, b) => a - b);
    this.prefix = [0];
    for (const k of this.keys) {
      this.prefix.push(this.prefix[this.prefix.length - 1] + (this.sizes.get(k)! - this.defaultSize));
    }
  }

  set(i: number, size: number) {
    this.sizes.set(i, size);
    this.rebuild();
  }

  size(i: number): number {
    return this.sizes.get(i) ?? this.defaultSize;
  }

  hidden(i: number): boolean {
    return this.sizes.get(i) === 0;
  }

  /** Number of exception keys strictly below i. */
  private below(i: number): number {
    let lo = 0;
    let hi = this.keys.length;
    while (lo < hi) {
      const m = (lo + hi) >> 1;
      if (this.keys[m] < i) lo = m + 1;
      else hi = m;
    }
    return lo;
  }

  /** Offset of the start of index i (1-based) from the start of index 1. */
  offset(i: number): number {
    return (i - 1) * this.defaultSize + this.prefix[this.below(i)];
  }

  total(): number {
    return this.offset(this.count + 1);
  }

  /** The index whose span contains offset px (clamped to [1, count]). */
  indexAt(px: number): number {
    if (px <= 0) return 1;
    let lo = 1;
    let hi = this.count;
    while (lo < hi) {
      const m = (lo + hi + 1) >> 1;
      if (this.offset(m) <= px) lo = m;
      else hi = m - 1;
    }
    // Skip hidden entries forward.
    while (lo < this.count && this.size(lo) === 0) lo++;
    return lo;
  }
}
