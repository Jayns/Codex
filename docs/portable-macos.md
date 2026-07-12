# 便携启动器 — macOS 移植说明

Windows / macOS 便携启动器（`chatgpt-launcher` bin）都已跑通。本文档记录 macOS 端的实现现状，供后续维护参考。

## 现状

- `chatgpt-launcher` bin = `apps/codex-plus-launcher/src/portable_main.rs`，Windows / macOS 共用。
- 配置读写 `crates/codex-plus-core/src/portable.rs` —— 跨平台，无需改动。
- 配置弹窗按平台分模块：
  - `crates/codex-plus-core/src/portable_dialog/win32.rs` —— Windows 实现（Win32 原生控件）。
  - `crates/codex-plus-core/src/portable_dialog/cocoa.rs` —— macOS 实现（AppKit，`objc2`/`objc2-app-kit`/`objc2-foundation`），已完成。
  - `crates/codex-plus-core/src/portable_dialog/mod.rs` —— 按平台分发。
- 启动 / CDP 注入 / relay 配置写入 —— `codex-plus-core` 已支持 macOS（`launcher.rs` 里有 `.app` / `open` 启动路径）。
- Windows 专属的任务栏图标、桌面快捷方式代码在 `portable_main.rs` 里都是 `#[cfg(windows)]`，macOS 不受影响。
- macOS 打包脚本 `scripts/installer/macos/package-portable.sh` 把 `chatgpt-launcher` 包成
  `ChatGPT Launcher.app`（`LSUIElement`，无 Dock 图标），双击直接运行，不会像裸
  可执行文件那样弹出 Terminal 窗口（详见下文）。

## macOS 配置弹窗（`portable_dialog/cocoa.rs`）

`show_portable_config_dialog(initial: &PortableConfig) -> anyhow::Result<Option<PortableConfig>>`：
用 `NSApplication::runModalForWindow` 弹出一个模态窗口，含 5 个输入框（用 `initial` 预填）：

- API 网址 (Base URL) → `api_base_url`
- API Key（`NSSecureTextField`）→ `api_key`
- 默认模型 → `model`
- Provider 名称 → `provider_name`
- Codex App 路径 → `codex_app_dir`，旁边一个「浏览」按钮，用 `NSOpenPanel`
  （`canChooseFiles`/`canChooseDirectories` 都开，方便直接选 `.app` 包）

两个按钮：

- 「退出」（或点击标题栏关闭按钮）→ 取消，返回 `Ok(None)`（调用方不启动 Codex）。
- 「保存并启动 Codex」→ 用各输入框的值构造 `PortableConfig` 返回 `Ok(Some(config))`。
  `debug_port` / `last_synced_hash` 沿用 `initial` 的值。

按钮 target/action 通过 `objc2::define_class!` 生成的 `ConfigDialogDelegate`（同时也是
`NSWindowDelegate`）实现，字段值存放在 `OnceCell<Retained<...>>` ivars 里。

依赖在 `crates/codex-plus-core/Cargo.toml`：

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = "0.3"
objc2-foundation = "0.3"
```

### 编译 / 运行

```bash
# 需要 Xcode Command Line Tools: xcode-select --install
cargo build --release -p codex-plus-launcher --bin chatgpt-launcher
./target/release/chatgpt-launcher          # 首次运行弹配置窗口
./target/release/chatgpt-launcher --config # 强制打开配置窗口
```

直接跑裸的 `target/release/chatgpt-launcher` 只适合本地调试：macOS 对没有 `.app` 包装的
Unix 可执行文件，双击时 Finder 会自动开一个 Terminal 窗口来跑它（这不是 bug，
是系统行为），而且这个终端窗口会一直挂着，因为便携启动器设计上要一直存活到
Codex 退出为止（见下面「打包成 .app」）。

### 打包成 .app（`scripts/installer/macos/package-portable.sh`）

```bash
scripts/installer/macos/package-portable.sh dist/macos/portable --build
```

会生成 `dist/macos/portable/ChatGPT Launcher.app`：

- `Info.plist` 里 `LSUIElement = true`，没有 Dock 图标；双击直接运行，不会像裸
  可执行文件那样弹出 Terminal 窗口。
- 图标复用 Windows 便携版任务栏用的同一份 Codex App 图标
  （`apps/codex-plus-launcher/assets/codex-app-icon.ico`），先转成 PNG 再用
  `sips` + `iconutil` 生成 `.icns`，两个平台的便携启动器图标保持一致（不是
  `codex-plus-manager` 那个 "C++" logo）。
- 打包后会做一次 ad-hoc 签名（`codesign --sign -`），避免刚构建出来的 bundle
  被 Gatekeeper 当成"已损坏"拒绝运行；不涉及需要 Apple 开发者证书的正式签名/公证。
- 参数：`[OutputDir] [--build] [--version X.Y.Z]`；`--build` 会先跑
  `cargo build --release -p codex-plus-launcher --bin chatgpt-launcher`。

### `config.ini` 的位置

macOS 上 `config.ini` **统一保存在**
`~/Library/Application Support/ChatGPT Launcher/config.ini`
（`portable.rs` 里 `portable_root_dir()` 的 macOS 分支），不再随 `.app` 的
位置变化。原因：通过微信/网盘等网络方式分发的 `.app` 带隔离标记，Gatekeeper
的 App Translocation 会把它挪到一个**只读**随机挂载点运行，写在 bundle 旁边
必然失败（os error 30），统一到用户目录在所有启动场景下都可写。
Windows 不受影响，仍是 exe 旁边的经典便携布局。

### Codex App 路径的默认值

macOS 便携包**不再随包携带一份 Codex.app**（不像 Windows 那样在 `codex_app/`
目录里放一份本地程序）。默认值改为直接查找已安装的原版 Codex：

- `portable_main.rs` 里的 `platform_default_app_dir()` 在 macOS 上调用
  `codex_plus_core::app_paths::find_macos_codex_app_default()`，会在
  `/Applications` 和 `~/Applications` 下查找 `Codex.app` /
  `OpenAI Codex.app` / `OpenAI.Codex.app`。
- 配置窗口首次打开时会用这个默认值预填「ChatGPT App 路径」；用户也可以用「浏览」
  按钮手动指向别处的 `Codex.app`。
- 如果 `codex_app_dir` 为空且自动查找也没找到（用户没装 Codex 或装在非常规位置），
  启动时会直接报错并提示用 `chatgpt-launcher --config` 打开配置窗口手动选择，而不是
  静默失败或退回到一个不存在的本地路径。

macOS 便携包布局大致：

```
dist/macos/portable/
  ChatGPT Launcher.app/   # 打包脚本生成，双击运行
  使用说明.txt             # 打包脚本生成，随包分发
```

（`config.ini` 在首次保存配置后生成于
`~/Library/Application Support/ChatGPT Launcher/`，不在分发文件夹里。）

（不需要 `codex_app/` 子目录；`launcher.rs` 已能用 `open` 启动已安装的 `.app`。）

## 待定 / 后续（第一版可跳过）

- **Dock 图标**：Windows 版把 Codex 原版图标应用到任务栏窗口。`ChatGPT Launcher.app`
  设了 `LSUIElement`，本来就不显示 Dock 图标，暂不需要额外处理。
- **桌面快捷方式**：Windows 版在桌面建 `.lnk`。macOS 对应物是把 `.app`
  拖进 Dock/`/Applications`，第一版先不自动做。
- 若要正式签名 / 公证以便分发给他人（而不只是自用），需要 Apple 开发者证书；
  目前 `package-portable.sh` 只做本地 ad-hoc 签名。
