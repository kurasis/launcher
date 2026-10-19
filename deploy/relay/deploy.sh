#!/bin/bash
# Installs one release of the relay; run by the deploy user over SSH from the
# Relay service workflow, with the allow list's token on standard input:
#
#   printf 'RELAY_TOKEN=%s\n' … | bash deploy.sh <release-id> <archive>
#
# Unpacks the archive into /home/www/lapi/relay/releases/<release-id>, points
# `current` at it, restarts the relay and checks that it answers; a failed
# check puts the previous release back. The five newest releases are kept.
set -euo pipefail

RELEASE="${1:?release id}"
ARCHIVE="${2:?archive path}"
APP=/home/www/lapi/relay

if [ ! -f "$APP/.bootstrapped" ]; then
    echo "NOT_BOOTSTRAPPED: run relay/bootstrap.sh as root first"
    exit 3
fi
[[ "$RELEASE" =~ ^[0-9a-f]{7,40}$ ]] || { echo "bad release id"; exit 1; }

dir=$APP/releases/$RELEASE
if [ "$(readlink "$APP/current" 2>/dev/null || true)" != "releases/$RELEASE" ]; then
    rm -rf "$dir"
    mkdir -p "$dir"
    tar -xzf "$ARCHIVE" -C "$dir"
    chmod 0750 "$dir" "$dir/lapi-relay"
fi
"$dir/lapi-relay" --version

# Only the token, on one line, nothing that could end it.
umask 027
: > "$APP/secrets.env.new"
while IFS= read -r line || [ -n "$line" ]; do
    [ -z "$line" ] && continue
    if [[ "$line" =~ ^RELAY_TOKEN=[A-Za-z0-9_-]{0,128}$ ]]; then
        [ -n "${line#*=}" ] && printf '%s\n' "$line" >> "$APP/secrets.env.new"
    else
        echo "a secrets line was refused (unknown key or bad characters)"
        rm -f "$APP/secrets.env.new"
        exit 1
    fi
done
if [ ! -s "$APP/secrets.env.new" ]; then
    echo "NO_TOKEN: the RELAY_TOKEN secret is not set"
    rm -f "$APP/secrets.env.new"
    exit 4
fi
chmod 0640 "$APP/secrets.env.new"
mv "$APP/secrets.env.new" "$APP/secrets.env"

previous=$(readlink "$APP/current" 2>/dev/null || true)
ln -sfn "releases/$RELEASE" "$APP/current.new"
mv -T "$APP/current.new" "$APP/current"
sudo -n /usr/bin/systemctl restart lapi-relay

answers() {
    for _ in $(seq 1 30); do
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 http://127.0.0.1/generate_204 || true)
        [ "$code" = 204 ] && return 0
        sleep 1
    done
    return 1
}
if answers; then
    echo "== the relay answers"
else
    echo "the relay did not answer; going back"
    if [ -n "$previous" ] && [ "$previous" != "releases/$RELEASE" ]; then
        ln -sfn "$previous" "$APP/current.new"
        mv -T "$APP/current.new" "$APP/current"
        sudo -n /usr/bin/systemctl restart lapi-relay || true
    fi
    exit 1
fi

ls -1dt "$APP"/releases/* | tail -n +6 | xargs -r rm -rf
