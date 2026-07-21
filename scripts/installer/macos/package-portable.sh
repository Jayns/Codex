#!/usr/bin/env bash
set -euo pipefail

# Assembles a self-contained macOS portable distribution:
#
#   <OutputDir>/
#     ChatGPT Launcher.app/            double-click to run; config dialog on first launch
#     Codex++ 皮肤管理工具.app/        sibling bundle (separate from the installed
#                                      full manager's Codex++ 管理工具.app / bundle
#                                      id); "打开皮肤管理" in the injected Codex
#                                      menu launches this restricted to the
#                                      皮肤管理 (Dream Skin) screen (--skin-only) —
#                                      the portable build's relay/plugin settings
#                                      live in config.ini instead, so the rest of
#                                      the manager UI would be redundant here.
#
# Unlike the DMG installer (package-dmg.sh), this does not register either
# app anywhere; the folder (and the config.ini created next to the launcher
# on first run) can be moved to another machine and run as-is. Unlike the
# Windows portable build (package-portable.ps1), macOS never bundles its own
# copy of Codex App: the "ChatGPT App 路径" defaults to whatever Codex.app is
# already installed under /Applications or ~/Applications (see
# codex_plus_core::app_paths::find_macos_codex_app_default).
#
# Packaging as a proper .app (rather than shipping the loose Mach-O binaries)
# matters because macOS auto-opens a Terminal window to run an un-bundled
# Unix executable when it's double-clicked in Finder; a bundled .app with
# LSUIElement runs directly with no Terminal window.
#
# Usage:
#   scripts/installer/macos/package-portable.sh [OutputDir] [--build] [--version X.Y.Z]
#
#   scripts/installer/macos/package-portable.sh dist/macos/portable --build

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"

OUTPUT_DIR="dist/macos/portable"
BUILD=0
VERSION="0.0.0"

while [ $# -gt 0 ]; do
  case "$1" in
    --build)
      BUILD=1
      shift
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    *)
      OUTPUT_DIR="$1"
      shift
      ;;
  esac
done

APP_NAME="ChatGPT Launcher"
EXECUTABLE_NAME="chatgpt-launcher"
BUNDLE_ID="com.bigpizzav3.codexplusplus.portable"
BINARY_PATH="$ROOT/target/release/chatgpt-launcher"
# Same Codex App icon used for the Windows portable launcher's taskbar icon
# (apps/codex-plus-launcher/src/portable_main.rs, CODEX_APP_ICON), not the
# codex-plus-manager "C++" logo, so both platforms' portable launchers show
# the actual Codex icon.
ICON_SOURCE_ICO="$ROOT/apps/codex-plus-launcher/assets/codex-app-icon.ico"

MANAGER_APP_NAME="Codex++ 皮肤管理工具"
MANAGER_EXECUTABLE_NAME="codex-plus-plus-manager"
# Distinct from the installed full manager's com.bigpizzav3.codexplusplus.manager
# so the two never get conflated by Launch Services on a machine that has both.
MANAGER_BUNDLE_ID="com.bigpizzav3.codexplusplus.skinmanager"
MANAGER_BINARY_PATH="$ROOT/target/release/codex-plus-plus-manager"
MANAGER_ICON_SOURCE_PNG="$ROOT/apps/codex-plus-manager/src-tauri/icons/icon.png"

if [ "$BUILD" -eq 1 ]; then
  (cd "$ROOT" && cargo build --release -p codex-plus-launcher --bin chatgpt-launcher)
  (cd "$ROOT/apps/codex-plus-manager" && npm install --package-lock=false && npm run vite:build)
  (cd "$ROOT" && cargo build --release -p codex-plus-manager --bin codex-plus-plus-manager)
fi

if [ ! -x "$BINARY_PATH" ]; then
  echo "error: built binary not found at $BINARY_PATH." >&2
  echo "Pass --build, or build it manually first: cargo build --release -p codex-plus-launcher --bin chatgpt-launcher" >&2
  exit 1
fi

if [ ! -x "$MANAGER_BINARY_PATH" ]; then
  echo "error: built binary not found at $MANAGER_BINARY_PATH." >&2
  echo "Pass --build, or build it manually first: (cd apps/codex-plus-manager && npm install && npm run vite:build) && cargo build --release -p codex-plus-manager --bin codex-plus-plus-manager" >&2
  exit 1
fi

OUTPUT_PATH="$ROOT/$OUTPUT_DIR"
mkdir -p "$OUTPUT_PATH"

# Builds an .icns from a single square source image (PNG, or any format sips
# can read) at every size macOS expects. Shared by both bundles below so
# each keeps its own icon.
build_icns() {
  local source_image="$1"
  local out_icns="$2"
  local workdir
  workdir="$(mktemp -d)"
  local iconset="$workdir/icon.iconset"
  mkdir -p "$iconset"
  sips -z 16 16 "$source_image" --out "$iconset/icon_16x16.png" >/dev/null
  sips -z 32 32 "$source_image" --out "$iconset/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 "$source_image" --out "$iconset/icon_32x32.png" >/dev/null
  sips -z 64 64 "$source_image" --out "$iconset/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 "$source_image" --out "$iconset/icon_128x128.png" >/dev/null
  sips -z 256 256 "$source_image" --out "$iconset/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 "$source_image" --out "$iconset/icon_256x256.png" >/dev/null
  sips -z 512 512 "$source_image" --out "$iconset/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 "$source_image" --out "$iconset/icon_512x512.png" >/dev/null
  sips -z 1024 1024 "$source_image" --out "$iconset/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$iconset" -o "$out_icns"
  rm -rf "$workdir"
}

# Assembles <OutputPath>/<app_name>.app from a plain binary: copies the
# executable in, converts+embeds the icon (icon_source may be empty to skip),
# writes Info.plist, and ad-hoc signs it (no Apple notarization — Gatekeeper
# still blocks first launch, see 使用说明.txt below).
create_app() {
  local app_name="$1"
  local executable_name="$2"
  local binary_path="$3"
  local bundle_id="$4"
  local icon_source="$5"
  local lsui_element="$6"
  local app_dir="$OUTPUT_PATH/$app_name.app"
  local icon_name="icon.icns"

  rm -rf "$app_dir"
  mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
  cp "$binary_path" "$app_dir/Contents/MacOS/$executable_name"
  chmod +x "$app_dir/Contents/MacOS/$executable_name"

  if [ -n "$icon_source" ] && [ -f "$icon_source" ] && command -v iconutil >/dev/null 2>&1; then
    local icon_png="$icon_source"
    if [ "${icon_source##*.}" != "png" ]; then
      # `sips` can't scale a multi-image .ico directly past its largest
      # embedded frame, so extract the largest embedded frame to a plain PNG
      # first and scale every iconset size from that.
      icon_png="$(mktemp -t codex-portable-icon).png"
      sips -s format png "$icon_source" --out "$icon_png" >/dev/null
    fi
    build_icns "$icon_png" "$app_dir/Contents/Resources/$icon_name"
    [ "$icon_png" != "$icon_source" ] && rm -f "$icon_png"
  fi

  printf 'APPL????' > "$app_dir/Contents/PkgInfo"
  cat > "$app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$app_name</string>
  <key>CFBundleDisplayName</key>
  <string>$app_name</string>
  <key>CFBundleIdentifier</key>
  <string>$bundle_id</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleSignature</key>
  <string>????</string>
  <key>CFBundleExecutable</key>
  <string>$executable_name</string>
  <key>CFBundleIconFile</key>
  <string>$icon_name</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>LSUIElement</key>
  <$lsui_element/>
</dict>
</plist>
PLIST

  # Ad-hoc sign so Gatekeeper doesn't flag the freshly-built bundle as damaged.
  codesign --force --sign - "$app_dir/Contents/MacOS/$executable_name"
  codesign --force --sign - "$app_dir"
}

create_app "$APP_NAME" "$EXECUTABLE_NAME" "$BINARY_PATH" "$BUNDLE_ID" "$ICON_SOURCE_ICO" "true"
create_app "$MANAGER_APP_NAME" "$MANAGER_EXECUTABLE_NAME" "$MANAGER_BINARY_PATH" "$MANAGER_BUNDLE_ID" "$MANAGER_ICON_SOURCE_PNG" "false"

APP_DIR="$OUTPUT_PATH/$APP_NAME.app"

# End-user README shipped next to the .app. The bundle is only ad-hoc signed
# (no Apple notarization), so recipients hit the Gatekeeper "无法验证" block on
# first open; the README walks them through that and the first-run setup.
cat > "$OUTPUT_PATH/使用说明.txt" <<README
$APP_NAME 使用说明
==============================

本文件夹是完整的分发包，可整体拷贝到其他 Mac 上使用。

一、准备工作
1. 本工具需要官方 ChatGPT 桌面应用。如果尚未安装，请先打开同目录下的
   ChatGPT.dmg 安装（把 ChatGPT 拖入"应用程序"文件夹）。
2. 如果 ChatGPT 应用正在运行，请先完全退出（按 Cmd+Q）。

二、首次打开（解除 macOS 安全提示）
两个 app（$APP_NAME 和 $MANAGER_APP_NAME）都未经 Apple 公证，首次打开都会提示
"Apple 无法验证…"，需要各自按以下步骤解除一次：
1. 双击 app，弹窗中点"完成"（不要点"移到废纸篓"）；
2. 打开 系统设置 → 隐私与安全性，拉到最底部；
3. 在"已阻止 xxx"提示处点"仍要打开"，再确认一次即可。

三、开始使用
1. 双击 $APP_NAME；
2. 首次运行会弹出配置窗口，填入 API 网址、API Key、默认模型等信息；
3. 点击"保存并启动 Codex"；
4. 启动时间较长，请耐心等待，ChatGPT 应用会自动打开。

四、更换皮肤
在 ChatGPT 里打开 Codex++ 增强菜单 → 点击"打开皮肤管理"，会自动启动同目录下的
"$MANAGER_APP_NAME.app"，直接进入"皮肤管理"界面（其余设置项已隐藏，便携版的
供应商/插件等设置只通过 config.ini 配置，不在这里）。

五、其他说明
- 配置完成后，再次双击即可直接启动，不再弹出配置窗口。
- 配置文件统一保存在：
    ~/Library/Application Support/ChatGPT Launcher/config.ini
- 如需修改配置，在终端运行：
    "$APP_NAME.app/Contents/MacOS/$EXECUTABLE_NAME" --config
- $APP_NAME 会在后台驻留（为增强功能提供支持），ChatGPT 退出后它会自动退出。
README

echo "Portable app assembled at $APP_DIR"
echo "Manager app (skin-only) assembled at $OUTPUT_PATH/$MANAGER_APP_NAME.app"
echo "README written to $OUTPUT_PATH/使用说明.txt"
echo "First launch shows the config dialog and creates config.ini next to \"$APP_NAME.app\"."
