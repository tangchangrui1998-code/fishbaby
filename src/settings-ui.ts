import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
const names: Record<string, string> = {
  toggle: "隐藏／继续",
  shelf: "返回书架",
  adjust: "调整阅读区",
  previous: "上一页",
  next: "下一页",
};
const defaults = {
  toggle: "CommandOrControl+Shift+H",
  shelf: "CommandOrControl+Shift+O",
  adjust: "CommandOrControl+Shift+J",
  previous: "CommandOrControl+Alt+Left",
  next: "CommandOrControl+Alt+Right",
};
interface Info {
  version: string;
  preview: boolean;
  revision: string;
  builtAt: string;
  platform: string;
  architecture: string;
  logDirectory: string;
  shortcuts: { action: string; key: string; active: boolean }[];
  layouts: { display: string; width: number; height: number }[];
}
export function shelfExtras(notify: (message: unknown) => void) {
  const button = document.createElement("button");
  button.dataset.view = "about";
  button.textContent = "关于与诊断";
  document.querySelector('nav[aria-label="主导航"]')!.append(button);
  const group = document.createElement("section");
  group.className = "setting-group";
  group.id = "shortcut-settings";
  group.innerHTML =
    '<h2>快捷键</h2><p class="setting-note">点击输入框，再按下组合键。保存时会检查重复和系统占用。</p>' +
    Object.entries(names)
      .map(
        ([id, label]) =>
          `<label>${label}<input data-shortcut="${id}" aria-label="${label}快捷键" readonly placeholder="按下组合键"></label>`,
      )
      .join("") +
    '<div class="settings-actions"><button id="shortcut-defaults">填入默认值</button><button id="save-shortcuts" class="primary">保存快捷键</button></div><p id="shortcut-status" role="status"></p>';
  document.querySelector(".fields")!.append(group);
  const about = document.createElement("section");
  about.id = "about-view";
  about.hidden = true;
  about.innerHTML =
    '<header><h1>关于与诊断</h1></header><section class="setting-group"><h2>隐阅</h2><dl id="app-info"></dl><p class="setting-note">当前为开发体验阶段，不会自动下载或安装新版本。</p></section><section class="setting-group"><h2>本地诊断</h2><p class="setting-note">导出内容包括版本、系统信息和窗口诊断日志，不含书籍正文、书名及书架文件，不会上传。</p><p id="log-directory"></p><div class="settings-actions"><button id="open-logs">打开日志目录</button><button id="export-logs">导出诊断文件</button></div></section><section class="setting-group"><h2>显示器布局</h2><p id="display-layouts"></p><p class="setting-note">每块显示器分别记住阅读区位置和尺寸。显示器名称由系统提供，同名显示器可能共用一份布局。</p></section>';
  document.querySelector("main")!.append(about);
  const attempt = (action: () => Promise<unknown>) =>
    void action().catch(notify);
  let editing = false;
  const fields = () =>
    Array.from(group.querySelectorAll<HTMLInputElement>("[data-shortcut]"));
  fields().forEach(
    (input) =>
      (input.onkeydown = (event) => {
        if (
          event.key === "Tab" &&
          !event.ctrlKey &&
          !event.metaKey &&
          !event.altKey
        )
          return;
        if (event.key === "Escape") {
          input.blur();
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        if (["Control", "Meta", "Alt", "Shift"].includes(event.key)) return;
        if (
          !event.ctrlKey &&
          !event.metaKey &&
          !event.altKey &&
          !event.shiftKey
        ) {
          notify("请至少包含一个修饰键");
          return;
        }
        const parts = [];
        if (event.metaKey) parts.push("Super");
        if (event.ctrlKey) parts.push("Control");
        if (event.altKey) parts.push("Alt");
        if (event.shiftKey) parts.push("Shift");
        const key = event.code.startsWith("Key")
          ? event.code.slice(3)
          : event.code.startsWith("Digit")
            ? event.code.slice(5)
            : ({
                ArrowLeft: "Left",
                ArrowRight: "Right",
                ArrowUp: "Up",
                ArrowDown: "Down",
                Space: "Space",
              }[event.code] ?? event.code);
        input.value = [...parts, key].join("+");
        editing = true;
      }),
  );
  group.querySelector<HTMLButtonElement>("#shortcut-defaults")!.onclick =
    () => {
      fields().forEach(
        (input) =>
          (input.value =
            defaults[input.dataset.shortcut as keyof typeof defaults]),
      );
      editing = true;
    };
  group.querySelector<HTMLButtonElement>("#save-shortcuts")!.onclick = () =>
    attempt(async () => {
      const bindings = Object.fromEntries(
        fields().map((input) => [input.dataset.shortcut, input.value]),
      );
      await invoke("save_shortcuts", { bindings });
      editing = false;
      await refresh();
      notify("快捷键已保存");
    });
  about.querySelector<HTMLButtonElement>("#open-logs")!.onclick = () =>
    attempt(() => invoke("open_log_directory"));
  about.querySelector<HTMLButtonElement>("#export-logs")!.onclick = () =>
    attempt(async () => {
      const destination = await save({
        defaultPath: `fishbaby-diagnostics-${new Date().toISOString().slice(0, 10)}.zip`,
        filters: [{ name: "诊断文件", extensions: ["zip"] }],
      });
      if (destination) {
        await invoke("export_diagnostics", { destination });
        notify("诊断文件已导出");
      }
    });
  async function refresh() {
    const info = await invoke<Info>("app_information");
    if (!editing)
      fields().forEach((input) => {
        input.value =
          info.shortcuts.find((s) => s.action === input.dataset.shortcut)
            ?.key ?? "";
      });
    const inactive = info.shortcuts
      .filter((s) => !s.active)
      .map((s) => names[s.action]);
    group.querySelector("#shortcut-status")!.textContent = inactive.length
      ? `未启用：${inactive.join("、")}。请改用其他组合键后保存。`
      : "快捷键均已启用";
    const dl = about.querySelector("#app-info")!;
    dl.replaceChildren();
    for (const [name, value] of [
      ["版本", `${info.version} · 开发预览`],
      ["代码版本", info.revision],
      ["构建时间", new Date(Number(info.builtAt) * 1000).toLocaleString()],
      ["系统", `${info.platform} / ${info.architecture}`],
    ]) {
      const dt = document.createElement("dt"),
        dd = document.createElement("dd");
      dt.textContent = name;
      dd.textContent = value;
      dl.append(dt, dd);
    }
    about.querySelector("#log-directory")!.textContent = info.logDirectory;
    about.querySelector("#display-layouts")!.textContent = info.layouts.length
      ? info.layouts
          .map(
            (l) =>
              `${l.display}：${Math.round(l.width)} × ${Math.round(l.height)}`,
          )
          .join("；")
      : "在阅读窗口使用后自动记住布局";
  }
  return { refresh };
}
