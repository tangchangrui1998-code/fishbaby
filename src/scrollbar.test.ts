import { test } from "node:test";
import assert from "node:assert/strict";
import { scrollGeometry, scrollOffset } from "./scrollbar.ts";
test("whole-book scrollbar maps its ends to first and last readable page", () => {
  assert.deepEqual(scrollGeometry(1000, 0, 100, 900, 200), {
    height: 24,
    top: 0,
  });
  assert.deepEqual(scrollGeometry(1000, 900, 100, 900, 200), {
    height: 24,
    top: 176,
  });
  assert.equal(scrollOffset("0123456789", 1, 7), 7);
  assert.equal(scrollOffset("0123456789", -1, 7), 0);
});
test("scrollbar seeks preserve emoji and small documents fill the track", () => {
  assert.equal(scrollOffset("甲😀乙丙", 0.5, 4), 1);
  assert.deepEqual(scrollGeometry(10, 0, 10, 0, 50), { height: 50, top: 0 });
  assert.deepEqual(scrollGeometry(0, 0, 0, 0, 0), { height: 0, top: 0 });
});
