#!/bin/bash
# Installs one release of a service; run by the deploy user over SSH from the
# service's workflow (D-061, D-062).
#
#   bash deploy.sh <service> <release-id> <archive> [mirror-quota-gb]
#
# Unpacks the archive into /home/www/lapi/<service>/releases/<release-id>,
# points `current` at it, restarts the service and checks /v1/health; a failed
# check puts the previous release back. The five newest releases are kept.
# nginx files are root's (bootstrap.sh installs them); this only says when
# they differ. For the mirror, the quota goes to settings.json, which the
# service reads every minute.
set -euo pipefail

SERVICE="${1:?service}"
RELEASE="${2:?release id}"
ARCHIVE="${3:?archive path}"
QUOTA_GB="${4:-}"
case "$SERVICE" in
    crash) PORT=8781 ;;
    mirror) PORT=8782 ;;
    *) echo "unknown service $SERVICE"; exit 1 ;;
esac
APP=/home/www/lapi/$SERVICE

if [ ! -f "$APP/.bootstrapped" ]; then
    echo "NOT_BOOTSTRAPPED: run bootstrap.sh as root first"
    exit 3
fi
[[ "$RELEASE" =~ ^[0-9a-f]{7,40}$ ]] || { echo "bad release id"; exit 1; }

dir=$APP/releases/$RELEASE
rm -rf "$dir"
mkdir -p "$dir"
tar -xzf "$ARCHIVE" -C "$dir"
chmod 0750 "$dir" "$dir/lapi-$SERVICE"
"$dir/lapi-$SERVICE" --version

if [ "$SERVICE" = mirror ] && [ -n "$QUOTA_GB" ]; then
    [[ "$QUOTA_GB" =~ ^[0-9]{1,6}$ ]] || { echo "bad quota $QUOTA_GB"; exit 1; }
    printf '{"maxGb": %s}\n' "$QUOTA_GB" > "$APP/settings.json.new"
    chmod 0644 "$APP/settings.json.new"
    mv "$APP/settings.json.new" "$APP/settings.json"
    echo "== quota: $QUOTA_GB GB"
fi

health() {
    for _ in $(seq 1 20); do
        if curl -fsS --max-time 2 "$1" -H "Host: $SERVICE.lapi.sh" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

echo "== nginx files"
if [ ! -f "/etc/nginx/lapi/$SERVICE.conf" ] || ! grep -q "listen 443" "/etc/nginx/lapi/$SERVICE.conf"; then
    echo "NGINX_PENDING: nginx does not have this service's files yet"
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
sudo -n /usr/bin/systemctl restart "lapi-$SERVICE"
if ! health "http://127.0.0.1:$PORT/v1/health"; then
    echo "the new release did not answer"
    if [ -n "$previous" ]; then
        ln -sfn "$previous" "$APP/current.new"
        mv -T "$APP/current.new" "$APP/current"
        sudo -n /usr/bin/systemctl restart "lapi-$SERVICE"
        echo "put $previous back"
    fi
    exit 1
fi
if grep -q "listen 443" "/etc/nginx/lapi/$SERVICE.conf" 2>/dev/null || [ "$SERVICE" = crash ]; then
    health http://127.0.0.1/v1/health || { echo "nginx does not reach the service"; exit 1; }
fi
echo "== healthy: $(curl -fsS "http://127.0.0.1:$PORT/v1/health")"
if [ "$SERVICE" = mirror ]; then
    # The warming timer runs once there is a release; starting it again is harmless.
    sudo -n /usr/bin/systemctl start lapi-mirror-warm.timer || echo "WARM_TIMER: not started (run bootstrap.sh)"
fi

# Keep the five newest releases, never the current one.
current=$(readlink "$APP/current")
ls -1t "$APP/releases" | tail -n +6 | while read -r old; do
    [ "releases/$old" = "$current" ] || rm -rf "${APP:?}/releases/$old"
done
rm -f "$ARCHIVE"
