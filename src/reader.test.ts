import { test } from "node:test";
import assert from "node:assert/strict";
import {
  boundary,
  pageEnd,
  previousStart,
  PageReader,
  WheelPager,
} from "./reader.ts";
test("page positions preserve original text through layout changes", () => {
  const text = "一二三四五六七八九十";
  const end = pageEnd(text, 0, (s) => s.length <= 4);
  assert.equal(end, 4);
  assert.equal(
    pageEnd(text, end, (s) => s.length <= 2),
    6,
  );
  assert.equal(
    previousStart(text, end, (s) => s.length <= 4),
    0,
  );
});

test("forward pages cover the original exactly and return to the same previous pages", () => {
  const text = "甲😀乙\n第二段落。\n".repeat(35);
  const reader = new PageReader(text);
  const fits = (s: string) => s.length <= 17;
  const starts: number[] = [];
  let result = "";
  reader.layout("wide", fits);
  do {
    starts.push(reader.offset);
    const page = text.slice(reader.offset, reader.end);
    assert(!/^[\uDC00-\uDFFF]|[\uD800-\uDBFF]$/.test(page));
    result += page;
  } while (reader.turn(1, "wide", fits));
  assert.equal(result, text);
  for (const start of starts.slice(0, -1).reverse()) {
    assert(reader.turn(-1, "wide", fits));
    assert.equal(reader.offset, start);
  }
  assert.equal(reader.turn(-1, "wide", fits), false);
});

test("resizing near the end preserves the first visible character instead of filling the last page", () => {
  const text = "一二三四五六七八九十";
  const reader = new PageReader(text, 8);
  reader.layout("small", (s) => s.length <= 2);
  assert.equal(reader.end, 10);
  reader.layout("large", (s) => s.length <= 6);
  assert.equal(reader.offset, 8);
  assert.equal(text.slice(reader.offset, reader.end), "九十");
  assert(reader.turn(-1, "large", (s) => s.length <= 6));
  assert.equal(reader.offset, 2);
  assert.equal(reader.end, 8);
  reader.turn(1, "large", (s) => s.length <= 6);
  assert.equal(reader.offset, 8);
  const reopened = new PageReader(text, reader.offset);
  reopened.layout("new size", (s) => s.length <= 3);
  assert.equal(reopened.offset, 8);
});

test("layout changes discard stale page boundaries but theme-only updates reuse them", () => {
  const reader = new PageReader("0123456789ABCDEFGHIJ");
  const four = (s: string) => s.length <= 4;
  reader.layout("four", four);
  reader.turn(1, "four", four);
  reader.turn(1, "four", four);
  assert.equal(reader.offset, 8);
  reader.layout("six", (s) => s.length <= 6);
  reader.turn(-1, "six", (s) => s.length <= 6);
  assert.equal(reader.offset, 2);
  assert.equal(reader.end, 8);
  reader.layout("six", () => {
    throw new Error("layout should be cached");
  });
  assert.equal(reader.end, 8);
});

test("very small pages and uncached backward pages always advance without splitting emoji", () => {
  const text = "甲😀乙😀丙";
  const reader = new PageReader(text, 3);
  reader.layout("tiny", (s) => s.length <= 1);
  reader.turn(-1, "tiny", (s) => s.length <= 1);
  assert.equal(reader.offset, 1);
  assert.equal(reader.end, 3);
  reader.turn(1, "tiny", (s) => s.length <= 1);
  assert.equal(reader.offset, 3);
  const empty = new PageReader("");
  empty.layout("empty", () => true);
  assert.equal(
    empty.turn(1, "empty", () => true),
    false,
  );
  assert.equal(
    empty.turn(-1, "empty", () => true),
    false,
  );
});

test("long documents are paginated locally, not measured in full", () => {
  const reader = new PageReader("一".repeat(1_000_000), 900_000);
  let calls = 0;
  reader.layout("layout", (part) => {
    assert(part.length <= 12000);
    calls++;
    return part.length <= 100;
  });
  assert.equal(reader.end, 900_100);
  assert(calls < 20);
  // Exceed the cache capacity and verify the fallback can still return.
  for (let i = 0; i < 140; i++)
    reader.turn(1, "layout", (s) => s.length <= 100);
  for (let i = 0; i < 140; i++)
    reader.turn(-1, "layout", (s) => s.length <= 100);
  assert.equal(reader.offset, 900_000);
});

test("one trackpad gesture including long momentum produces one page turn", () => {
  const wheel = new WheelPager();
  const turns = [];
  for (let i = 0; i < 70; i++) {
    const direction = wheel.push(i < 10 ? 5 : 2, 0, i * 16, 200);
    if (direction) turns.push(direction);
  }
  assert.deepEqual(turns, [1]);
  assert.equal(wheel.push(60, 0, 1400, 200), 1);
  assert.equal(wheel.push(-60, 0, 1416, 200), -1);
});

test("wheel line and page units work and accidental small motions do not flip pages", () => {
  const wheel = new WheelPager();
  assert.equal(wheel.push(1, 0, 0, 200), 0);
  assert.equal(wheel.push(-1, 0, 16, 200), 0);
  assert.equal(wheel.push(3, 1, 300, 200), 1);
  assert.equal(wheel.push(1, 2, 600, 200), 1);
  assert.equal(wheel.push(NaN, 0, 900, 200), 0);
});
test("consecutive mouse wheel ticks each turn a page without a gesture idle gap", () => {
  const wheel = new WheelPager();
  for (const now of [0, 30, 60, 90]) {
    assert.equal(wheel.push(1, 1, now, 200, true), 1);
  }
  for (const now of [120, 150, 180]) {
    assert.equal(wheel.push(3, 0, now, 200, true), 1);
  }
  assert.equal(wheel.push(-3, 0, 210, 200, true), -1);
});
test("mouse mode handles small pixel ticks without relying on browser-specific delta values", () => {
  const wheel = new WheelPager();
  for (const [index, delta] of [0.5, 1, 3, 10, 100, 120].entries())
    assert.equal(wheel.push(delta, 0, index * 15, 200, true), 1);
});
test("page boundaries do not split emoji surrogate pairs", () => {
  const text = "甲😀乙";
  assert.equal(boundary(text, 2), 1);
  assert.equal(
    pageEnd(text, 0, (s) => s.length <= 2),
    1,
  );
  assert.equal(
    pageEnd(text, 1, (s) => s.length <= 1),
    3,
  );
});
