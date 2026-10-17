#!/usr/bin/env bash
# Package smoke test for a macOS DMG.
# Usage: scripts/smoke/macos.sh <package.dmg>
#
# Mounts the image, copies the app out as a user would, reports its architecture
# and signature, starts it and waits until it holds the supervisor lock. Deleting
# the app bundle (macOS "uninstall") must leave DATA_ROOT in place.
set -euo pipefail

dmg=$1
wait_seconds=${SMOKE_WAIT_SECONDS:-20}
data_root="$HOME/Library/Application Support/app.launcher"
apps=$(mktemp -d)
mount_point=$(mktemp -d)

step() { printf '\n== %s\n' "$*"; }

step "System"
sw_vers
uname -m

step "Mount $(basename "$dmg") and copy the app"
hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mount_point" >/dev/null
cp -R "$mount_point/Launcher.app" "$apps/"
hdiutil detach "$mount_point" >/dev/null
app="$apps/Launcher.app"
binary="$app/Contents/MacOS/launcher"
file "$binary"
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$app/Contents/Info.plist"
# Reports the signature state; an unsigned or ad-hoc app is expected without
# owner-provided signing credentials.
codesign -dv "$app" 2>&1 | grep -E '^(Identifier|Signature|TeamIdentifier|Authority)' || true
spctl --assess --type execute "$app" 2>&1 || true

step "Start the app"
rm -rf "$data_root"
"$binary" >/tmp/smoke-macos.log 2>&1 &
pid=$!
sleep "$wait_seconds"
if ! kill -0 "$pid" 2>/dev/null; then
  cat /tmp/smoke-macos.log
  echo "app exited early" >&2
  exit 1
fi
test -f "$data_root/locks/supervisor.lock"
echo "running after ${wait_seconds}s, DATA_ROOT created"
kill "$pid"
wait "$pid" || true

step "Deleting the app keeps user data"
mkdir -p "$data_root/instances/smoke"
echo kept >"$data_root/instances/smoke/sentinel.txt"
rm -rf "$app"
grep -qx kept "$data_root/instances/smoke/sentinel.txt"
echo "app removed, $data_root left in place"

step "macOS package smoke test passed"
