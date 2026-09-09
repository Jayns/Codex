# Shared Developer ID signing + Apple notarization helpers for the macOS
# packaging scripts (package-dmg.sh / package-portable.sh).
#
# Everything here is opt-in through environment variables, so the scripts keep
# working with ad-hoc signatures when no Apple credentials are configured
# (local dev, CI without secrets, contributors without a paid account):
#
#   CODEX_MACOS_SIGN_IDENTITY
#       codesign identity string, e.g.
#       "Developer ID Application: Chengdu Shengwei ... (VZUUDKF3MW)".
#       Unset / empty  -> ad-hoc signing ("-"), same as before.
#
#   CODEX_MACOS_ENTITLEMENTS
#       Path to an entitlements plist applied when signing with a real
#       identity. Unset -> hardened runtime with no extra entitlements (what
#       these two self-contained binaries need). A ready-to-use reference file
#       for the WKWebView manager lives next to this script as
#       entitlements-webview.plist; point this at it if the signed manager
#       crashes on launch.
#
#   CODEX_MACOS_NOTARY_PROFILE
#       Name of an `xcrun notarytool store-credentials` keychain profile. When
#       set, finished .dmg / .zip artifacts are submitted to Apple's notary
#       service and the ticket is stapled. Unset -> no notarization.
#
#   CODEX_MACOS_NOTARY_KEYCHAIN
#       Optional keychain holding that profile (passed as
#       `--keychain <path>`); defaults to the login keychain.

CODEX_CODESIGN_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

codex_sign_identity() {
  printf '%s' "${CODEX_MACOS_SIGN_IDENTITY:--}"
}

codex_is_real_identity() {
  [ "$(codex_sign_identity)" != "-" ]
}

codex_entitlements_path() {
  printf '%s' "${CODEX_MACOS_ENTITLEMENTS:-}"
}

# codex_codesign_path <file-or-bundle> [--deep-nested]
#
# Signs one Mach-O file or .app bundle. With a real Developer ID identity the
# hardened runtime is enabled, a secure timestamp is requested, and (for a
# real identity only) the entitlements file is applied when it exists.
codex_codesign_path() {
  local target="$1"
  local identity
  identity="$(codex_sign_identity)"

  local args=(--force --sign "$identity")
  if codex_is_real_identity; then
    args+=(--options runtime --timestamp)
    local entitlements
    entitlements="$(codex_entitlements_path)"
    if [ -n "$entitlements" ] && [ -f "$entitlements" ]; then
      args+=(--entitlements "$entitlements")
    fi
  fi

  codesign "${args[@]}" "$target"
}

# codex_codesign_app <app-dir>
#
# Signs a bundle inside-out: every nested Mach-O under Contents/ first, then
# the bundle itself (required for the hardened runtime / notarization).
codex_codesign_app() {
  local app_dir="$1"

  local executable
  executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app_dir/Contents/Info.plist")"

  # Nested Mach-O (bundled dylibs / helper tools / frameworks), deepest first,
  # before the main executable and the bundle. These hand-assembled bundles
  # normally contain nothing but the one executable, but stay correct if that
  # ever changes.
  local nested
  while IFS= read -r nested; do
    [ -z "$nested" ] && continue
    [ "$nested" = "$app_dir/Contents/MacOS/$executable" ] && continue
    case "$(file -b "$nested" 2>/dev/null)" in
      *Mach-O*) codex_codesign_path "$nested" ;;
    esac
  done < <(
    find "$app_dir/Contents" -type f \
      \( -name '*.dylib' -o -name '*.so' -o -perm -u+x \) -print 2>/dev/null \
      | awk '{ print length, $0 }' | sort -rn | cut -d' ' -f2-
  )

  codex_codesign_path "$app_dir/Contents/MacOS/$executable"
  codex_codesign_path "$app_dir"

  if codex_is_real_identity; then
    codesign --verify --strict --deep "$app_dir"
    echo "signed: $app_dir ($(codex_sign_identity))"
  fi
}

# codex_notarize <artifact> [staple-target]
#
# No-op unless CODEX_MACOS_NOTARY_PROFILE is set. Submits <artifact> (.dmg or
# .zip) to Apple, waits for the result, and staples the ticket onto
# <staple-target> (defaults to <artifact>; a .zip is not stapleable, pass the
# .app it contains).
codex_notarize() {
  local artifact="$1"
  local staple_target="${2:-$1}"

  if [ -z "${CODEX_MACOS_NOTARY_PROFILE:-}" ]; then
    echo "notarize: skipped (CODEX_MACOS_NOTARY_PROFILE unset) for $artifact"
    return 0
  fi

  local notary_args=(--keychain-profile "$CODEX_MACOS_NOTARY_PROFILE" --wait)
  if [ -n "${CODEX_MACOS_NOTARY_KEYCHAIN:-}" ]; then
    notary_args+=(--keychain "$CODEX_MACOS_NOTARY_KEYCHAIN")
  fi

  echo "notarize: submitting $artifact ..."
  xcrun notarytool submit "$artifact" "${notary_args[@]}"

  echo "notarize: stapling $staple_target ..."
  xcrun stapler staple "$staple_target"
  xcrun stapler validate "$staple_target"
}
