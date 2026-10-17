#!/usr/bin/env bash
# Package smoke test for a fresh Debian/Ubuntu system (container or VM) as root.
# Usage: scripts/smoke/linux.sh <package.deb> <package.AppImage>
#
# Checks that the .deb installs with only its declared runtime dependencies (no
# Rust, Node or Java), that the installed launcher and the AppImage start under a
# virtual X server and create DATA_ROOT, and that purging the package
# leaves user data in place. A start counts as successful when the process is
# still running after the wait and holds the supervisor lock.
set -euo pipefail

deb=$(realpath "$1")
appimage=$(realpath "$2")
wait_seconds=${SMOKE_WAIT_SECONDS:-20}
data_root="$HOME/.local/share/io.github.kurasis.launcher"

step() { printf '\n== %s\n' "$*"; }

step "System"
grep PRETTY_NAME /etc/os-release
# sed reads all input: with pipefail, head could end ldd with SIGPIPE.
ldd --version | sed -n 1p

step "Install $(basename "$deb") with declared dependencies only"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends "$deb"
apt-get install -y -q --no-install-recommends xvfb xauth dbus
dpkg-query -W -f='${Package} ${Version} depends: ${Depends}\n' kurasis-launcher
for tool in rustc cargo node npm java; do
  if command -v "$tool" >/dev/null; then
    echo "unexpected developer tool on the test system: $tool" >&2
    exit 1
  fi
done
test -x /usr/bin/kurasis-launcher
desktop=$(dpkg -L kurasis-launcher | grep '\.desktop$')
grep -E '^(Name|Exec|Icon|Categories)=' "$desktop"

# Starts a command under a virtual display; succeeds if it is still running after
# the wait and DATA_ROOT holds the supervisor lock.
start_and_check() {
  local label=$1
  shift
  rm -rf "$data_root"
  local status=0
  timeout --signal=TERM --kill-after=10 "$wait_seconds" \
    xvfb-run -a dbus-run-session -- "$@" >"/tmp/smoke-$label.log" 2>&1 || status=$?
  tail -n 20 "/tmp/smoke-$label.log"
  if [ "$status" -ne 124 ]; then
    echo "$label exited early with status $status" >&2
    exit 1
  fi
  test -f "$data_root/locks/supervisor.lock"
  echo "$label: running after ${wait_seconds}s, DATA_ROOT created"
}

step "Start the installed launcher"
start_and_check deb /usr/bin/kurasis-launcher

step "Purging the package keeps user data"
mkdir -p "$data_root/instances/smoke"
echo kept >"$data_root/instances/smoke/sentinel.txt"
# Purge is removal plus configuration files: the strongest uninstall apt offers.
apt-get purge -y -q kurasis-launcher
test ! -e /usr/bin/kurasis-launcher
grep -qx kept "$data_root/instances/smoke/sentinel.txt"
echo "uninstall left $data_root in place"

step "Start the AppImage on the base system plus desktop libraries"
# Remove everything the .deb pulled in, then add only the libraries that
# linuxdeploy deliberately leaves to the host (its excludelist): every desktop
# installation provides them. The X libraries come with xvfb.
apt-mark manual xvfb xauth dbus >/dev/null
apt-get autoremove --purge -y -q
apt-get install -y -q --no-install-recommends \
  libegl1 libgl1 libgles2 libfontconfig1 libfribidi0 libgbm1 libharfbuzz0b \
  libwayland-client0
apt-get install -y -q --no-install-recommends libasound2t64 ||
  apt-get install -y -q --no-install-recommends libasound2
chmod +x "$appimage"
# No FUSE in containers: extract-and-run.
APPIMAGE_EXTRACT_AND_RUN=1 start_and_check appimage "$appimage"

step "Linux package smoke test passed"
