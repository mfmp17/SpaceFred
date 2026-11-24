#!/usr/bin/env bash
# sign-macos.sh <unsigned Spaceman.app zip> <out dir>: signs Spaceman.app
# inside-out with the Developer ID identity (hardened runtime, secure
# timestamp): both executables, then the bundle. Notarizes and staples it, and
# writes Spaceman-<version>-macos-arm64.zip (root Spaceman.app) with its
# .sha256. The app is data here: it is never launched.
#
# Environment: SPACEMAN_VERSION (the release's version),
# SPACEMAN_BUNDLE_VERSION (Cargo.toml's, which the bundle carries),
# SPACEMAN_SIGNING_IDENTITY, SPACEMAN_KEYCHAIN, SPACEMAN_NOTARY_KEY,
# SPACEMAN_NOTARY_KEY_ID, SPACEMAN_NOTARY_ISSUER (macos-credentials.sh).
. "$(dirname "$0")/lib.sh"

input=$1
out=$2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
name="SpaceFred-$SPACEMAN_VERSION-macos-arm64.zip"

/usr/bin/ditto -x -k "$input" "$work/unpacked"
app="$work/unpacked/SpaceFred.app"
if [ ! -d "$app" ] || [ "$(find "$work/unpacked" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" != 1 ]; then
  die "The macOS build must hold exactly SpaceFred.app."
fi
plist="$app/Contents/Info.plist"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" = one.frederico.spacefred ] ||
  die "SpaceFred.app has the wrong bundle identifier."
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")" = "$SPACEMAN_BUNDLE_VERSION" ] ||
  die "SpaceFred.app is not version $SPACEMAN_BUNDLE_VERSION."

# Every Mach-O in the bundle must be one this script signs; anything else would
# ship unsigned or unnoticed.
expected=$(printf '%s\n' Contents/MacOS/spaceman Contents/MacOS/spaceman-agent | LC_ALL=C sort)
found=$(cd "$app" && find . -type f -exec sh -c 'file -b "$1" | grep -q Mach-O && printf "%s\n" "${1#./}"' _ {} \; | LC_ALL=C sort)
[ "$found" = "$expected" ] || die "Unexpected code in SpaceFred.app: $(echo "$found" | tr '\n' ' ')"
for executable in spaceman spaceman-agent; do
  [ "$(lipo -archs "$app/Contents/MacOS/$executable")" = arm64 ] || die "$executable is not an arm64 binary."
done

sign() {
  codesign --force --timestamp --options runtime --keychain "$SPACEMAN_KEYCHAIN" \
    --sign "$SPACEMAN_SIGNING_IDENTITY" "$@"
}
sign "$app/Contents/MacOS/spaceman-agent"
sign "$app/Contents/MacOS/spaceman"
sign --identifier one.frederico.spacefred "$app"
codesign --verify --deep --strict --verbose=2 "$app"

# zip, as scripts/release-app.sh always has: symlinks kept, no __MACOSX
# entries, which validate-release.sh refuses.
(cd "$work/unpacked" && /usr/bin/zip -qry --symlinks "$work/notarize.zip" SpaceFred.app)
xcrun notarytool submit "$work/notarize.zip" --key "$SPACEMAN_NOTARY_KEY" --key-id "$SPACEMAN_NOTARY_KEY_ID" \
  --issuer "$SPACEMAN_NOTARY_ISSUER" --wait --timeout 45m --output-format json > "$work/notary.json" || true
cat "$work/notary.json"
status=$(jq -r '.status // empty' "$work/notary.json" 2>/dev/null || true)
if [ "$status" != Accepted ]; then
  id=$(jq -r '.id // empty' "$work/notary.json" 2>/dev/null || true)
  if [ -n "$id" ]; then
    xcrun notarytool log "$id" --key "$SPACEMAN_NOTARY_KEY" --key-id "$SPACEMAN_NOTARY_KEY_ID" \
      --issuer "$SPACEMAN_NOTARY_ISSUER" || true
  fi
  die "Notarization ended with status '${status:-unknown}'."
fi
xcrun stapler staple "$app"
xcrun stapler validate "$app"

out=$(cd "$out" && pwd)
rm -f "$out/$name"
(cd "$work/unpacked" && /usr/bin/zip -qry --symlinks "$out/$name" SpaceFred.app)
(cd "$out" && shasum -a 256 "$name" > "$name.sha256")
ls -l "$out"
