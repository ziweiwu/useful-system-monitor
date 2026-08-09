/**
 * Fixed-capacity ring buffer. All history uses this so memory never grows
 * with uptime. See I-10.
 */
export class Ring {
  private readonly buf: number[];
  private len = 0;
  private head = 0;

  constructor(readonly capacity: number) {
    if (capacity <= 0) throw new RangeError('capacity must be > 0');
    this.buf = Array.from({ length: capacity }, () => 0);
  }

  push(v: number): void {
    this.buf[this.head] = v;
    this.head = (this.head + 1) % this.capacity;
    if (this.len < this.capacity) this.len++;
  }

  get size(): number {
    return this.len;
  }

  /** Oldest-to-newest. */
  toArray(): number[] {
    const out: number[] = [];
    const start = (this.head - this.len + this.capacity) % this.capacity;
    for (let i = 0; i < this.len; i++) out.push(this.buf[(start + i) % this.capacity]!);
    return out;
  }

  last(): number | null {
    return this.len === 0 ? null : this.buf[(this.head - 1 + this.capacity) % this.capacity]!;
  }
}
