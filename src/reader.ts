// Positions use UTF-16 offsets, independent of the current font or window size.
export function boundary(text: string, offset: number): number {
  const n = Math.max(0, Math.min(text.length, Math.floor(offset)));
  const code = text.charCodeAt(n);
  return code >= 0xdc00 && code <= 0xdfff ? n - 1 : n;
}
export function pageEnd(
  text: string,
  start: number,
  fits: (part: string) => boolean,
): number {
  if (start >= text.length) return text.length;
  let lo = start,
    hi = Math.min(text.length, start + 12000);
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (fits(text.slice(start, mid))) lo = mid;
    else hi = mid - 1;
  }
  const minimum = boundary(text, start + 1) === start ? start + 2 : start + 1;
  return Math.max(Math.min(text.length, minimum), boundary(text, lo));
}
export function previousStart(
  text: string,
  end: number,
  fits: (part: string) => boolean,
): number {
  let lo = Math.max(0, end - 12000),
    hi = end;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (fits(text.slice(mid, end))) hi = mid;
    else lo = mid + 1;
  }
  let start = boundary(text, lo);
  // Rounding away from a surrogate must not make the previous page overflow.
  if (start < lo && !fits(text.slice(start, end))) start += 2;
  if (start >= end && end > 0) start = boundary(text, end - 1);
  return start;
}

// Page boundaries belong to one layout. Only the original text offset is durable.
export class PageReader {
  offset: number;
  end: number;
  private key = "";
  private pages = new Map<number, number>();
  private previous = new Map<number, number>();

  constructor(
    readonly text: string,
    offset = 0,
  ) {
    this.offset = boundary(
      text,
      Math.min(offset, Math.max(0, text.length - 1)),
    );
    this.end = this.offset;
  }

  layout(key: string, fits: (part: string) => boolean) {
    if (key !== this.key) {
      this.key = key;
      this.pages.clear();
      this.previous.clear();
    }
    this.end =
      this.pages.get(this.offset) ?? pageEnd(this.text, this.offset, fits);
    this.remember(this.offset, this.end);
  }

  turn(direction: number, key: string, fits: (part: string) => boolean) {
    this.layout(key, fits);
    if (direction > 0) {
      if (this.end >= this.text.length) return false;
      this.offset = this.end;
    } else {
      if (this.offset === 0) return false;
      const end = this.offset;
      this.offset =
        this.previous.get(end) ?? previousStart(this.text, end, fits);
      // A backward page must end at the place we came from, even after a resize.
      this.remember(this.offset, end);
    }
    this.layout(key, fits);
    return true;
  }

  jump(last: boolean, key: string, fits: (part: string) => boolean) {
    this.layout(key, fits);
    this.offset = last ? previousStart(this.text, this.text.length, fits) : 0;
    this.layout(key, fits);
  }

  private remember(start: number, end: number) {
    const oldEnd = this.pages.get(start);
    if (
      oldEnd !== undefined &&
      oldEnd !== end &&
      this.previous.get(oldEnd) === start
    )
      this.previous.delete(oldEnd);
    if (!this.pages.has(start) && this.pages.size >= 128) {
      const oldest = this.pages.keys().next().value!;
      const oldEnd = this.pages.get(oldest)!;
      this.pages.delete(oldest);
      if (this.previous.get(oldEnd) === oldest) this.previous.delete(oldEnd);
    }
    this.pages.set(start, end);
    this.previous.set(end, start);
  }
}

export class WheelPager {
  private lastEvent = -Infinity;
  private direction = 0;
  private distance = 0;
  private turned = false;

  push(delta: number, mode: number, now: number, height: number): number {
    if (!Number.isFinite(delta) || delta === 0) return 0;
    const direction = Math.sign(delta);
    if (now - this.lastEvent >= 180 || direction !== this.direction) {
      this.distance = 0;
      this.turned = false;
    }
    this.lastEvent = now;
    this.direction = direction;
    this.distance +=
      Math.abs(delta) * (mode === 1 ? 16 : mode === 2 ? height : 1);
    if (this.turned || this.distance < 40) return 0;
    this.turned = true;
    return direction;
  }
}
