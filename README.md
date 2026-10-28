# fishbaby · 隐阅

macOS 悬浮文字阅读器。鼠标悬停在阅读区域时显示文字，移开后隐藏窗口，将鼠标操作交给下方应用。

现已实现第一版桌面应用：

- 本地 TXT 书架：批量导入、去重、移出。
- 每本书独立保存阅读位置，切书和重启后继续阅读。原文件变化时尝试匹配原文，失败则按原百分比恢复。
- 无边框置顶阅读窗口、150 毫秒悬停唤醒、滚轮翻页、整本书进度滚动条、暂停和位置调整。
- 深色 / 浅色阅读主题，可在阅读窗口顶部直接切换并保存。
- 书架点击阅读/继续阅读后最小化后台，显示阅读窗口；窗口内点击“开始阅读”才进入悬停隐藏，支持“暂停阅读”和“返回书架”。
- 字号、行距、文字颜色、底色浓度、单行模式和阅读区域尺寸设置。
- 菜单栏入口；`⌘⇧H` 暂停/继续，`⌘⇧O` 打开书架。

## 开发

需要 macOS、Node.js 22、Rust 1.88+ 和 Xcode Command Line Tools。

```sh
npm ci
npm run desktop
```

书架和设置保存在 `~/Library/Application Support/com.fishbaby.reader/library.json`；原始 TXT 保留在导入位置，应用不会上传内容或修改原文件。

## 检查与构建

```sh
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --debug --bundles app
```

应用包输出到 `src-tauri/target/debug/bundle/macos/隐阅.app`。`npm run dev` 仅启动前端服务，实际桌面能力需通过 `npm run desktop` 运行。

这是一版开发构建，尚未签名和公证。快捷键当前固定；多显示器切换、窗口隐藏后的鼠标穿透和反复唤醒仍需按[产品方案](docs/PRODUCT_PLAN.md)完成完整桌面验收。

界面测试页：运行 `npm run dev` 后打开 `http://127.0.0.1:1420/tests/reader.html?reader`。测试页使用独立的模拟文本，不读取本地书架；可检查滚动条拖动、键盘跳转及保存回调。正式应用仍通过 `npm run desktop` 运行。
