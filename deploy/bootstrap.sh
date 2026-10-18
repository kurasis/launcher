#!/bin/bash
# Root setup of our services on the Debian 13 server: crash.lapi.sh (D-061),
# mirror.lapi.sh (D-062), upd.lapi.sh (D-063), oauth.lapi.sh and
# friends.lapi.sh (D-064).
#
#   sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user>
#
# <deploy-user> is the account in the SSH_USER secret. The workflows upload
# this folder; after this script, they deploy the services without root. Run
# it again when a workflow says the nginx files changed or a new service is
# there: nginx config is root's alone, since whoever writes it can make nginx
# (root) write any file. Safe to run again: every step checks what is there.
#
# For each service folder here (crash/, mirror/, upd/, oauth/, friends/):
#  1. a system user lapi-<service> that runs it;
#  2. /home/www/lapi/<service>/{releases,data}: the deploy user writes
#     releases, the service user alone writes its data; /home/www and
#     /home/www/lapi become root's;
#  3. its systemd units, if it runs as one (upd is static files and a
#     command; the mirror also has a warming timer);
#  4. a Let's Encrypt certificate for <service>.lapi.sh (webroot), with a
#     self-signed one as the fallback, and nginx reloads on renewal;
#  5. its nginx files, with the shared ones from ./nginx, in /etc/nginx/lapi;
#  6. sudo rights for the deploy user: restart/is-active of each service and
#     its command line as its user. Nothing else.
set -euo pipefail

DEPLOY_USER="${1:?usage: sudo bash bootstrap.sh <deploy-user>}"
WWW=/home/www
ROOT=$WWW/lapi
NGINX=/etc/nginx/lapi
TLS=/etc/nginx/lapi-tls
HERE="$(cd "$(dirname "$0")" && pwd)"

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)"; exit 1; }
id "$DEPLOY_USER" >/dev/null || { echo "no user $DEPLOY_USER"; exit 1; }
[ -d "$HERE/nginx" ] || { echo "no $HERE/nginx: re-run a workflow first"; exit 1; }
step() { printf '\n== %s\n' "$*"; }

SERVICES=()
for service in crash mirror upd oauth friends; do
    [ -d "$HERE/$service/nginx" ] && SERVICES+=("$service")
done
# A service with a systemd unit runs as a process.
daemon() { [ -f "$HERE/$1/lapi-$1.service" ]; }
[ "${#SERVICES[@]}" -gt 0 ] || { echo "no service folders in $HERE"; exit 1; }
echo "services: ${SERVICES[*]}"

# Settings per service: folder modes (nginx reads the mirror's kept files;
# crash reports and accounts are private) and the first environment file.
private() { [ "$1" = crash ] || [ "$1" = oauth ] || [ "$1" = friends ]; }
app_mode() { private "$1" && echo 2750 || echo 2755; }
data_mode() { private "$1" && echo 0750 || echo 0755; }
first_env() {
    case "$1" in
        crash) printf 'LAPI_CRASH_ADDR=127.0.0.1:8781\nLAPI_CRASH_DATA=%s/crash/data\nLAPI_CRASH_KEEP_DAYS=90\nLAPI_CRASH_KEEP_STATS_DAYS=400\nLAPI_CRASH_MAX_STORED_MB=5120\n' "$ROOT" ;;
        mirror) printf 'LAPI_MIRROR_ADDR=127.0.0.1:8782\nLAPI_MIRROR_DATA=%s/mirror/data\nLAPI_MIRROR_SETTINGS=%s/mirror/settings.json\n' "$ROOT" "$ROOT" ;;
        oauth) printf 'LAPI_OAUTH_ADDR=127.0.0.1:8783\nLAPI_OAUTH_DATA=%s/oauth/data\nLAPI_OAUTH_URL=https://oauth.lapi.sh\n' "$ROOT" ;;
        friends) printf 'LAPI_FRIENDS_ADDR=127.0.0.1:8784\nLAPI_FRIENDS_DATA=%s/friends/data\nLAPI_FRIENDS_REQUIRE_EMAIL=0\n' "$ROOT" ;;
    esac
}

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
install -d -o root -g root -m 0755 "$ROOT/acme"

for service in "${SERVICES[@]}"; do
    step "service $service"
    user=lapi-$service
    app=$ROOT/$service
    if ! id "$user" >/dev/null 2>&1; then
        useradd --system --home-dir "$app" --no-create-home --shell /usr/sbin/nologin "$user"
    fi
    install -d -o "$DEPLOY_USER" -g "$user" -m "$(app_mode "$service")" "$app"
    install -d -o "$DEPLOY_USER" -g "$user" -m 2750 "$app/releases"
    # A mounted disk keeps its own owner and mode until set here.
    install -d "$app/data"
    chown "$user:$user" "$app/data"
    chmod "$(data_mode "$service")" "$app/data"
    if [ "$service" = upd ]; then
        # Release archives are uploaded here by the deploy user; lapi-upd reads them.
        install -d -o "$DEPLOY_USER" -g "$user" -m 2770 "$app/data/incoming"
    fi
    daemon "$service" || continue
    if [ ! -f "$app/$service.env" ]; then
        first_env "$service" > "$app/$service.env"
    fi
    chown "root:$user" "$app/$service.env"
    chmod 0640 "$app/$service.env"
    install -m 0644 "$HERE/$service/lapi-$service.service" "/etc/systemd/system/lapi-$service.service"
    # Helper units, such as the mirror's warming timer.
    for unit in "$HERE/$service"/lapi-"$service"-*.service "$HERE/$service"/lapi-"$service"-*.timer; do
        [ -f "$unit" ] && install -m 0644 "$unit" "/etc/systemd/system/$(basename "$unit")"
    done
done
systemctl daemon-reload
for service in "${SERVICES[@]}"; do
    daemon "$service" || continue
    systemctl enable "lapi-$service.service"
    for timer in "$HERE/$service"/lapi-"$service"-*.timer; do
        # Started by the first deploy, once there is a release to run.
        [ -f "$timer" ] && systemctl enable "$(basename "$timer")"
    done
done

# Puts the shared nginx files and those of every service with a certificate
# into $NGINX; the files before stay if nginx rejects them.
install_site() {
    local backup service
    backup=$(mktemp -d)
    cp -a "$NGINX/." "$backup/"
    install -m 0644 -o root -g root "$HERE"/nginx/* "$NGINX/"
    for service in "${SERVICES[@]}"; do
        if [ -f "$TLS/$service.lapi.sh.conf" ]; then
            install -m 0644 -o root -g root "$HERE/$service"/nginx/* "$NGINX/"
        else
            # Until there is a certificate: answer the ACME challenge on port 80.
            cat > "$NGINX/$service.conf" <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $service.lapi.sh;
    location /.well-known/acme-challenge/ { root $ROOT/acme; }
    location / { return 503; }
}
EOF
        fi
    done
    if ! nginx -t; then
        echo "nginx rejected the new files; the old ones are back"
        rm -f "$NGINX"/*
        cp -a "$backup/." "$NGINX/"
        rm -rf "$backup"
        return 1
    fi
    rm -rf "$backup"
}

step "nginx"
install -d -o root -g root -m 0755 "$NGINX" "$TLS"
cat > /etc/nginx/conf.d/lapi.conf <<EOF
# Our APIs (D-061, D-062): their files are in $NGINX (root's).
include $NGINX/*.conf;
EOF
# The layout before root kept nginx files: deploys wrote them.
rm -rf "$ROOT/nginx"
install_site
systemctl enable nginx
systemctl reload-or-restart nginx

step "TLS certificates"
apt-get install -y --no-install-recommends certbot openssl >/dev/null
install -d /etc/letsencrypt/renewal-hooks/deploy
printf '#!/bin/sh\nsystemctl reload nginx\n' > /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
for service in "${SERVICES[@]}"; do
    domain=$service.lapi.sh
    tls=$TLS/$domain.conf
    if certbot certonly --webroot -w "$ROOT/acme" -d "$domain" \
            --non-interactive --agree-tos --register-unsafely-without-email --keep-until-expiring; then
        printf 'ssl_certificate /etc/letsencrypt/live/%s/fullchain.pem;\nssl_certificate_key /etc/letsencrypt/live/%s/privkey.pem;\n' \
            "$domain" "$domain" > "$tls"
        echo "$domain: Let's Encrypt certificate in place"
    elif [ ! -f "$tls" ] || grep -q /etc/ssl/lapi/ "$tls"; then
        echo "WARNING: $domain: Let's Encrypt failed (Cloudflare may block the challenge);" \
             "using a self-signed certificate. Cloudflare SSL mode must then be 'Full', not 'Full (strict)'."
        install -d -m 0700 /etc/ssl/lapi
        if [ ! -f "/etc/ssl/lapi/$domain.key" ]; then
            openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$domain" \
                -keyout "/etc/ssl/lapi/$domain.key" -out "/etc/ssl/lapi/$domain.crt" 2>/dev/null
        fi
        printf 'ssl_certificate /etc/ssl/lapi/%s.crt;\nssl_certificate_key /etc/ssl/lapi/%s.key;\n' \
            "$domain" "$domain" > "$tls"
    fi
    chmod 0644 "$tls"
done

step "nginx files of the services"
install_site
systemctl reload nginx

step "sudo rights of $DEPLOY_USER"
sudoers=/etc/sudoers.d/lapi-deploy
{
    echo "# Deploys of $ROOT services by GitHub Actions (D-061 to D-064). Nothing else."
    for service in "${SERVICES[@]}"; do
        if daemon "$service"; then
            echo "$DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart lapi-$service, /usr/bin/systemctl is-active lapi-$service"
        fi
        echo "$DEPLOY_USER ALL=(lapi-$service) NOPASSWD: $ROOT/$service/current/lapi-$service"
        for timer in "$HERE/$service"/lapi-"$service"-*.timer; do
            [ -f "$timer" ] && echo "$DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl start $(basename "$timer")"
        done
    done
} > "$sudoers.tmp"
visudo -cf "$sudoers.tmp"
chmod 0440 "$sudoers.tmp"
mv "$sudoers.tmp" "$sudoers"

for service in "${SERVICES[@]}"; do
    touch "$ROOT/$service/.bootstrapped"
done
step "done"
echo "The workflows deploy the services by themselves from now on."
