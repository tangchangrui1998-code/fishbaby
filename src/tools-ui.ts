import { invoke } from "@tauri-apps/api/core";
import {
  currentChapter,
  searchText,
  JumpHistory,
  type Navigation,
  type SearchHit,
} from "./navigation";
const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
interface Hooks {
  book(): { id: string; title: string; length: number } | null;
  text(): string;
  offset(): number;
  go(offset: number): Promise<void>;
  notify(error: unknown): void;
}
export function readerTools(hooks: Hooks) {
  const history = new JumpHistory();
  let navigation: Navigation = { bookId: "", chapters: [], bookmarks: [] };
  let tab = "chapters",
    generation = 0,
    searchId = 0,
    next: number | null = null;
  let hits: SearchHit[] = [],
    query = "";
  let worker: Worker | null = null;
  let pending: {
    id: number;
    query: string;
    from: number;
    bookId: string;
  } | null = null;
  try {
    worker = new Worker(new URL("./search.worker.ts", import.meta.url), {
      type: "module",
    });
  } catch {
    /* Some desktop WebViews cannot start workers from a custom scheme. */
  }
  const drawer = document.createElement("section");
  drawer.id = "reader-tools";
  drawer.hidden = true;
  drawer.setAttribute("aria-label", "阅读工具");
  drawer.innerHTML = `<div class="tool-bar"><nav aria-label="阅读工具分类"><button data-tab="chapters">目录</button><button data-tab="bookmarks">书签</button><button data-tab="search">搜索</button></nav><button id="close-tools" aria-label="关闭阅读工具">关闭</button></div><div id="tool-controls"></div><div id="tool-results" aria-live="polite"></div>`;
  document.querySelector("#overlay")!.append(drawer);
  const controls = drawer.querySelector<HTMLElement>("#tool-controls")!;
  const results = drawer.querySelector<HTMLElement>("#tool-results")!;
  const back = document.createElement("button");
  back.id = "return-location";
  back.textContent = "返回原处";
  back.hidden = true;
  document.querySelector(".page-controls")!.append(back);
  const attempt = (action: () => Promise<unknown>) =>
    void action().catch(hooks.notify);
  const update = () => {
    back.hidden = !history.available;
    const index = currentChapter(navigation.chapters, hooks.offset());
    const chapter = navigation.chapters[index];
    const end = navigation.chapters[index + 1]?.offset ?? hooks.text().length;
    const progress = chapter
      ? Math.min(
          100,
          Math.floor(
            ((hooks.offset() - chapter.offset) /
              Math.max(1, end - chapter.offset)) *
              100,
          ),
        )
      : 0;
    const output = document.querySelector<HTMLElement>("#chapter-position");
    if (output) {
      output.textContent = chapter ? `${chapter.title} · ${progress}%` : "";
      output.title = output.textContent;
    }
  };
  const go = async (offset: number) => {
    if (offset !== hooks.offset()) history.push(hooks.offset());
    await hooks.go(offset);
    close();
    update();
  };
  back.onclick = () =>
    attempt(async () => {
      const offset = history.pop();
      if (offset !== undefined) await hooks.go(offset);
      update();
    });
  async function refresh() {
    const book = hooks.book();
    const request = ++generation;
    if (!book) return;
    const response = await invoke<Navigation>("reading_navigation");
    if (
      request !== generation ||
      book.id !== hooks.book()?.id ||
      response.bookId !== book.id
    )
      return;
    navigation = response;
    update();
    if (!drawer.hidden) render();
  }
  function renderSearchResults() {
    results.innerHTML =
      hits
        .map(
          (hit) =>
            `<button class="result" data-offset="${hit.offset}"><span>${escape(hit.before)}<mark>${escape(hit.match)}</mark>${escape(hit.after)}</span></button>`,
        )
        .join("") +
      (next !== null
        ? '<button id="more-results">加载更多结果</button>'
        : hits.length
          ? '<p class="tool-hint">已显示全部匹配</p>'
          : `<p class="tool-hint">${query ? "没有找到匹配内容" : "输入关键词，按回车搜索"}</p>`);
    results
      .querySelector("#more-results")
      ?.addEventListener("click", () => search(true));
  }
  function search(more = false) {
    const input = drawer.querySelector<HTMLInputElement>("#book-query")!;
    const value = input.value.trim();
    if (!value || value.length > 200) {
      hooks.notify("请输入 1–200 个字符");
      return;
    }
    if (!more || value !== query) {
      hits = [];
      next = null;
    }
    query = value;
    ++searchId;
    results.setAttribute("aria-busy", "true");
    pending = {
      id: searchId,
      query,
      from: more ? (next ?? 0) : 0,
      bookId: hooks.book()?.id ?? "",
    };
    if (worker) worker.postMessage({ type: "search", ...pending });
    else fallbackSearch();
  }
  function acceptResult(result: {
    id: number;
    bookId: string;
    hits: SearchHit[];
    next: number | null;
  }) {
    if (result.id !== searchId || result.bookId !== hooks.book()?.id) return;
    hits.push(...result.hits);
    next = result.next;
    pending = null;
    results.removeAttribute("aria-busy");
    if (tab === "search") renderSearchResults();
  }
  function fallbackSearch() {
    const request = pending;
    if (!request) return;
    setTimeout(() => {
      if (request.id !== searchId || request.bookId !== hooks.book()?.id)
        return;
      acceptResult({
        ...request,
        ...searchText(hooks.text(), request.query, request.from),
      });
    }, 0);
  }
  if (worker) {
    worker.onmessage = (event) => acceptResult(event.data);
    worker.onerror = (event) => {
      event.preventDefault();
      worker?.terminate();
      worker = null;
      fallbackSearch();
    };
  }
  function render() {
    drawer
      .querySelectorAll<HTMLButtonElement>("[data-tab]")
      .forEach((button) => {
        button.setAttribute("aria-pressed", String(button.dataset.tab === tab));
      });
    if (tab === "search") {
      controls.innerHTML = `<form id="search-book"><input id="book-query" aria-label="书内搜索" maxlength="200" placeholder="人物、地名或句子" value="${escape(query)}"><button>搜索</button></form>`;
      controls.querySelector("form")!.onsubmit = (event) => {
        event.preventDefault();
        search();
      };
      renderSearchResults();
      controls.querySelector("input")!.focus();
    } else if (tab === "bookmarks") {
      controls.innerHTML =
        '<form id="add-bookmark"><input id="bookmark-label" maxlength="80" aria-label="书签名称" placeholder="书签名称（可选）"><button>保存当前位置</button></form>';
      controls.querySelector("form")!.onsubmit = (event) => {
        event.preventDefault();
        attempt(async () => {
          const book = hooks.book();
          if (!book) return;
          await invoke("save_bookmark", {
            bookId: book.id,
            offset: hooks.offset(),
            label: controls.querySelector<HTMLInputElement>("input")!.value,
          });
          await refresh();
        });
      };
      results.innerHTML = navigation.bookmarks.length
        ? [...navigation.bookmarks]
            .sort((a, b) => a.offset - b.offset)
            .map(
              (mark) =>
                `<div class="bookmark-row"><button class="result" data-offset="${mark.offset}">${escape(mark.label)}<small>${Math.floor((mark.offset / Math.max(1, hooks.text().length)) * 100)}%</small></button><button data-delete="${escape(mark.id)}" aria-label="删除书签 ${escape(mark.label)}">删除</button></div>`,
            )
            .join("")
        : '<p class="tool-hint">还没有书签，保存一处以后想回看的位置。</p>';
    } else {
      controls.innerHTML =
        '<p class="tool-hint">选择章节跳转；底部“返回原处”可找回跳转前的位置。</p>';
      const current = currentChapter(navigation.chapters, hooks.offset());
      results.innerHTML = navigation.chapters.length
        ? navigation.chapters
            .map(
              (chapter, index) =>
                `<button class="result" style="padding-left:${8 + chapter.level * 12}px" data-offset="${chapter.offset}" ${current === index ? 'aria-current="location"' : ""}>${escape(chapter.title)}</button>`,
            )
            .join("")
        : '<p class="tool-hint">未识别到目录，仍可使用书内搜索或书签。</p>';
      results
        .querySelector('[aria-current="location"]')
        ?.scrollIntoView({ block: "nearest" });
    }
  }
  results.onclick = (event) => {
    const target = event.target as HTMLElement;
    const jump = target.closest<HTMLElement>("[data-offset]");
    if (jump) attempt(() => go(Number(jump.dataset.offset)));
    const remove = target.closest<HTMLElement>("[data-delete]");
    if (remove)
      attempt(async () => {
        await invoke("remove_bookmark", {
          bookId: hooks.book()?.id,
          id: remove.dataset.delete,
        });
        await refresh();
      });
  };
  function close() {
    drawer.hidden = true;
    document
      .querySelector<HTMLElement>("#overlay")!
      .focus({ preventScroll: true });
  }
  drawer.querySelector<HTMLButtonElement>("#close-tools")!.onclick = close;
  drawer.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach(
    (button) =>
      (button.onclick = () => {
        tab = button.dataset.tab!;
        render();
      }),
  );
  drawer.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      close();
    }
  });
  return {
    get isOpen() {
      return !drawer.hidden;
    },
    update,
    async open(which = "chapters") {
      if (!hooks.book()) return;
      await invoke("set_mode", { mode: "ready" });
      tab = which;
      drawer.hidden = false;
      results.textContent = "加载中…";
      try {
        await refresh();
      } catch (error) {
        hooks.notify(error);
        results.textContent = "无法加载阅读工具，请关闭后重试。";
      }
    },
    async reset() {
      history.clear();
      navigation = { bookId: "", chapters: [], bookmarks: [] };
      query = "";
      hits = [];
      next = null;
      ++searchId;
      close();
      pending = null;
      worker?.postMessage({
        type: "book",
        bookId: hooks.book()?.id ?? "",
        text: hooks.text(),
      });
      update();
      await refresh();
    },
  };
}
