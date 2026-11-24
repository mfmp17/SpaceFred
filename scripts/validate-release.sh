#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
plist="$root/app/Info.plist"
expected_bundle_id=one.frederico.spacefred
expected_executables="spaceman spaceman-agent"
expected_feed_url=https://pub-6f85d683ffe0419cad0a71508904053a.r2.dev/appcast.xml
expected_sparkle_key=mIek27lttJe8cIBqVZFhh6reRKjpTx1h9ZY9OKWPtuM=

fail() {
  echo "release validation failed: $*" >&2
  exit 1
}

plist_value() {
  /usr/bin/plutil -extract "$1" raw "$2"
}

cargo_version=$(awk -F ' *= *' '
  /^\[package\]$/ { package = 1; next }
  /^\[/ { package = 0 }
  package && $1 == "version" { gsub(/"/, "", $2); print $2; exit }
' "$root/Cargo.toml")
plist_version=$(plist_value CFBundleShortVersionString "$plist")
plist_build=$(plist_value CFBundleVersion "$plist")
bundle_id=$(plist_value CFBundleIdentifier "$plist")
bundle_executable=$(plist_value CFBundleExecutable "$plist")
deployment_target=$(plist_value LSMinimumSystemVersion "$plist")
feed_url=$(plist_value SUFeedURL "$plist")
sparkle_key=$(plist_value SUPublicEDKey "$plist")
signed_feed=$(plist_value SURequireSignedFeed "$plist")

[ -n "$cargo_version" ] || fail "Cargo package version is missing"
[ "$cargo_version" = "$plist_version" ] || \
  fail "Cargo version $cargo_version does not match Info.plist version $plist_version"
[ "$bundle_id" = "$expected_bundle_id" ] || \
  fail "unexpected bundle identifier $bundle_id (expected $expected_bundle_id)"
[ "$bundle_executable" = spaceman-agent ] || \
  fail "source Info.plist has an unexpected main executable"
case "$plist_build" in
  ''|*[!0-9]*) fail "CFBundleVersion must be a positive integer" ;;
  0) fail "CFBundleVersion must be greater than zero" ;;
esac
[ -n "$deployment_target" ] || fail "Info.plist does not declare LSMinimumSystemVersion"
[ "$feed_url" = "$expected_feed_url" ] || fail "Info.plist has an unexpected Sparkle feed"
[ "$sparkle_key" = "$expected_sparkle_key" ] || fail "Info.plist has an unexpected Sparkle public key"
[ "$signed_feed" = true ] || fail "Info.plist does not require signed Sparkle feeds"
/usr/bin/plutil -lint "$plist" >/dev/null || fail "source Info.plist is invalid"

if [ "${1:-}" = "--metadata-only" ]; then
  printf 'Metadata valid: version %s, bundle ID %s, declared minimum macOS %s\n' \
    "$cargo_version" "$bundle_id" "$deployment_target"
  exit 0
fi

[ "$#" -eq 2 ] || {
  echo "usage: $0 --metadata-only" >&2
  echo "       $0 /path/to/SpaceFred.app /path/to/SpaceFred.zip" >&2
  exit 2
}

bundle=$1
archive=$2
expected_arch=${SPACEMAN_RELEASE_ARCH:-$(uname -m)}
case "$expected_arch" in
  arm64|x86_64) ;;
  *) fail "unsupported expected architecture '$expected_arch'; set SPACEMAN_RELEASE_ARCH to arm64 or x86_64" ;;
esac

[ -d "$bundle" ] || fail "app bundle not found: $bundle"
[ -f "$archive" ] || fail "ZIP artifact not found: $archive"
/usr/bin/cmp "$plist" "$bundle/Contents/Info.plist" || \
  fail "built bundle Info.plist differs from source Info.plist"
[ -s "$bundle/Contents/Resources/AppIcon.icns" ] || fail "built bundle is missing AppIcon.icns"
[ -s "$bundle/Contents/Resources/Assets.car" ] || fail "built bundle is missing Assets.car"

for executable in $expected_executables; do
  path="$bundle/Contents/MacOS/$executable"
  [ -f "$path" ] && [ -x "$path" ] || fail "missing executable: $executable"
  archs=$(/usr/bin/lipo -archs "$path")
  [ "$archs" = "$expected_arch" ] || \
    fail "$executable architecture is '$archs' (expected '$expected_arch')"
  binary_target=$(/usr/bin/vtool -show-build "$path" | awk '/minos/ { print $2; exit }')
  [ "$binary_target" = "$deployment_target" ] || \
    fail "$executable deployment target is $binary_target (Info.plist declares $deployment_target)"

  signature=$(/usr/bin/codesign --display --verbose=4 "$path" 2>&1) || \
    fail "$executable does not have a readable signature"
  printf '%s\n' "$signature" | grep -q '^Authority=Developer ID Application:' || \
    fail "$executable is not signed with a Developer ID Application identity"
  printf '%s\n' "$signature" | grep -q 'flags=.*runtime' || \
    fail "$executable signature does not enable the hardened runtime"
  printf '%s\n' "$signature" | grep -q '^Timestamp=' || \
    fail "$executable signature has no trusted timestamp"
  /usr/bin/codesign --verify --strict --verbose=2 "$path" || \
    fail "$executable signature verification failed"
done

bundle_signature=$(/usr/bin/codesign --display --verbose=4 "$bundle" 2>&1) || \
  fail "bundle does not have a readable signature"
printf '%s\n' "$bundle_signature" | grep -q '^Authority=Developer ID Application:' || \
  fail "bundle is not signed with a Developer ID Application identity"
printf '%s\n' "$bundle_signature" | grep -q "^Identifier=$expected_bundle_id$" || \
  fail "signed bundle identifier is incorrect"
printf '%s\n' "$bundle_signature" | grep -q 'flags=.*runtime' || \
  fail "bundle signature does not enable the hardened runtime"
printf '%s\n' "$bundle_signature" | grep -q '^Timestamp=' || \
  fail "bundle signature has no trusted timestamp"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$bundle" || \
  fail "bundle signature verification failed"

xcrun stapler validate "$bundle" || fail "bundle has no valid stapled notarization ticket"
/usr/sbin/spctl --assess --type execute --verbose=4 "$bundle" || \
  fail "Gatekeeper rejected the bundle"

/usr/bin/unzip -tq "$archive" >/dev/null || fail "ZIP integrity check failed"
actual_contents=$(mktemp)
extracted=$(mktemp -d)
trap 'rm -f "$actual_contents"; rm -rf "$extracted"' EXIT HUP INT TERM
/usr/bin/unzip -Z1 "$archive" >"$actual_contents"
if grep -Ev '^SpaceFred\.app(/|$)' "$actual_contents" >/dev/null; then
  fail "ZIP contains files outside SpaceFred.app"
fi
for required in \
  SpaceFred.app/Contents/Info.plist \
  SpaceFred.app/Contents/MacOS/spaceman \
  SpaceFred.app/Contents/MacOS/spaceman-agent \
  SpaceFred.app/Contents/Resources/AppIcon.icns \
  SpaceFred.app/Contents/Resources/Assets.car \
  SpaceFred.app/Contents/_CodeSignature/CodeResources
do
  grep -Fxq "$required" "$actual_contents" || fail "ZIP is missing $required"
done
/usr/bin/unzip -qq "$archive" -d "$extracted"
for relative_path in \
  Contents/CodeResources \
  Contents/Info.plist \
  Contents/MacOS/spaceman \
  Contents/MacOS/spaceman-agent \
  Contents/Resources/AppIcon.icns \
  Contents/Resources/Assets.car \
  Contents/_CodeSignature/CodeResources
do
  /usr/bin/cmp "$bundle/$relative_path" "$extracted/SpaceFred.app/$relative_path" || \
    fail "packed $relative_path differs from the validated bundle"
done
/usr/bin/codesign --verify --deep --strict --verbose=2 "$extracted/SpaceFred.app" || \
  fail "packed bundle signature verification failed"
xcrun stapler validate "$extracted/SpaceFred.app" || \
  fail "packed bundle has no valid stapled notarization ticket"

printf 'Release valid: version %s, macOS %s, %s, Developer ID signed, notarized, and Gatekeeper accepted\n' \
  "$cargo_version" "$deployment_target" "$expected_arch"
