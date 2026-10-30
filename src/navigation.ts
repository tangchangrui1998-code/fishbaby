import { boundary } from "./reader";
export interface Chapter {
  title: string;
  offset: number;
  level: number;
}
export interface Bookmark {
  id: string;
  offset: number;
  label: string;
  anchor: string;
  createdAt: number;
}
export interface Navigation {
  bookId: string;
  chapters: Chapter[];
  bookmarks: Bookmark[];
}
export interface SearchHit {
  offset: number;
  before: string;
  match: string;
  after: string;
}
export function searchText(text: string, query: string, from = 0, limit = 100) {
  const hits: SearchHit[] = [];
  if (!query || query.length > 200)
    return { hits, next: null as number | null };
  let cursor = Math.max(0, from);
  while (hits.length < limit) {
    const offset = text.indexOf(query, cursor);
    if (offset < 0) return { hits, next: null as number | null };
    hits.push({
      offset,
      before: text.slice(boundary(text, Math.max(0, offset - 28)), offset),
      match: query,
      after: text.slice(
        offset + query.length,
        boundary(text, offset + query.length + 45),
      ),
    });
    cursor = offset + query.length;
  }
  return { hits, next: cursor < text.length ? cursor : null };
}
export function currentChapter(chapters: Chapter[], offset: number) {
  let low = 0,
    high = chapters.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (chapters[middle].offset <= offset) low = middle + 1;
    else high = middle;
  }
  return low - 1;
}
export class JumpHistory {
  private locations: number[] = [];
  get available() {
    return this.locations.length > 0;
  }
  push(offset: number) {
    if (this.locations.at(-1) !== offset) this.locations.push(offset);
    if (this.locations.length > 50) this.locations.shift();
  }
  pop() {
    return this.locations.pop();
  }
  clear() {
    this.locations = [];
  }
}
