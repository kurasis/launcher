#!/bin/bash
# Root setup of relay.lapi.sh on its own server (D-068, plan W6), once:
#
#   sudo bash ~/lapi-bootstrap/relay/bootstrap.sh <deploy-user>
#
# <deploy-user> is the account in the RELAY_SSH_USER secret; the Relay service
# workflow uploads this folder there. Afterwards it deploys by itself, without
# root. Safe to run again: every step checks what is there.
#
#  1. a system user lapi-relay that runs it;
#  2. /home/www/lapi/relay/{releases,data}: the deploy user writes releases,
#     lapi-relay alone writes its data (the certificate cache);
#  3. relay.env with the defaults, and its systemd unit;
#  4. sudo rights for the deploy user: restarting lapi-relay. Nothing else.
#
# The relay answers on 80/tcp (plain HTTP, captive portal checks), 443/tcp
# (the relay, with a Let's Encrypt certificate it fetches itself) and
# 7842/udp (QUIC address discovery). If the server has a firewall, open
# those three. The DNS record relay.lapi.sh points here directly, not
# through Cloudflare: the relay needs its own TLS and UDP.
set -euo pipefail

DEPLOY_USER="${1:?usage: sudo bash bootstrap.sh <deploy-user>}"
WWW=/home/www
ROOT=$WWW/lapi
APP=$ROOT/relay
HERE="$(cd "$(dirname "$0")" && pwd)"

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)"; exit 1; }
id "$DEPLOY_USER" >/dev/null || { echo "no user $DEPLOY_USER"; exit 1; }
step() { printf '\n== %s\n' "$*"; }

step "folders under $APP"
# Every folder on the way to what root runs must be root's.
for dir in "$WWW" "$ROOT"; do
    install -d "$dir"
    if [ "$(stat -c %U:%a "$dir")" != "root:755" ]; then
        chown root:root "$dir"
        chmod 0755 "$dir"
    fi
done
if ! id lapi-relay >/dev/null 2>&1; then
    useradd --system --home-dir "$APP" --no-create-home --shell /usr/sbin/nologin lapi-relay
fi
install -d -o "$DEPLOY_USER" -g lapi-relay -m 2750 "$APP" "$APP/releases"
install -d "$APP/data"
chown lapi-relay:lapi-relay "$APP/data"
chmod 0700 "$APP/data"

step "settings and unit"
if [ ! -f "$APP/relay.env" ]; then
    printf 'RELAY_DOMAIN=relay.lapi.sh\nRELAY_DATA=%s/data\nRELAY_ALLOW_URL=https://worlds.lapi.sh/v1/relay/allow\n' "$APP" \
        > "$APP/relay.env"
fi
chown "root:lapi-relay" "$APP/relay.env"
chmod 0640 "$APP/relay.env"
install -m 0644 "$HERE/lapi-relay.service" /etc/systemd/system/lapi-relay.service
systemctl daemon-reload
systemctl enable lapi-relay.service
if systemctl is-active --quiet lapi-relay; then
    systemctl restart lapi-relay || true
fi

step "sudo rights of $DEPLOY_USER"
sudoers=/etc/sudoers.d/lapi-relay-deploy
{
    echo "# Deploys of the relay by GitHub Actions (D-068). Nothing else."
    echo "$DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart lapi-relay"
} > "$sudoers.tmp"
visudo -cf "$sudoers.tmp"
chmod 0440 "$sudoers.tmp"
mv "$sudoers.tmp" "$sudoers"

touch "$APP/.bootstrapped"
step "done"
echo "The Relay service workflow deploys the relay by itself from now on."
