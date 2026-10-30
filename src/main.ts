import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { boundary, PageReader, WheelPager } from "./reader";
import { readerTools } from "./tools-ui";
import { shelfExtras } from "./settings-ui";
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
  wheelMode: "mouse" | "trackpad";
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
    wheelMode: "mouse",
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
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
function toast(message: unknown) {
  const el = document.querySelector<HTMLElement>("#notice");
  if (el) {
    el.textContent = String(message);
    el.hidden = false;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => {
      el.hidden = true;
    }, 6500);
  }
}
async function action(fn: () => Promise<unknown>) {
  try {
    await fn();
  } catch (e) {
    toast(e);
  }
}
let extras: ReturnType<typeof shelfExtras> | undefined;
let tools: ReturnType<typeof readerTools> | undefined;
let resumeTimer: ReturnType<typeof setTimeout> | undefined;
async function refresh() {
  data = await invoke<Library>("library");
  if (reader) {
    styleReader();
  } else {
    renderBooks();
    void extras?.refresh().catch(toast);
    for (const id of ["width", "height"] as const)
      (document.getElementById(id) as HTMLInputElement).value = String(
        data.settings[id],
      );
    const theme = document.querySelector<HTMLSelectElement>("#theme");
    if (theme) theme.value = data.settings.theme;
    (document.querySelector("#wheelMode") as HTMLSelectElement).value =
      data.settings.wheelMode;
    (document.querySelector("#color") as HTMLInputElement).value =
      data.settings.color;
    (document.querySelector("#background") as HTMLInputElement).value = String(
      data.settings.background,
    );
    for (const [id, value] of [
      ["background", `${Math.round(data.settings.background * 100)}%`],
      ["fontSize", data.settings.fontSize],
      ["lineHeight", data.settings.lineHeight],
    ] as const) {
      document.getElementById(`${id}-value`)!.textContent = String(value);
    }
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
          return `<article class="book"><button class="cover" data-id="${b.id}" aria-label="阅读 ${esc(b.title)}"><strong>${esc(b.title)}</strong><span>${/\.epub$/i.test(b.path) ? "EPUB" : "TXT"}</span></button><div class="book-info"><h3 title="${esc(b.title)}">${esc(b.title)}</h3><div class="book-meta"><span>${b.available ? `${percent(b)}% · ${b.lastRead ? new Date(b.lastRead * 1000).toLocaleDateString("zh-CN") : "未读"}` : "文件不可用"}</span>${data.activeId === b.id ? '<span class="current">当前阅读</span>' : ""}</div><div class="progress" aria-hidden="true"><span style="width:${percent(b)}%"></span></div><div class="book-actions"><button class="read" data-id="${b.id}">${b.offset ? "继续阅读" : "阅读"}</button><button class="remove" data-remove="${b.id}" aria-label="将 ${esc(b.title)} 移出书架">移出</button></div></div></article>`;
        })
        .join("")
    : `<div class="empty"><h2>书架里还没有书</h2><p>导入本地 TXT 或 EPUB 文件，开始阅读。</p><button id="empty-import" class="primary">导入书籍</button></div>`;
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
              "移出后会清除这本书的阅读进度和书签，原始文件会保留。确定移出吗？",
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
      filters: [{ name: "电子书（TXT、EPUB）", extensions: ["txt", "epub"] }],
    });
    if (!paths) return;
    const errors = await invoke<string[]>("import_books", {
      paths: Array.isArray(paths) ? paths : [paths],
    });
    if (errors.length) toast(errors.join("\n"));
  });
}
function renderShelf() {
  root.innerHTML = `<aside><div class="brand">隐阅<span>fishbaby</span></div><nav aria-label="主导航"><button class="selected" data-view="library" aria-current="page">书架</button><button data-view="preferences">阅读设置</button></nav></aside><main><section id="library-view"><header><div class="heading"><h1>书架</h1><span id="count"></span></div><button id="import" class="primary">导入书籍</button></header><div class="toolbar"><span>最近阅读</span><div><button id="adjust">调整阅读区</button><button id="pause">暂停阅读</button></div></div><section id="books" aria-label="书籍"></section></section><section id="preferences-view" hidden><header class="settings-heading"><div><h1>阅读设置</h1><p class="page-caption">外观与排版设置会自动保存</p></div></header><div class="fields"><section class="setting-group"><h2>外观</h2><label>主题<select id="theme"><option value="light" ${data.settings.theme === "light" ? "selected" : ""}>浅色</option><option value="dark" ${data.settings.theme === "dark" ? "selected" : ""}>深色</option></select></label><label>文字颜色<input id="color" type="color" value="${data.settings.color}"></label><label>背景浓度<span><output id="background-value">${Math.round(data.settings.background * 100)}%</output><input id="background" type="range" min="0" max="1" step="0.05" value="${data.settings.background}"></span></label></section><section class="setting-group"><h2>排版</h2><label>字号<span><output id="fontSize-value">${data.settings.fontSize}</output><input id="fontSize" type="range" min="12" max="40" value="${data.settings.fontSize}"></span></label><label>行距<span><output id="lineHeight-value">${data.settings.lineHeight}</output><input id="lineHeight" type="range" min="1.2" max="2.5" step="0.1" value="${data.settings.lineHeight}"></span></label><label>滚轮设备<select id="wheelMode"><option value="mouse" ${data.settings.wheelMode !== "trackpad" ? "selected" : ""}>鼠标滚轮</option><option value="trackpad" ${data.settings.wheelMode === "trackpad" ? "selected" : ""}>触控板</option></select></label><label>单行阅读<input id="singleLine" type="checkbox" ${data.settings.singleLine ? "checked" : ""}></label></section><section class="setting-group"><h2>阅读窗口</h2><label>宽度<span><input id="width" type="number" min="220" max="1400" value="${data.settings.width}"> px</span></label><label>高度<span><input id="height" type="number" min="90" max="800" value="${data.settings.height}"> px</span></label><div class="setting-action"><span>拖动窗口顶部可调整位置</span><button id="reset">恢复居中</button></div></section></div></section><p id="notice" role="status" hidden></p></main>`;
  extras = shelfExtras(toast);
  void extras.refresh().catch(toast);
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach(
    (button) =>
      (button.onclick = () => {
        for (const view of ["library", "preferences", "about"]) {
          (document.querySelector(`#${view}-view`) as HTMLElement).hidden =
            view !== button.dataset.view;
        }
        if (button.dataset.view === "about")
          void extras?.refresh().catch(toast);
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
    if (output)
      output.textContent =
        input.id === "background"
          ? `${Math.round(Number(input.value) * 100)}%`
          : input.value;
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
      if ((event.target as HTMLElement).closest("#shortcut-settings")) return;
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
      settings.wheelMode = (
        document.getElementById("wheelMode") as HTMLSelectElement
      ).value as Settings["wheelMode"];
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
let readerRevision = -1;
interface ReaderView {
  state: string;
  revision: number;
}
function applyReaderView(view: ReaderView) {
  if (view.revision <= readerRevision) return;
  const resumed = readerState === "hidden" && view.state === "reading";
  readerRevision = view.revision;
  readerState = view.state;
  document.querySelector("#overlay")!.className = view.state;
  document.querySelector("#start")!.textContent =
    view.state === "reading" ? "暂停阅读" : "开始阅读";
  paginate();
  if (resumed) showResumeHint();
  if (view.state === "hidden") clearResumeHint();
  reportReaderView();
}
function clearResumeHint() {
  clearTimeout(resumeTimer);
  document.querySelector("#text")?.classList.remove("resume-hint");
}
function showResumeHint() {
  clearResumeHint();
  document.querySelector("#text")?.classList.add("resume-hint");
  resumeTimer = setTimeout(clearResumeHint, 1000);
}
function reportReaderView() {
  const overlay = document.querySelector("#overlay")!;
  void invoke("reader_report", {
    revision: readerRevision,
    state: overlay.className,
    hovered: overlay.matches(":hover"),
    visible: document.visibilityState === "visible",
    contentVisible: getComputedStyle(overlay).visibility === "visible",
  }).catch(console.warn);
}
let text = "";
let book: Book | null = null;
let pages: PageReader | null = null;
let layoutKey = "";
let layoutReady = false;
let resizeTimer: ReturnType<typeof setTimeout> | undefined;
let saveQueue: Promise<unknown> = Promise.resolve();
const wheelPager = new WheelPager();
function fitsText(part: string) {
  const el = document.querySelector<HTMLElement>("#text")!;
  // Line breaks at a page boundary belong to this page, not a blank next page.
  el.textContent = part.trimEnd();
  return (
    el.scrollHeight <= el.clientHeight + 1 &&
    el.scrollWidth <= el.clientWidth + 1
  );
}
function persistPosition() {
  if (!book) return Promise.resolve();
  const { id, offset } = book;
  const anchor = text.slice(offset, boundary(text, offset + 120));
  const save = saveQueue
    .catch(() => {})
    .then(() => invoke("save_progress", { id, offset, anchor }));
  saveQueue = save;
  return save;
}
function syncPageControls() {
  const previous = document.querySelector<HTMLButtonElement>("#previous-page")!;
  const next = document.querySelector<HTMLButtonElement>("#next-page")!;
  previous.disabled = !layoutReady || !pages || pages.offset === 0;
  next.disabled = !layoutReady || !pages || pages.end >= text.length;
  document.querySelector("#page-progress")!.textContent = book
    ? `${pages && pages.end >= text.length ? 100 : percent({ ...book, length: text.length })}%`
    : "—";
}
function styleReader() {
  const el = document.querySelector<HTMLElement>("#text");
  if (!el) return;
  const s = data.settings;
  el.style.fontSize = `${s.fontSize}px`;
  el.style.lineHeight = String(s.lineHeight);
  el.style.maxHeight = s.singleLine ? `${s.fontSize * s.lineHeight}px` : "";
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
  const el = document.querySelector<HTMLElement>("#text")!;
  const hint = document.querySelector<HTMLElement>("#layout-hint")!;
  layoutReady =
    !!book &&
    !!pages &&
    el.clientHeight + 1 >= parseFloat(getComputedStyle(el).lineHeight);
  hint.hidden = !book || layoutReady;
  el.style.visibility = hint.hidden ? "" : "hidden";
  if (!book || !pages || !layoutReady) {
    syncPageControls();
    return;
  }
  layoutKey = [
    el.clientWidth,
    el.clientHeight,
    data.settings.fontSize,
    data.settings.lineHeight,
    data.settings.singleLine,
  ].join(":");
  pages.layout(layoutKey, fitsText);
  book.offset = pages.offset;
  el.textContent = text.slice(pages.offset, pages.end).trimEnd();
  el.scrollTop = 0;
  el.scrollLeft = 0;
  document.querySelector("#position")!.textContent = book.title;
  syncPageControls();
  tools?.update();
}
async function loadCurrent() {
  try {
    clearTimeout(resizeTimer);
    [book, text] = await invoke<[Book, string]>("current_text");
    pages = new PageReader(text, book.offset);
    styleReader();
    await tools?.reset().catch(toast);
  } catch {
    book = null;
    pages = null;
    text = "";
    document.querySelector("#position")!.textContent = "";
    document.querySelector("#text")!.textContent = "请在书架中选择一本书";
    paginate();
  }
}
async function turn(direction: number) {
  // Finish a pending reflow before using any cached page boundary.
  clearTimeout(resizeTimer);
  paginate();
  if (!pages || !book || !layoutReady || tools?.isOpen) return;
  clearResumeHint();
  if (!pages.turn(direction, layoutKey, fitsText)) return;
  paginate();
  await persistPosition();
}
async function jump(last: boolean) {
  clearTimeout(resizeTimer);
  paginate();
  if (!pages || !book || !layoutReady) return;
  pages.jump(last, layoutKey, fitsText);
  paginate();
  await persistPosition();
}
async function goToOffset(offset: number) {
  if (!book) return;
  clearTimeout(resizeTimer);
  clearResumeHint();
  pages = new PageReader(text, offset);
  paginate();
  await persistPosition();
  showResumeHint();
}
function renderReader() {
  document.documentElement.classList.add("reader-root");
  document.body.classList.add("reader");
  root.innerHTML =
    '<section id="overlay" class="hidden" tabindex="0" aria-label="阅读窗口"><div class="drag"><span id="position"></span><button id="reader-menu" aria-label="打开阅读工具">工具</button><button id="theme-toggle" aria-label="切换主题">深色</button><button id="start">开始阅读</button><button id="back">返回书架</button></div><div class="reader-content"><div id="text"></div><span id="layout-hint" hidden>请调高窗口以显示完整文字</span></div><nav class="page-controls" aria-label="翻页"><button id="previous-page" aria-controls="text" title="上一页（← / Page Up）" disabled>上一页</button><span id="chapter-position" aria-label="当前章节"></span><output id="page-progress" aria-label="阅读进度" aria-live="polite">—</output><button id="next-page" aria-controls="text" title="下一页（→ / 空格 / Page Down）" disabled>下一页</button></nav><p id="notice" role="status" hidden></p></section>';
  tools = readerTools({
    book: () => book,
    text: () => text,
    offset: () => book?.offset ?? 0,
    go: goToOffset,
    notify: toast,
  });
  document
    .querySelector("#reader-menu")!
    .addEventListener("click", () => void tools!.open().catch(toast));
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
    void action(() => invoke("reader_pointer", { inside: false }));
  });
  overlay.addEventListener("pointerdown", (event) => {
    if (
      !(event.target as HTMLElement).closest("button, input, textarea, select")
    )
      (overlay as HTMLElement).focus({ preventScroll: true });
  });
  document
    .querySelector("#previous-page")!
    .addEventListener("click", () => void action(() => turn(-1)));
  document
    .querySelector("#next-page")!
    .addEventListener("click", () => void action(() => turn(1)));
  document
    .querySelector(".reader-content")!
    .addEventListener("click", (event) => {
      const click = event as MouseEvent;
      if (
        readerState === "hidden" ||
        click.button !== 0 ||
        click.detail > 1 ||
        click.ctrlKey ||
        click.metaKey ||
        click.altKey ||
        click.shiftKey
      )
        return;
      if (getSelection()?.toString()) return;
      const bounds = document
        .querySelector(".reader-content")!
        .getBoundingClientRect();
      void action(() =>
        turn(click.clientX < bounds.left + bounds.width / 2 ? -1 : 1),
      );
    });
  document.addEventListener("keydown", (event) => {
    if (tools?.isOpen) return;
    if (event.key === "Escape" && readerState !== "hidden") {
      event.preventDefault();
      void action(() => invoke("set_mode", { mode: "paused" }));
      return;
    }
    if (
      (event.ctrlKey || event.metaKey) &&
      event.key.toLowerCase() === "f" &&
      readerState !== "hidden"
    ) {
      event.preventDefault();
      void tools?.open("search").catch(toast);
      return;
    }
    if (
      readerState === "hidden" ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      event.isComposing
    )
      return;
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select, [contenteditable=true]"))
      return;
    if (event.key === " " && target.closest("button")) return;
    let direction = 0;
    if (["ArrowRight", "ArrowDown", "PageDown"].includes(event.key))
      direction = 1;
    if (["ArrowLeft", "ArrowUp", "PageUp"].includes(event.key)) direction = -1;
    if (event.key === " ") direction = event.shiftKey ? -1 : 1;
    if (!direction && !["Home", "End"].includes(event.key)) return;
    event.preventDefault();
    if (direction) void action(() => turn(direction));
    else void action(() => jump(event.key === "End"));
  });
  document.querySelector(".drag")!.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button, input")) return;
    void getCurrentWindow().startDragging();
  });
  document.addEventListener(
    "wheel",
    (event) => {
      if ((event.target as HTMLElement).closest("#reader-tools")) return;
      if (
        readerState === "hidden" ||
        tools?.isOpen ||
        event.ctrlKey ||
        event.metaKey
      )
        return;
      if (Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
      event.preventDefault();
      const direction = wheelPager.push(
        event.deltaY,
        event.deltaMode,
        performance.now(),
        root.clientHeight,
        data.settings.wheelMode !== "trackpad",
      );
      if (direction) void action(() => turn(direction));
    },
    { passive: false },
  );
  new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(paginate, 120);
  }).observe(document.querySelector("#text")!);
}
async function boot() {
  if (!isTauri()) {
    root.innerHTML =
      '<div class="browser-note"><h1>隐阅</h1><p>这是一个桌面应用。请在项目目录执行 <code>npm run desktop</code>。</p></div>';
    return;
  }
  if (reader) renderReader();
  data = await invoke<Library>("library");
  if (reader) {
    await listen<ReaderView>("reader-state", (e) => applyReaderView(e.payload));
    await listen<string>("reader-action", (e) => {
      if (!tools?.isOpen && readerState !== "hidden")
        void action(() => turn(e.payload === "next" ? 1 : -1));
    });
    await listen("book-changed", loadCurrent);
    await loadCurrent();
    // The first native event can precede lazy WebView initialization on Windows.
    // Hydrate after installing listeners; revisions reject late/stale responses.
    applyReaderView(await invoke<ReaderView>("reader_state"));
    // Recover a missed native event; heartbeat records the state actually applied
    // by the WebView, independently of the desktop-side state machine.
    let checking = false;
    let heartbeat = 0;
    setInterval(async () => {
      if (checking) return;
      checking = true;
      try {
        applyReaderView(await invoke<ReaderView>("reader_state"));
        if (++heartbeat % 5 === 0) reportReaderView();
      } catch (error) {
        console.warn("reader-state-sync", error);
      } finally {
        checking = false;
      }
    }, 1000);
  } else renderShelf();
  await listen("library-changed", () => void refresh());
  await invoke("frontend_ready");
}
void boot().catch((error) => {
  if (!root.textContent?.trim())
    root.innerHTML = `<div class="browser-note"><h1>启动未完成</h1><p></p></div>`;
  const target = root.querySelector(".browser-note p");
  if (target) target.textContent = String(error);
  toast(error);
});
