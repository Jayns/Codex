# macOS 签名与公证

`package-dmg.sh` 和 `package-portable.sh` 默认用 **ad-hoc 签名**（`codesign --sign -`），
产物能跑但别人下载后会被 Gatekeeper 拦（"已损坏 / 无法验证"）。要出「双击即开」的包，
需要 **Developer ID 签名 + Apple 公证（notarization）**。

两个脚本通过 `scripts/installer/macos/lib-codesign.sh` 共享签名逻辑，全部 **opt-in**：
不设环境变量时行为和以前完全一样。

## 环境变量

| 变量 | 作用 |
| --- | --- |
| `CODEX_MACOS_SIGN_IDENTITY` | codesign 身份串，如 `Developer ID Application: Chengdu Shengwei Evolutionary Intelligent Technology Co., Ltd (VZUUDKF3MW)`。不设 → ad-hoc。设了 → 启用 hardened runtime + 安全时间戳。 |
| `CODEX_MACOS_ENTITLEMENTS` | 签名时附加的 entitlements plist 路径。不设 → 仅 hardened runtime，不加任何 entitlements（这两个自包含二进制够用）。管理工具（WKWebView）如签名后启动崩溃，指向 `scripts/installer/macos/entitlements-webview.plist`。 |
| `CODEX_MACOS_NOTARY_PROFILE` | `xcrun notarytool store-credentials` 保存的钥匙串 profile 名。设了 → 成品 `.dmg` / `.zip` 提交 Apple 公证并 staple 票据。 |
| `CODEX_MACOS_NOTARY_KEYCHAIN` | 存放该 profile 的钥匙串路径（可选，默认登录钥匙串）。 |

## 一次性准备

### 1. Developer ID Application 证书

[developer.apple.com](https://developer.apple.com/account/resources/certificates/list) → Certificates
→ ➕ → **Developer ID Application**（需要 Account Holder 权限，每账号上限 5 张）。
下载 `.cer` 双击导入登录钥匙串，确认它下面挂着私钥。核对：

```bash
security find-identity -v -p codesigning
# 应出现：Developer ID Application: <你的公司名> (VZUUDKF3MW)
```

> 截图里那几张 "Apple Distribution / Development" 是 App Store / TestFlight 用的，
> 直接分发的 Mac app 过不了公证，必须用 Developer ID Application。

### 2. App Store Connect API Key（公证凭据）

[App Store Connect](https://appstoreconnect.apple.com/access/integrations/api) → Users and Access
→ Integrations → Team Keys → ➕，角色选 **Developer**，下载 `AuthKey_XXXXXXXXXX.p8`
（只能下一次）。记下 **Issuer ID** 和 **Key ID**。

存成 notarytool 钥匙串 profile（以后只用 profile 名）：

```bash
xcrun notarytool store-credentials CodexPlusPlus-Notary \
  --key /path/to/AuthKey_XXXXXXXXXX.p8 \
  --key-id XXXXXXXXXX \
  --issuer xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

## 出包

```bash
export CODEX_MACOS_SIGN_IDENTITY="Developer ID Application: <你的公司名> (VZUUDKF3MW)"
export CODEX_MACOS_NOTARY_PROFILE="CodexPlusPlus-Notary"

# 便携版（仅启动器）
cargo build --release -p codex-plus-launcher --bin chatgpt-launcher
bash scripts/installer/macos/package-portable.sh dist/macos/portable-launcher-only \
  --launcher-only --version <x.y.z>

# 便携版（含皮肤管理工具）/ DMG
bash scripts/installer/macos/package-portable.sh dist/macos/portable --build --version <x.y.z>
BINARY_DIR="$PWD/target/release" bash scripts/installer/macos/package-dmg.sh <x.y.z> arm64
```

公证提交后 `notarytool submit --wait` 会阻塞几十秒到几分钟。

## 验证

```bash
# 签名 + hardened runtime
codesign -dvv --entitlements - "dist/.../ChatGPT Launcher.app"
# 票据已 staple
xcrun stapler validate "dist/.../ChatGPT Launcher.app"
# Gatekeeper 放行（Notarized Developer ID）
spctl -a -vvv --type exec "dist/.../ChatGPT Launcher.app"
```

## CI

目前只接了本地流程。要在 GitHub Actions 里出签名包，需要把证书（`.p12` base64）、
证书密码、`.p8`、Key ID、Issuer ID 放进仓库 Secrets，在 `release-assets.yml` 的
macOS job 里 `security import` 建临时钥匙串 + `notarytool store-credentials` 后再调用
上面同一套脚本 —— 环境变量接口不变。
