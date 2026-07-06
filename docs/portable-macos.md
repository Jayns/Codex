# 便携启动器 — macOS 移植说明

Windows 便携启动器（`codex` bin）已经跑通。本文档给在 **Mac 上继续开发**的人（或 Mac 端的 Claude）一个清晰的起点。跨平台脚手架已经搭好，只剩 macOS 原生配置弹窗需要实现。

## 现状

- `codex` bin = `apps/codex-plus-launcher/src/portable_main.rs`，Windows / macOS 共用。
- 配置读写 `crates/codex-plus-core/src/portable.rs` —— 跨平台，无需改动。
- 配置弹窗按平台分模块：
  - `crates/codex-plus-core/src/portable_dialog/win32.rs` —— Windows 实现（已完成）。
  - `crates/codex-plus-core/src/portable_dialog/cocoa.rs` —— **macOS 待实现（当前是占位，会 bail 报错）**。
  - `crates/codex-plus-core/src/portable_dialog/mod.rs` —— 按平台分发。
- 启动 / CDP 注入 / relay 配置写入 —— `codex-plus-core` 已支持 macOS（`launcher.rs` 里有 `.app` / `open` 启动路径）。
- Windows 专属的任务栏图标、桌面快捷方式代码在 `portable_main.rs` 里都是 `#[cfg(windows)]`，macOS 不受影响（第一版可先不做 Dock 图标 / 快捷方式）。

## 需要做的事

### 1. 实现 `portable_dialog/cocoa.rs`

对齐 `win32.rs` 的行为，导出同一个函数：

```rust
pub fn show_portable_config_dialog(
    initial: &PortableConfig,
) -> anyhow::Result<Option<PortableConfig>>
```

一个模态窗口，含 5 个输入框（用 `initial` 预填）：

- API 网址 (Base URL) → `api_base_url`
- API Key（用 secure/密码输入框）→ `api_key`
- 默认模型 → `model`
- Provider 名称 → `provider_name`
- Codex App 路径 → `codex_app_dir`，旁边一个「浏览…」按钮，用 `NSOpenPanel`
  （`setCanChooseDirectories: true`）选目录

两个按钮：

- 「退出」→ 取消，返回 `Ok(None)`（调用方不启动 Codex）。
- 「保存并启动 Codex」→ 用各输入框的值构造 `PortableConfig` 返回 `Ok(Some(config))`。
  `debug_port` / `last_synced_hash` 沿用 `initial` 的值。

建议用 AppKit 绑定：`objc2` + `objc2-foundation` + `objc2-app-kit`。在
`crates/codex-plus-core/Cargo.toml` 里加：

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-foundation = "0.2"
objc2-app-kit = "0.2"
```

（版本以实际可编译为准，用 `cargo add` 拉最新兼容版。）

### 2. 编译 / 运行

```bash
# 需要 Xcode Command Line Tools: xcode-select --install
cargo build --release -p codex-plus-launcher --bin codex
./target/release/codex          # 首次运行弹配置窗口
./target/release/codex --config # 强制打开配置窗口
```

### 3. 便携包布局（macOS）

macOS 上 Codex 是 `.app` bundle，不是 Windows 那种松散文件夹。便携包大致：

```
Codex/
  codex              # 本启动器（Mach-O 可执行文件）
  codex_app/         # 放 Codex.app（或指向已安装的 /Applications/Codex.app）
  config.ini         # 配置
```

`portable.rs` 里 `default_portable_app_dir()` 返回可执行文件同级的 `codex_app`。
「Codex App 路径」填 `Codex.app` 的路径即可；`launcher.rs` 已能用 `open` 启动
`.app`。

## 待定 / 后续（第一版可跳过）

- **Dock 图标**：Windows 版把 Codex 原版图标应用到任务栏窗口。macOS 的 Dock 图标
  由 `.app` 的 `Info.plist` / icns 决定，机制不同，第一版先不做。
- **桌面快捷方式**：Windows 版在桌面建 `.lnk`。macOS 对应物是 alias 或 `.app`，
  第一版先不做。
- 若要签名 / 公证以便分发给他人，需要 Apple 开发者证书（自用可跳过）。
