#!/usr/bin/env bash
set -euo pipefail

# Assembles a self-contained macOS portable distribution:
#
#   <OutputDir>/
#     ChatGPT Launcher.app/   double-click to run; config dialog on first launch
#
# Unlike the DMG installer (package-dmg.sh), this does not register the app
# anywhere; the .app (and the config.ini created next to it on first run) can
# be moved to another machine and run as-is. Unlike the Windows portable
# build (package-portable.ps1), macOS never bundles its own copy of Codex
# App: the "ChatGPT App 路径" defaults to whatever Codex.app is already
# installed under /Applications or ~/Applications (see
# codex_plus_core::app_paths::find_macos_codex_app_default).
#
# Packaging as a proper .app (rather than shipping the loose `codex`
# Mach-O binary) matters because macOS auto-opens a Terminal window to run
# an un-bundled Unix executable when it's double-clicked in Finder; a
# bundled .app with LSUIElement runs directly with no Terminal window.
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

if [ "$BUILD" -eq 1 ]; then
  (cd "$ROOT" && cargo build --release -p codex-plus-launcher --bin chatgpt-launcher)
fi

if [ ! -x "$BINARY_PATH" ]; then
  echo "error: built binary not found at $BINARY_PATH." >&2
  echo "Pass --build, or build it manually first: cargo build --release -p codex-plus-launcher --bin chatgpt-launcher" >&2
  exit 1
fi

OUTPUT_PATH="$ROOT/$OUTPUT_DIR"
APP_DIR="$OUTPUT_PATH/$APP_NAME.app"
ICON_NAME="codex-portable.icns"

mkdir -p "$OUTPUT_PATH"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$BINARY_PATH" "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"
chmod +x "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"

if [ -f "$ICON_SOURCE_ICO" ] && command -v iconutil >/dev/null 2>&1; then
  ICON_WORKDIR="$(mktemp -d)"
  # `sips` can't scale directly from a multi-image .ico past its largest
  # embedded frame (fails scaling up to 1024x1024), so extract the largest
  # embedded frame (256x256) to a plain PNG first and scale every iconset
  # size from that.
  ICON_SOURCE_PNG="$ICON_WORKDIR/codex-app-icon.png"
  sips -s format png "$ICON_SOURCE_ICO" --out "$ICON_SOURCE_PNG" >/dev/null

  ICONSET_DIR="$ICON_WORKDIR/codex-portable.iconset"
  mkdir -p "$ICONSET_DIR"
  sips -z 16 16 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_16x16.png" >/dev/null
  sips -z 32 32 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_32x32.png" >/dev/null
  sips -z 64 64 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_128x128.png" >/dev/null
  sips -z 256 256 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_256x256.png" >/dev/null
  sips -z 512 512 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_512x512.png" >/dev/null
  sips -z 1024 1024 "$ICON_SOURCE_PNG" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$ICONSET_DIR" -o "$APP_DIR/Contents/Resources/$ICON_NAME"
  rm -rf "$ICON_WORKDIR"
fi

printf 'APPL????' > "$APP_DIR/Contents/PkgInfo"
cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
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
  <string>$EXECUTABLE_NAME</string>
  <key>CFBundleIconFile</key>
  <string>$ICON_NAME</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
PLIST

# Ad-hoc sign so Gatekeeper doesn't flag the freshly-built bundle as damaged.
codesign --force --sign - "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"
codesign --force --sign - "$APP_DIR"

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
应用未经 Apple 公证，首次打开会提示"Apple 无法验证…"，按以下步骤解除：
1. 双击 app，弹窗中点"完成"（不要点"移到废纸篓"）；
2. 打开 系统设置 → 隐私与安全性，拉到最底部；
3. 在"已阻止 $APP_NAME"提示处点"仍要打开"，再确认一次即可。

三、开始使用
1. 双击 $APP_NAME；
2. 首次运行会弹出配置窗口，填入 API 网址、API Key、默认模型等信息；
3. 点击"保存并启动 Codex"；
4. 启动时间较长，请耐心等待，ChatGPT 应用会自动打开。

四、其他说明
- 配置完成后，再次双击即可直接启动，不再弹出配置窗口。
- 配置文件统一保存在：
    ~/Library/Application Support/ChatGPT Launcher/config.ini
- 如需修改配置，在终端运行：
    "$APP_NAME.app/Contents/MacOS/$EXECUTABLE_NAME" --config
- $APP_NAME 会在后台驻留（为增强功能提供支持），ChatGPT 退出后它会自动退出。
README

echo "Portable app assembled at $APP_DIR"
echo "README written to $OUTPUT_PATH/使用说明.txt"
echo "First launch shows the config dialog and creates config.ini next to \"$APP_NAME.app\"."
