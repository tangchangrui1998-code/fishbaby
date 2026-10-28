import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { boundary, pageEnd, previousStart } from "./reader";
import { scrollGeometry, scrollOffset } from "./scrollbar";
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
    for (const id of ["width", "height"] as const)
      (document.getElementById(id) as HTMLInputElement).value = String(
        data.settings[id],
      );
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
  document.querySelector("#count")!.textContent = `${books.length} 本`;
  list.innerHTML = books.length
    ? books
        .map((b) => {
          const tone =
            Array.from(b.title).reduce((n, c) => n + c.codePointAt(0)!, 0) % 4;
          return `<article class="book"><button class="cover c${tone}" data-id="${b.id}" aria-label="阅读 ${esc(b.title)}"><strong>${esc(b.title)}</strong><span>TXT</span></button><div class="book-info"><h3 title="${esc(b.title)}">${esc(b.title)}</h3><div class="book-meta"><span>${b.available ? `${percent(b)}% · ${b.lastRead ? new Date(b.lastRead * 1000).toLocaleDateString("zh-CN") : "未读"}` : "文件不可用"}</span>${data.activeId === b.id ? '<span class="current">当前阅读</span>' : ""}</div><div class="progress" aria-hidden="true"><span style="width:${percent(b)}%"></span></div><div class="book-actions"><button class="read" data-id="${b.id}">${b.offset ? "继续阅读" : "阅读"}</button><button class="remove" data-remove="${b.id}" aria-label="将 ${esc(b.title)} 移出书架">移出</button></div></div></article>`;
        })
        .join("")
    : `<div class="empty"><h2>书架里还没有书</h2><p>导入本地 TXT 文件，开始阅读。</p><button id="empty-import" class="primary">导入文本</button></div>`;
  list.querySelectorAll<HTMLButtonElement>("[data-id]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          const result = await invoke<[Book, string, boolean]>("read_book", {
            id: b.dataset.id,
          });
          if (result[2]) toast("文件内容有变化，请确认恢复的阅读位置。");
        })),
  );
  list.querySelectorAll<HTMLButtonElement>("[data-remove]").forEach(
    (b) =>
      (b.onclick = () =>
        action(async () => {
          if (
            confirm(
              "移出后会清除这本书的阅读进度，原始文件会保留。确定移出吗？",
            )
          )
            await invoke("remove_book", { id: b.dataset.remove });
        })),
  );
  document
    .querySelector("#empty-import")
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
  root.innerHTML = `<aside><div class="brand">隐阅<span>fishbaby</span></div><nav aria-label="主导航"><button class="selected" data-view="library" aria-current="page">书架</button><button data-view="preferences">阅读设置</button></nav><div class="side-bottom"><p>快捷键</p><dl><dt>暂停 / 继续</dt><dd>⌘ ⇧ H</dd><dt>打开书架</dt><dd>⌘ ⇧ O</dd></dl></div></aside><main><section id="library-view"><header><div class="heading"><h1>书架</h1><span id="count"></span></div><button id="import" class="primary">导入文本</button></header><div class="toolbar"><span>最近阅读</span><div><button id="adjust">调整阅读区</button><button id="pause">暂停阅读</button></div></div><section id="books" aria-label="书籍"></section></section><section id="preferences-view" hidden><header><h1>阅读设置</h1></header><div class="fields"><section class="setting-group"><h2>外观</h2><label>主题<select id="theme"><option value="light" ${data.settings.theme === "light" ? "selected" : ""}>浅色</option><option value="dark" ${data.settings.theme === "dark" ? "selected" : ""}>深色</option></select></label><label>文字颜色<input id="color" type="color" value="${data.settings.color}"></label><label>背景不透明度<input id="background" type="range" min="0" max="1" step="0.05" value="${data.settings.background}"></label></section><section class="setting-group"><h2>排版</h2><label>字号<span><output id="fontSize-value">${data.settings.fontSize}</output><input id="fontSize" type="range" min="12" max="40" value="${data.settings.fontSize}"></span></label><label>行距<span><output id="lineHeight-value">${data.settings.lineHeight}</output><input id="lineHeight" type="range" min="1.2" max="2.5" step="0.1" value="${data.settings.lineHeight}"></span></label><label>单行阅读<input id="singleLine" type="checkbox" ${data.settings.singleLine ? "checked" : ""}></label></section><section class="setting-group"><h2>阅读窗口</h2><label>宽度<span><input id="width" type="number" min="220" max="1400" value="${data.settings.width}"> px</span></label><label>高度<span><input id="height" type="number" min="90" max="800" value="${data.settings.height}"> px</span></label><div class="setting-action"><span>拖动窗口顶部可调整位置</span><button id="reset">恢复居中</button></div></section></div></section><p id="notice" role="status" hidden></p></main>`;
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach(
    (button) =>
      (button.onclick = () => {
        const library = button.dataset.view === "library";
        (document.querySelector("#library-view") as HTMLElement).hidden =
          !library;
        (document.querySelector("#preferences-view") as HTMLElement).hidden =
          library;
        document.querySelectorAll("[data-view]").forEach((item) => {
          item.classList.toggle("selected", item === button);
          if (item === button) item.setAttribute("aria-current", "page");
          else item.removeAttribute("aria-current");
        });
      }),
  );
  document.querySelector(".fields")!.addEventListener("input", (event) => {
    const input = event.target as HTMLInputElement;
    const output = document.getElementById(`${input.id}-value`);
    if (output) output.textContent = input.value;
  });
  document.querySelector("#import")!.addEventListener("click", importBooks);
  for (const [id, mode] of [
    ["adjust", "setup"],
    ["pause", "paused"],
  ])
    document.querySelector(`#${id}`)!.addEventListener("click", () =>
      action(async () => {
        await invoke("set_mode", { mode });
        toast(mode === "paused" ? "已暂停阅读" : "拖动阅读窗口顶部可调整位置");
      }),
    );
  document
    .querySelector("#reset")!
    .addEventListener("click", () => action(() => invoke("reset_position")));
  document.querySelector(".fields")!.addEventListener("change", (event) =>
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
      await invoke("save_settings", {
        settings,
        resize: ["width", "height"].includes((event.target as HTMLElement).id),
      });
    }),
  );
  renderBooks();
}
let readerState = "hidden";
let text = "",
  book: Book | null = null,
  end = 0,
  lastWheel = 0;
let lastPageStart = 0,
  layoutKey = "",
  scrollDrag: { id: number; grab: number } | null = null,
  scrollFrame = 0,
  saveTimer: ReturnType<typeof setTimeout> | undefined;
function fitsText(part: string) {
  const el = document.querySelector<HTMLElement>("#text")!;
  el.textContent = part;
  return (
    el.scrollHeight <= el.clientHeight + 1 &&
    el.scrollWidth <= el.clientWidth + 1
  );
}
function syncScrollbar() {
  const track = document.querySelector<HTMLElement>("#reader-scrollbar")!;
  const thumb = document.querySelector<HTMLElement>("#scroll-thumb")!;
  const geometry = scrollGeometry(
    text.length,
    book?.offset ?? 0,
    end - (book?.offset ?? 0),
    lastPageStart,
    track.clientHeight,
  );
  thumb.style.height = `${geometry.height}px`;
  thumb.style.transform = `translateY(${geometry.top}px)`;
  track.setAttribute("aria-valuemax", String(lastPageStart));
  track.setAttribute("aria-valuenow", String(book?.offset ?? 0));
  track.setAttribute(
    "aria-valuetext",
    `${book ? percent({ ...book, length: text.length }) : 0}%`,
  );
  track.setAttribute("aria-disabled", String(lastPageStart === 0));
}
async function persistPosition() {
  if (!book) return;
  const { id, offset } = book;
  await invoke("save_progress", {
    id,
    offset,
    anchor: text.slice(offset, boundary(text, offset + 120)),
  });
}
function seek(ratio: number) {
  if (!book) return;
  book.offset = scrollOffset(text, ratio, lastPageStart);
  paginate();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void action(persistPosition), 180);
}

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
  const key = [
    book.id,
    text.length,
    el.clientWidth,
    el.clientHeight,
    data.settings.fontSize,
    data.settings.lineHeight,
    data.settings.singleLine,
  ].join(":");
  if (key !== layoutKey) {
    lastPageStart = previousStart(
      text,
      text.trimEnd().length || text.length,
      fitsText,
    );
    layoutKey = key;
  }
  book.offset = boundary(text, Math.min(book.offset, lastPageStart));
  end = pageEnd(text, book.offset, fitsText);
  el.textContent = text.slice(book.offset, end);
  document.querySelector("#position")!.textContent =
    `${book.title} · ${percent({ ...book, length: text.length })}%`;
  syncScrollbar();
}
async function loadCurrent() {
  try {
    clearTimeout(saveTimer);
    [book, text] = await invoke<[Book, string]>("current_text");
    layoutKey = "";
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
  clearTimeout(saveTimer);
  await persistPosition();
}
function renderReader() {
  document.documentElement.classList.add("reader-root");
  document.body.classList.add("reader");
  root.innerHTML =
    '<section id="overlay" class="hidden"><div class="drag"><span id="position"></span><button id="theme-toggle" aria-label="切换主题">深色</button><button id="start">开始阅读</button><button id="back">返回书架</button></div><div class="reader-content"><div id="text"></div><div id="reader-scrollbar" role="scrollbar" tabindex="0" aria-label="阅读进度" aria-orientation="vertical" aria-controls="text" aria-valuemin="0"><div id="scroll-thumb"></div></div></div><p id="notice" hidden></p></section>';
  document.querySelector("#theme-toggle")!.addEventListener("click", () =>
    action(async () => {
      const latest = await invoke<Library>("library");
      const theme: Theme = latest.settings.theme === "light" ? "dark" : "light";
      await invoke("save_settings", {
        resize: false,
        settings: {
          ...latest.settings,
          theme,
          color: themes[theme].color,
          background: Math.max(0.85, latest.settings.background),
        },
      });
    }),
  );
  document.querySelector("#back")!.addEventListener("click", () =>
    action(async () => {
      clearTimeout(saveTimer);
      await persistPosition();
      await invoke("return_to_shelf");
    }),
  );
  document
    .querySelector("#start")!
    .addEventListener("click", () =>
      action(() =>
        readerState === "reading"
          ? invoke("set_mode", { mode: "ready" })
          : invoke("start_reading"),
      ),
    );
  const overlay = document.querySelector("#overlay")!;
  overlay.addEventListener(
    "pointerenter",
    () => void action(() => invoke("reader_pointer", { inside: true })),
  );
  overlay.addEventListener("pointerleave", () => {
    if (!scrollDrag)
      void action(() => invoke("reader_pointer", { inside: false }));
  });
  const track = document.querySelector<HTMLElement>("#reader-scrollbar")!;
  const thumb = document.querySelector<HTMLElement>("#scroll-thumb")!;
  const dragSeek = (event: PointerEvent) => {
    const rect = track.getBoundingClientRect();
    const travel = Math.max(1, rect.height - thumb.offsetHeight);
    seek(
      (event.clientY -
        rect.top -
        (scrollDrag?.grab ?? thumb.offsetHeight / 2)) /
        travel,
    );
  };
  track.addEventListener("pointerdown", (event) => {
    if (!book || lastPageStart === 0) return;
    event.preventDefault();
    clearTimeout(saveTimer);
    scrollDrag = {
      id: event.pointerId,
      grab:
        event.target === thumb
          ? event.clientY - thumb.getBoundingClientRect().top
          : thumb.offsetHeight / 2,
    };
    track.setPointerCapture(event.pointerId);
    void invoke("reader_pointer", { inside: true });
    dragSeek(event);
  });
  track.addEventListener("pointermove", (event) => {
    if (!scrollDrag || scrollDrag.id !== event.pointerId) return;
    cancelAnimationFrame(scrollFrame);
    scrollFrame = requestAnimationFrame(() => dragSeek(event));
  });
  const finishDrag = (event: PointerEvent) => {
    if (!scrollDrag) return;
    cancelAnimationFrame(scrollFrame);
    dragSeek(event);
    scrollDrag = null;
    clearTimeout(saveTimer);
    void action(persistPosition);
    const bounds = overlay.getBoundingClientRect();
    void invoke("reader_pointer", {
      inside:
        event.clientX >= bounds.left &&
        event.clientX < bounds.right &&
        event.clientY >= bounds.top &&
        event.clientY < bounds.bottom,
    });
  };
  track.addEventListener("pointerup", finishDrag);
  track.addEventListener("pointercancel", finishDrag);
  track.addEventListener("keydown", (event) => {
    if (
      ["ArrowDown", "PageDown", "ArrowUp", "PageUp", "Home", "End"].includes(
        event.key,
      )
    )
      event.preventDefault();
    if (["ArrowDown", "PageDown"].includes(event.key))
      void action(() => turn(1));
    if (["ArrowUp", "PageUp"].includes(event.key)) void action(() => turn(-1));
    if (event.key === "Home") seek(0);
    if (event.key === "End") seek(1);
  });
  document.querySelector(".drag")!.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  });
  document.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      if (scrollDrag) return;
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
