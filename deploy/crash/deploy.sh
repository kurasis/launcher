#!/bin/bash
# Installs one release of the crash service; run by the deploy user over SSH
# from the "Crash service" workflow (D-061).
#
#   bash deploy.sh <release-id> <archive>
#
# Unpacks the archive into releases/<release-id>, points `current` at the
# release, restarts the service and checks /v1/health. A failed check puts the
# previous release back. The five newest releases are kept. nginx files are
# root's (bootstrap.sh installs them); this only says when they differ.
set -euo pipefail

RELEASE="${1:?release id}"
ARCHIVE="${2:?archive path}"
ROOT=/home/www/lapi
APP=$ROOT/crash

if [ ! -f "$APP/.bootstrapped" ]; then
    echo "NOT_BOOTSTRAPPED: run deploy/crash/bootstrap.sh once as root first"
    exit 3
fi
[[ "$RELEASE" =~ ^[0-9a-f]{7,40}$ ]] || { echo "bad release id"; exit 1; }

dir=$APP/releases/$RELEASE
rm -rf "$dir"
mkdir -p "$dir"
tar -xzf "$ARCHIVE" -C "$dir"
chmod 0750 "$dir" "$dir/lapi-crash"
"$dir/lapi-crash" --version

health() {
    for _ in $(seq 1 20); do
        if curl -fsS --max-time 2 "$1" -H "Host: crash.lapi.sh" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

echo "== nginx files"
if [ ! -f /etc/nginx/lapi/crash.conf ]; then
    echo "NGINX_PENDING: nginx still uses the old layout"
else
    for file in "$dir"/nginx/*; do
        if ! cmp -s "$file" "/etc/nginx/lapi/$(basename "$file")"; then
            echo "NGINX_CHANGED: $(basename "$file") differs from the server's"
        fi
    done
fi

echo "== switching to $RELEASE"
previous=$(readlink "$APP/current" 2>/dev/null || true)
ln -sfn "releases/$RELEASE" "$APP/current.new"
mv -T "$APP/current.new" "$APP/current"
sudo -n /usr/bin/systemctl restart lapi-crash
if ! health http://127.0.0.1:8781/v1/health; then
    echo "the new release did not answer"
    if [ -n "$previous" ]; then
        ln -sfn "$previous" "$APP/current.new"
        mv -T "$APP/current.new" "$APP/current"
        sudo -n /usr/bin/systemctl restart lapi-crash
        echo "put $previous back"
    fi
    exit 1
fi
health http://127.0.0.1/v1/health || { echo "nginx does not reach the service"; exit 1; }
echo "== healthy: $(curl -fsS http://127.0.0.1:8781/v1/health)"

# Keep the five newest releases, never the current one.
current=$(readlink "$APP/current")
ls -1t "$APP/releases" | tail -n +6 | while read -r old; do
    [ "releases/$old" = "$current" ] || rm -rf "${APP:?}/releases/$old"
done
rm -f "$ARCHIVE"
