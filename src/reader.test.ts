import { test } from "node:test";
import assert from "node:assert/strict";
import { boundary, pageEnd, previousStart } from "./reader.ts";
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
