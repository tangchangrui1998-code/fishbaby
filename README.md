# fishbaby · 隐阅

macOS / Windows 悬浮文字阅读器。鼠标悬停在阅读区域时显示文字，移开后隐藏窗口，将鼠标操作交给下方应用。

现已实现第一版桌面应用：

- 本地 TXT / EPUB 书架：批量导入、去重、移出；EPUB 优先使用书内标题。
- 文本编码：自动读取 UTF-8（含 BOM）、GB18030/GBK/GB2312，以及带 BOM 的 UTF-16 LE/BE；原始文件保持不变。
- EPUB 2/3：按书内章节顺序提取正文，保留标题与段落，使用阅读器的字体、主题和进度；首版不保留原书图片、CSS 排版或提供目录跳转，暂不支持 DRM 加密书和纯图片书。
- 每本书独立保存阅读位置，切书和重启后继续阅读。原文件变化时尝试匹配原文，失败则按原百分比恢复。
- 无边框置顶阅读窗口、150 毫秒悬停唤醒、逐页阅读、暂停和位置调整。
- 翻页：底部“上一页 / 百分比 / 下一页”；正文左半区点击上一页、右半区点击下一页，拖选文字时不翻页；方向键、Page Up/Down、空格翻页，Shift+空格返回上一页，Home/End 到首末页。普通鼠标滚轮按刻度直接翻页，连续滚动无需停顿；触控板等平滑输入合并惯性，减少误跳页。
- 默认使用鼠标滚轮模式；使用触控板时，可在“阅读设置 → 滚轮设备”切换为触控板，选择会自动保存。
- 调整尺寸、字号、行距或单行模式后，保留当前页开头的原文位置，按新尺寸重新排版；页面按需计算，不预先拆分整本书，不显示随窗口变化的总页数。
- 深色 / 浅色阅读主题，可在阅读窗口顶部直接切换并保存。
- 书架点击阅读/继续阅读后最小化后台，显示阅读窗口；窗口内点击“开始阅读”才进入悬停隐藏，支持“暂停阅读”和“返回书架”。
- 字号、行距、文字颜色、底色浓度、单行模式和阅读区域尺寸设置。
- 菜单栏入口；`⌘⇧H` 暂停/继续，`⌘⇧O` 打开书架。

## 开发

需要 Node.js 22、Rust 1.88+。macOS 另需 Xcode Command Line Tools；Windows 需 Visual Studio C++ Build Tools 和 WebView2。

```sh
npm ci
npm run desktop
```

macOS 的书架和设置保存在 `~/Library/Application Support/com.fishbaby.reader/library.json`，Windows 保存在系统应用数据目录下的 `com.fishbaby.reader/library.json`；原始书籍保留在导入位置，应用不会上传内容或修改原文件。单文件上限 50 MB，EPUB 解压读取的正文及目录合计上限 50 MB。

## 检查与构建

```sh
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --debug --bundles app
```

应用包输出到 `src-tauri/target/debug/bundle/macos/隐阅.app`。`npm run dev` 仅启动前端服务，实际桌面能力需通过 `npm run desktop` 运行。

macOS 安装包采用 ad-hoc 签名，尚未配置 Developer ID 和公证。快捷键当前固定；多显示器切换、窗口隐藏后的鼠标穿透和反复唤醒仍需按[产品方案](docs/PRODUCT_PLAN.md)完成完整桌面验收。

界面测试页：运行 `npm run dev` 后打开 `http://127.0.0.1:1420/tests/reader.html?reader`。测试页使用独立的模拟文本和会话进度，不读取本地书架；提供窗口大小、字号、单行与重新载入控件，可检查按钮/键盘/滚轮翻页、缩放时原文位置不变及刷新后的恢复。正式应用仍通过 `npm run desktop` 运行。

## 平台分支与安装包

- `main`：公共代码基线。
- `mac`：macOS 开发与打包分支。
- `windows`：Windows 开发与 GitHub Actions 构建分支。

平台分支从同一份公共代码建立，Tauri 自动合并 `tauri.macos.conf.json` 或 `tauri.windows.conf.json`。公共修复先合入 `main`，再同步到平台分支；以后分别在对应平台分支开发。

macOS 本地打包（当前输出为 Apple Silicon / arm64）：

```sh
git switch mac
npm ci
npm run package:mac -- --ci
```

产物位于 `src-tauri/target/release/bundle/dmg/` 和 `src-tauri/target/release/bundle/macos/`。配置使用 ad-hoc 签名，未配置 Apple Developer ID 或公证。

Windows 在 GitHub 上手动构建：

1. 打开仓库 **Actions → Build Windows installer → Run workflow**。
2. 分支选择 **windows**，点击运行。选择其他分支会跳过构建。
3. 完成后在该次运行的 **Artifacts** 下载 `fishbaby-windows-x64`。
4. 解压后运行 `*-setup.exe`，`SHA256SUMS.txt` 用于校验下载文件。

工作流使用 Windows Server 2022、Node.js 22 和 Rust 1.88，运行测试后打 NSIS x64 安装包。安装范围为当前用户；未安装 WebView2 时会下载其安装引导程序。无需配置发布或签名密钥；安装包保留 30 天，不自动发布 GitHub Release。

本地 Windows 构建需 Visual Studio C++ Build Tools、WebView2 和 Rust MSVC 工具链，命令为 `npm run package:windows -- --ci`。
