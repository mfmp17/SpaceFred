#!/usr/bin/env bash
# Spaceman live gauntlet: runs a real Spaceman.app against disposable windows and checks every step
# against the window server and Spaceman's own state (docs/development.md › Live gauntlet).
#
#   scripts/gauntlet.sh [--app PATH] [--out DIR] [--host SSH_TARGET] [--only LIST] [--reflow-budget-ms N]
#
# --app     SpaceFred.app to test (default target/SpaceFred.app from scripts/build-app.sh).
# --out     receipt directory (default /tmp/spaceman-gauntlet-<timestamp>).
# --host    run on another Mac over ssh: copies both apps there, runs, and copies the receipt back.
#           GAUNTLET_SSH replaces the ssh command (for example "ssh -i KEY -J JUMP").
# --only    comma-separated scenarios: switch,focus,reflow,float,scratchpad,crash. boot and recovery
#           always run.
#
# The target Mac needs a logged-in desktop, Accessibility granted to that Spaceman.app (grants follow
# the Developer ID signature, so one grant covers later builds), and no other window manager or
# Spaceman running. Windows appear on its main display. State and config live in the receipt
# directory; the user's own Spaceman state is never read or written.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/SpaceFred.app"
out=""
host=""
forward=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) app="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --host) host="$2"; shift 2 ;;
    --only|--reflow-budget-ms) forward+=("$1" "$2"); shift 2 ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
out="${out:-/tmp/spaceman-gauntlet-$(date +%Y%m%d-%H%M%S)}"
[[ -x "$app/Contents/MacOS/spaceman-agent" ]] || {
  echo "no SpaceFred.app at $app; build it with scripts/build-app.sh" >&2
  exit 2
}

# The harness is one Swift binary in a small app bundle, so each test window is its own app.
harness="$root/target/gauntlet/SpacemanGauntlet.app"
sources=("$root"/scripts/gauntlet/*.swift)
binary="$harness/Contents/MacOS/gauntlet"
if [[ ! -x "$binary" ]] || [[ -n "$(find "${sources[@]}" -newer "$binary" 2>/dev/null)" ]]; then
  rm -rf "$harness"
  mkdir -p "$harness/Contents/MacOS"
  swiftc -O -swift-version 5 -target "$(uname -m)-apple-macos14.0" -o "$binary" "${sources[@]}"
  cat >"$harness/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>anomaly.spaceman.gauntlet</string>
  <key>CFBundleName</key><string>Spaceman Gauntlet</string>
  <key>CFBundleExecutable</key><string>gauntlet</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
  codesign --force --sign - "$harness" >/dev/null
fi

if [[ -z "$host" ]]; then
  exec "$binary" run --app "$app" --out "$out" ${forward[@]+"${forward[@]}"}
fi

read -r -a ssh_command <<<"${GAUNTLET_SSH:-ssh}"
remote() { "${ssh_command[@]}" "$host" "$@"; }
remote_dir="$(remote 'mktemp -d /tmp/spaceman-gauntlet.XXXXXX')"
[[ "$remote_dir" == /tmp/spaceman-gauntlet.* ]] || { echo "could not make a remote directory" >&2; exit 1; }
COPYFILE_DISABLE=1 tar -C "$(dirname "$app")" -cf - "$(basename "$app")" \
  -C "$(dirname "$harness")" SpacemanGauntlet.app | remote "tar -C '$remote_dir' -xf -"
status=0
remote "'$remote_dir/SpacemanGauntlet.app/Contents/MacOS/gauntlet' run --app '$remote_dir/$(basename "$app")' --out '$remote_dir/out' ${forward[*]+${forward[*]}}" \
  || status=$?
mkdir -p "$out"
remote "tar -C '$remote_dir/out' -cf - ." | tar -C "$out" -xf - || status=1
remote "rm -rf '$remote_dir'" || true
echo "receipt: $out/receipt.json"
exit "$status"
