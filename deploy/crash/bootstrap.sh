#!/bin/bash
# Root setup of the crash service on the Debian 13 server (D-061).
#
#   sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user>
#
# <deploy-user> is the account in the SSH_USER secret. After this, the
# "Crash service" workflow deploys the service without root. Run it again when
# the workflow says the nginx files changed: nginx config is root's alone,
# since whoever writes it can make nginx (root) write any file. Safe to run
# again: every step checks what is already there.
#
# What it does:
#  1. creates the system user `lapi-crash` that runs the service;
#  2. makes /home/www and /home/www/lapi root's (others read only) and creates
#     /home/www/lapi/{crash/{releases,data},acme}: the deploy user writes
#     releases, the service user alone writes its data;
#  3. installs the systemd unit lapi-crash.service;
#  4. gets a Let's Encrypt certificate for crash.lapi.sh (webroot), with a
#     self-signed one as the fallback, and reloads nginx on renewal;
#  5. installs the nginx files from ./nginx into /etc/nginx/lapi (root's) and
#     makes nginx include them;
#  6. lets the deploy user run exactly: restart/is-active lapi-crash, and the
#     service's command line as lapi-crash.
set -euo pipefail

DEPLOY_USER="${1:?usage: sudo bash bootstrap.sh <deploy-user>}"
DOMAIN=crash.lapi.sh
WWW=/home/www
ROOT=$WWW/lapi
APP=$ROOT/crash
NGINX=/etc/nginx/lapi
HERE="$(cd "$(dirname "$0")" && pwd)"

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)"; exit 1; }
id "$DEPLOY_USER" >/dev/null || { echo "no user $DEPLOY_USER"; exit 1; }
[ -f "$HERE/nginx/crash.conf" ] || { echo "no $HERE/nginx: re-run the workflow first"; exit 1; }
step() { printf '\n== %s\n' "$*"; }

# Puts ./nginx into $NGINX; the files before stay if nginx rejects them.
install_site() {
    local backup
    backup=$(mktemp -d)
    cp -a "$NGINX/." "$backup/"
    install -m 0644 -o root -g root "$HERE"/nginx/* "$NGINX/"
    if ! nginx -t; then
        echo "nginx rejected the new files; the old ones are back"
        rm -f "$NGINX"/*
        cp -a "$backup/." "$NGINX/"
        rm -rf "$backup"
        return 1
    fi
    rm -rf "$backup"
}

step "service user"
if ! id lapi-crash >/dev/null 2>&1; then
    useradd --system --home-dir "$APP" --no-create-home --shell /usr/sbin/nologin lapi-crash
fi

step "folders under $ROOT"
# Every folder on the way to what root runs or reads must be root's: an owner
# of a parent could rename the folder and put their own files in its place.
for dir in "$WWW" "$ROOT"; do
    install -d "$dir"
    if [ "$(stat -c %U:%a "$dir")" != "root:755" ]; then
        echo "$dir was $(stat -c %U:%G:%a "$dir"); now root:root 755"
        chown root:root "$dir"
        chmod 0755 "$dir"
    fi
done
install -d -o "$DEPLOY_USER" -g lapi-crash -m 2750 "$APP" "$APP/releases"
install -d -o lapi-crash -g lapi-crash -m 0750 "$APP/data"
install -d -o root -g root -m 0755 "$ROOT/acme"
if [ ! -f "$APP/crash.env" ]; then
    cat > "$APP/crash.env" <<EOF
LAPI_CRASH_ADDR=127.0.0.1:8781
LAPI_CRASH_DATA=$APP/data
LAPI_CRASH_KEEP_DAYS=90
LAPI_CRASH_KEEP_STATS_DAYS=400
LAPI_CRASH_MAX_STORED_MB=5120
EOF
fi
chown root:lapi-crash "$APP/crash.env"
chmod 0640 "$APP/crash.env"

step "systemd unit"
install -m 0644 "$HERE/lapi-crash.service" /etc/systemd/system/lapi-crash.service
systemctl daemon-reload
systemctl enable lapi-crash.service

step "nginx include"
install -d -o root -g root -m 0755 "$NGINX" /etc/nginx/lapi-tls
cat > /etc/nginx/conf.d/lapi.conf <<EOF
# Our APIs (D-061): each one adds its files to $NGINX (root's).
include $NGINX/*.conf;
EOF
tls=/etc/nginx/lapi-tls/$DOMAIN.conf
if [ -f "$tls" ]; then
    install_site
else
    # Until there is a certificate: answer the ACME challenge on port 80.
    cat > "$NGINX/crash.conf" <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $DOMAIN;
    location /.well-known/acme-challenge/ { root $ROOT/acme; }
    location / { return 503; }
}
EOF
fi
# The layout before root kept nginx files: deploys wrote them.
rm -rf "$ROOT/nginx"
nginx -t
systemctl enable nginx
systemctl reload-or-restart nginx

step "TLS certificate"
apt-get install -y --no-install-recommends certbot openssl >/dev/null
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

step "nginx files of the crash service"
install_site
systemctl reload nginx

step "sudo rights of $DEPLOY_USER"
sudoers=/etc/sudoers.d/lapi-deploy
cat > "$sudoers.tmp" <<EOF
# Deploys of $ROOT services by GitHub Actions (D-061). Nothing else.
$DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart lapi-crash, /usr/bin/systemctl is-active lapi-crash
$DEPLOY_USER ALL=(lapi-crash) NOPASSWD: $APP/current/lapi-crash
EOF
visudo -cf "$sudoers.tmp"
chmod 0440 "$sudoers.tmp"
mv "$sudoers.tmp" "$sudoers"

touch "$APP/.bootstrapped"
step "done"
echo "The workflow deploys the service by itself from now on."
