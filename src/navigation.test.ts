import { test } from "node:test";
import { strict as assert } from "node:assert";
import { currentChapter, JumpHistory, searchText } from "./navigation";
test("literal search keeps UTF16 offsets and HTML-looking text as plain text", () => {
  const text = "😀<script>人物</script>\n人物和人物";
  const first = searchText(text, "人物", 0, 2);
  assert.equal(first.hits.length, 2);
  for (const hit of first.hits)
    assert.equal(text.slice(hit.offset, hit.offset + hit.match.length), "人物");
  const next = searchText(text, "人物", first.next!, 2);
  assert.equal(next.hits.length, 1);
  assert.equal(next.next, null);
  assert.equal(searchText(text, "").hits.length, 0);
  assert.equal(searchText(text, "不存在").next, null);
});
test("chapters and bounded jump return history remain independent of layout", () => {
  const chapters = [
    { title: "甲", offset: 0, level: 0 },
    { title: "乙", offset: 50, level: 0 },
  ];
  assert.equal(currentChapter(chapters, 49), 0);
  assert.equal(currentChapter(chapters, 50), 1);
  const history = new JumpHistory();
  history.push(12);
  history.push(12);
  history.push(57);
  assert.equal(history.pop(), 57);
  assert.equal(history.pop(), 12);
  assert.equal(history.available, false);
  for (let i = 0; i < 60; i++) history.push(i);
  for (let i = 59; i >= 10; i--) assert.equal(history.pop(), i);
  assert.equal(history.pop(), undefined);
});
