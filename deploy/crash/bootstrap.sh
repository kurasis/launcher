#!/bin/bash
# One-time root setup of the crash service on the Debian 13 server (D-061).
#
#   sudo bash bootstrap.sh <deploy-user>
#
# <deploy-user> is the account in the SSH_USER secret. After this, the
# "Crash service" workflow deploys without root. Safe to run again: every step
# checks what is already there.
#
# What it does:
#  1. creates the system user `lapi-crash` that runs the service;
#  2. creates /home/www/lapi/{crash/{releases,data},nginx,acme} with owners and
#     modes: the deploy user writes releases and nginx snippets, the service
#     user alone writes its data;
#  3. installs the systemd unit lapi-crash.service;
#  4. makes nginx include /home/www/lapi/nginx/*.conf and starts nginx;
#  5. gets a Let's Encrypt certificate for crash.lapi.sh (webroot), with a
#     self-signed one as the fallback, and reloads nginx on renewal;
#  6. lets the deploy user run exactly: restart/is-active lapi-crash,
#     nginx -t, reload nginx, and the service's command line as lapi-crash.
set -euo pipefail

DEPLOY_USER="${1:?usage: sudo bash bootstrap.sh <deploy-user>}"
DOMAIN=crash.lapi.sh
ROOT=/home/www/lapi
APP=$ROOT/crash
HERE="$(cd "$(dirname "$0")" && pwd)"

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)"; exit 1; }
id "$DEPLOY_USER" >/dev/null || { echo "no user $DEPLOY_USER"; exit 1; }
step() { printf '\n== %s\n' "$*"; }

step "service user"
if ! id lapi-crash >/dev/null 2>&1; then
    useradd --system --home-dir "$APP" --no-create-home --shell /usr/sbin/nologin lapi-crash
fi

step "folders under $ROOT"
install -d -m 0755 "$ROOT"
install -d -o "$DEPLOY_USER" -g lapi-crash -m 2750 "$APP" "$APP/releases"
install -d -o lapi-crash -g lapi-crash -m 0750 "$APP/data"
install -d -o "$DEPLOY_USER" -g "$DEPLOY_USER" -m 0755 "$ROOT/nginx"
install -d -o root -g root -m 0755 "$ROOT/acme"
if [ ! -f "$APP/crash.env" ]; then
    cat > "$APP/crash.env" <<EOF
LAPI_CRASH_ADDR=127.0.0.1:8781
LAPI_CRASH_DATA=$APP/data
LAPI_CRASH_KEEP_DAYS=90
LAPI_CRASH_KEEP_STATS_DAYS=400
EOF
fi
chown root:lapi-crash "$APP/crash.env"
chmod 0640 "$APP/crash.env"

step "systemd unit"
install -m 0644 "$HERE/lapi-crash.service" /etc/systemd/system/lapi-crash.service
systemctl daemon-reload
systemctl enable lapi-crash.service

step "nginx"
install -d -m 0755 /etc/nginx/lapi-tls
cat > /etc/nginx/conf.d/lapi.conf <<EOF
# Our APIs under $ROOT (D-061): each one adds its server blocks there.
include $ROOT/nginx/*.conf;
EOF
# Until a release brings the real config: answer the ACME challenge on port 80.
if [ ! -f "$ROOT/nginx/crash.conf" ]; then
    cat > "$ROOT/nginx/crash.conf" <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $DOMAIN;
    location /.well-known/acme-challenge/ { root $ROOT/acme; }
    location / { return 503; }
}
EOF
    chown "$DEPLOY_USER:$DEPLOY_USER" "$ROOT/nginx/crash.conf"
fi
nginx -t
systemctl enable nginx
systemctl restart nginx

step "TLS certificate"
apt-get install -y --no-install-recommends certbot openssl >/dev/null
tls=/etc/nginx/lapi-tls/$DOMAIN.conf
if certbot certonly --webroot -w "$ROOT/acme" -d "$DOMAIN" \
        --non-interactive --agree-tos --register-unsafely-without-email --keep-until-expiring; then
    printf 'ssl_certificate /etc/letsencrypt/live/%s/fullchain.pem;\nssl_certificate_key /etc/letsencrypt/live/%s/privkey.pem;\n' \
        "$DOMAIN" "$DOMAIN" > "$tls"
    install -d /etc/letsencrypt/renewal-hooks/deploy
    printf '#!/bin/sh\nsystemctl reload nginx\n' > /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
    chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
    echo "Let's Encrypt certificate in place"
else
    echo "WARNING: Let's Encrypt failed (Cloudflare may block the challenge);" \
         "using a self-signed certificate. Cloudflare SSL mode must then be 'Full', not 'Full (strict)'."
    install -d -m 0700 /etc/ssl/lapi
    if [ ! -f "/etc/ssl/lapi/$DOMAIN.key" ]; then
        openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$DOMAIN" \
            -keyout "/etc/ssl/lapi/$DOMAIN.key" -out "/etc/ssl/lapi/$DOMAIN.crt" 2>/dev/null
    fi
    printf 'ssl_certificate /etc/ssl/lapi/%s.crt;\nssl_certificate_key /etc/ssl/lapi/%s.key;\n' \
        "$DOMAIN" "$DOMAIN" > "$tls"
fi
chmod 0644 "$tls"

step "sudo rights of $DEPLOY_USER"
sudoers=/etc/sudoers.d/lapi-deploy
cat > "$sudoers.tmp" <<EOF
# Deploys of $ROOT services by GitHub Actions (D-061). Nothing else.
$DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart lapi-crash, /usr/bin/systemctl is-active lapi-crash, /usr/sbin/nginx -t, /usr/bin/systemctl reload nginx
$DEPLOY_USER ALL=(lapi-crash) NOPASSWD: $APP/current/lapi-crash
EOF
visudo -cf "$sudoers.tmp"
chmod 0440 "$sudoers.tmp"
mv "$sudoers.tmp" "$sudoers"

touch "$APP/.bootstrapped"
step "done"
echo "Now re-run the 'Crash service' workflow (or push to main) to deploy."
