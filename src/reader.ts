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
  return boundary(text, lo);
}
