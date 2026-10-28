import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { boundary, pageEnd, previousStart } from "./reader";
import "./style.css";
interface Book {
  id: string;
  title: string;
  path: string;
  offset: number;
  length: number;
  lastRead: number;
  available: boolean;
}
type Theme = "light" | "dark";
const themes = {
  light: { surface: "247,247,240", color: "#26332e", chrome: "#55715f" },
  dark: { surface: "28,33,32", color: "#e1e7df", chrome: "#afc2b5" },
};
interface Settings {
  theme: Theme;
  fontSize: number;
  lineHeight: number;
  color: string;
  background: number;
  singleLine: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
}
interface Library {
  books: Book[];
  activeId: string | null;
  settings: Settings;
}
const root = document.querySelector<HTMLDivElement>("#app")!;
const reader = new URLSearchParams(location.search).has("reader");
let data: Library = {
  books: [],
  activeId: null,
  settings: {
    theme: "light",
    fontSize: 18,
    lineHeight: 1.8,
    color: "#26332e",
    background: 0.92,
    singleLine: false,
    width: 420,
    height: 160,
    x: null,
    y: null,
  },
};
const esc = (s: string) =>
  s.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
function toast(message: unknown) {
  const el = document.querySelector<HTMLElement>("#notice");
  if (el) {
    el.textContent = String(message);
    el.hidden = false;
  }
}
async function action(fn: () => Promise<unknown>) {
  try {
    await fn();
  } catch (e) {
    toast(e);
  }
}
async function refresh() {
  data = await invoke<Library>("library");
  if (reader) {
    styleReader();
  } else {
    renderBooks();
    const theme = document.querySelector<HTMLSelectElement>("#theme");
    if (theme) theme.value = data.settings.theme;
    (document.querySelector("#color") as HTMLInputElement).value =
      data.settings.color;
    (document.querySelector("#background") as HTMLInputElement).value = String(
      data.settings.background,
    );
  }
}
function percent(b: Book) {
  return Math.min(100, Math.floor((b.offset / Math.max(1, b.length)) * 100));
}
function renderBooks() {
  const list = document.querySelector("#books")!;
  const books = [...data.books].sort((a, b) => b.lastRead - a.lastRead);
  document.querySelector("#count")!.textContent =
    `${books.length} 本书 · 保存在此设备`;
  list.innerHTML = books.length
    ? books
        .map(
          (b, i) =>
            `<article class="book ${data.activeId === b.id ? "active" : ""}"><div class="cover c${i % 4}"><span>TXT / ${String(i + 1).padStart(2, "0")}</span><strong>${esc(b.title)}</strong><i>隐 阅</i></div><div class="book-info"><h3>${esc(b.title)}</h3><p class="path" title="${esc(b.path)}">${esc(b.path)}</p><div class="progress"><span style="width:${percent(b)}%"></span></div><p>${data.activeId === b.id ? "正在阅读 · " : ""}${percent(b)}% ${b.available ? "" : " · 文件不可用"} <span>${b.lastRead ? new Date(b.lastRead * 1000).toLocaleDateString("zh-CN") : "尚未开始"}</span></p><button class="read primary" data-id="${b.id}">${b.offset ? "继续阅读" : "阅读"} ↗</button><div class="book-actions"><button data-rename="${b.id}">改名</button><button data-relocate="${b.id}">重新定位</button><button data-remove="${b.id}">移出</button></div></div></article>`,
        )
        .join("")
    : `<div class="empty"><div>◫</div><h2>下一段故事，从这里开始</h2><p>导入本地 TXT，每本书都会记住你读到的位置。</p><button id="empty-import" class="primary">＋ 导入第一本书</button></div>`;
  list.querySelectorAll<HTMLButtonElement>("[data-id]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          const result = await invoke<[Book, string, boolean]>("read_book", {
            id: b.dataset.id,
          });
          if (result[2])
            toast("文件内容发生变化，已尝试恢复原文位置，请确认。");
        })),
  );
  list.querySelectorAll<HTMLButtonElement>("[data-remove]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          if (
            confirm(
              "移出书架会删除这本书的阅读进度，原始 TXT 文件会保留。确定移出吗？",
            )
          )
            await invoke("edit_book", {
              id: b.dataset.remove,
              title: null,
              path: null,
              remove: true,
            });
        })),
  );
  list.querySelectorAll<HTMLButtonElement>("[data-rename]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          const book = data.books.find((x) => x.id === b.dataset.rename)!;
          const title = prompt("书名", book.title);
          if (title?.trim())
            await invoke("edit_book", {
              id: book.id,
              title,
              path: null,
              remove: false,
            });
        })),
  );
  list.querySelectorAll<HTMLButtonElement>("[data-relocate]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          const path = await open({
            filters: [{ name: "TXT 文本", extensions: ["txt"] }],
            multiple: false,
          });
          if (path)
            await invoke("edit_book", {
              id: b.dataset.relocate,
              path,
              title: null,
              remove: false,
            });
        })),
  );
  document
    .querySelector<HTMLButtonElement>("#empty-import")
    ?.addEventListener("click", importBooks);
}
async function importBooks() {
  await action(async () => {
    const paths = await open({
      multiple: true,
      filters: [{ name: "TXT 文本（UTF-8）", extensions: ["txt"] }],
    });
    if (!paths) return;
    const errors = await invoke<string[]>("import_books", {
      paths: Array.isArray(paths) ? paths : [paths],
    });
    if (errors.length) toast(errors.join("\n"));
  });
}
function renderShelf() {
  root.innerHTML = `<aside><a class="brand">◫ <strong>隐阅</strong><small>FISHBABY</small></a><div class="nav selected">▤ <span>我的书架</span></div><div class="side-bottom"><span class="dot"></span> 本地阅读，随时继续<p>⌘ ⇧ H　暂停 / 继续<br>⌘ ⇧ O　打开书架</p></div></aside><main><header><div><span class="eyebrow">A LITTLE TIME, A LITTLE STORY</span><h1>给自己，留一点阅读时间。</h1><p id="count"></p></div><button id="import" class="primary">＋ 导入 TXT</button></header><div class="toolbar"><h2>我的书架</h2><span>最近阅读优先</span><button id="adjust">调整阅读区</button><button id="pause">暂停</button></div><p id="notice" role="status" hidden></p><section id="books"></section><section class="settings"><div><h2>按你的习惯阅读</h2><p>选择一本书后，调整阅读区。移开鼠标，文字便会隐去。</p></div><div class="fields"><label>阅读主题 <select id="theme"><option value="light" ${data.settings.theme === "light" ? "selected" : ""}>浅色</option><option value="dark" ${data.settings.theme === "dark" ? "selected" : ""}>深色</option></select></label><label>字号 <input id="fontSize" type="range" min="12" max="40" value="${data.settings.fontSize}"></label><label>行距 <input id="lineHeight" type="range" min="1.2" max="2.5" step="0.1" value="${data.settings.lineHeight}"></label><label>文字颜色 <input id="color" type="color" value="${data.settings.color}"></label><label>底色浓度 <input id="background" type="range" min="0" max="1" step="0.05" value="${data.settings.background}"></label><label>宽度 <input id="width" type="number" min="220" max="1400" value="${data.settings.width}"></label><label>高度 <input id="height" type="number" min="90" max="800" value="${data.settings.height}"></label><label class="check"><input id="singleLine" type="checkbox" ${data.settings.singleLine ? "checked" : ""}> 单行阅读</label><button id="reset">阅读区居中</button></div></section><footer>只属于你的一小块阅读空间。<span>TXT · UTF-8 · 本地保存</span></footer></main>`;
  document.querySelector("#import")!.addEventListener("click", importBooks);
  for (const [id, mode] of [
    ["adjust", "setup"],
    ["pause", "paused"],
  ])
    document.querySelector(`#${id}`)!.addEventListener("click", () =>
      action(async () => {
        await invoke("set_mode", { mode });
        toast(
          mode === "hover"
            ? "悬停阅读已开启，将鼠标移入阅读区域即可显示。"
            : mode === "paused"
              ? "悬停阅读已暂停。"
              : "可拖动阅读区顶部来调整位置。",
        );
      }),
    );
  document
    .querySelector("#reset")!
    .addEventListener("click", () => action(() => invoke("reset_position")));
  document.querySelector(".fields")!.addEventListener("change", () =>
    action(async () => {
      const settings = { ...data.settings };
      for (const id of [
        "fontSize",
        "lineHeight",
        "background",
        "width",
        "height",
      ] as const)
        settings[id] = Number(
          (document.getElementById(id) as HTMLInputElement).value,
        );
      settings.color = (
        document.getElementById("color") as HTMLInputElement
      ).value;
      settings.singleLine = (
        document.getElementById("singleLine") as HTMLInputElement
      ).checked;
      const theme = (document.getElementById("theme") as HTMLSelectElement)
        .value as Theme;
      if (theme !== settings.theme) {
        settings.theme = theme;
        settings.color = themes[theme].color;
        settings.background = Math.max(0.85, settings.background);
      }
      await invoke("save_settings", { settings });
    }),
  );
  renderBooks();
}
let readerState = "hidden";
let text = "",
  book: Book | null = null,
  end = 0,
  lastWheel = 0;
function styleReader() {
  const el = document.querySelector<HTMLElement>("#text");
  if (!el) return;
  const s = data.settings;
  el.style.fontSize = `${s.fontSize}px`;
  el.style.lineHeight = String(s.lineHeight);
  el.style.color = s.color;
  const palette = themes[s.theme];
  const overlay = document.querySelector<HTMLElement>("#overlay")!;
  overlay.style.setProperty("--reader-surface", palette.surface);
  overlay.style.setProperty("--reader-chrome", palette.chrome);
  overlay.style.color = palette.chrome;
  overlay.style.background = `rgba(${palette.surface},${s.background})`;
  const toggle = document.querySelector<HTMLButtonElement>("#theme-toggle");
  if (toggle) {
    toggle.textContent = s.theme === "light" ? "深色" : "浅色";
    toggle.setAttribute("aria-label", `切换到${toggle.textContent}主题`);
  }
  el.classList.toggle("single", s.singleLine);
  paginate();
}
function paginate() {
  if (!book || !text) return;
  const el = document.querySelector<HTMLElement>("#text")!;
  book.offset = boundary(text, book.offset);
  end = pageEnd(text, book.offset, (s) => {
    el.textContent = s;
    return (
      el.scrollHeight <= el.clientHeight + 1 &&
      el.scrollWidth <= el.clientWidth + 1
    );
  });
  el.textContent = text.slice(book.offset, end);
  document.querySelector("#position")!.textContent =
    `${book.title} · ${percent({ ...book, length: text.length })}%`;
}
async function loadCurrent() {
  try {
    [book, text] = await invoke<[Book, string]>("current_text");
    styleReader();
  } catch {
    book = null;
    text = "";
    document.querySelector("#text")!.textContent = "请在书架中选择一本书";
  }
}
async function turn(direction: number) {
  if (!book || !text) return;
  const el = document.querySelector<HTMLElement>("#text")!;
  const offset =
    direction > 0
      ? end < text.length
        ? end
        : book.offset
      : previousStart(text, book.offset, (s) => {
          el.textContent = s;
          return (
            el.scrollHeight <= el.clientHeight + 1 &&
            el.scrollWidth <= el.clientWidth + 1
          );
        });
  book.offset = offset;
  paginate();
  await invoke("save_progress", {
    id: book.id,
    offset,
    anchor: text.slice(offset, boundary(text, offset + 120)),
  });
}
function renderReader() {
  document.documentElement.classList.add("reader-root");
  document.body.classList.add("reader");
  root.innerHTML =
    '<section id="overlay" class="hidden"><div class="drag"><span id="position"></span><button id="theme-toggle" aria-label="切换主题">深色</button><button id="start">开始阅读</button><button id="back">返回书架</button></div><div id="text"></div><p id="notice" hidden></p></section>';
  document.querySelector("#theme-toggle")!.addEventListener("click", () =>
    action(async () => {
      const latest = await invoke<Library>("library");
      const theme: Theme = latest.settings.theme === "light" ? "dark" : "light";
      await invoke("save_settings", {
        settings: {
          ...latest.settings,
          theme,
          color: themes[theme].color,
          background: Math.max(0.85, latest.settings.background),
        },
      });
    }),
  );
  document
    .querySelector("#back")!
    .addEventListener("click", () => action(() => invoke("return_to_shelf")));
  document
    .querySelector("#start")!
    .addEventListener("click", () =>
      action(() =>
        invoke("set_mode", {
          mode: readerState === "reading" ? "ready" : "hover",
        }),
      ),
    );
  document.querySelector(".drag")!.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  });
  document.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      if (Math.abs(e.deltaY) < 2 || Date.now() - lastWheel < 180) return;
      lastWheel = Date.now();
      void action(() => turn(Math.sign(e.deltaY)));
    },
    { passive: false },
  );
  new ResizeObserver(paginate).observe(document.querySelector("#text")!);
}
async function boot() {
  if (!isTauri()) {
    root.innerHTML =
      '<div class="browser-note"><h1>隐阅</h1><p>这是一个 macOS 桌面应用。请在项目目录执行 <code>npm run desktop</code>。</p></div>';
    return;
  }
  if (reader) renderReader();
  data = await invoke<Library>("library");
  if (reader) {
    await listen<string>("reader-state", (e) => {
      const el = document.querySelector("#overlay")!;
      el.className = e.payload;
      readerState = e.payload;
      document.querySelector("#start")!.textContent =
        e.payload === "reading" ? "暂停阅读" : "开始阅读";
      paginate();
    });
    await listen("book-changed", loadCurrent);
    await loadCurrent();
  } else renderShelf();
  await listen("library-changed", () => void refresh());
}
void boot().catch(toast);
