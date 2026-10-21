#!/bin/bash
# Checks the nginx files of our services (D-061 to D-069) the way the server's
# nginx reads them, in the official nginx image; run by the workflows.
#
#   bash deploy/check-nginx.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

# Both Cloudflare lists must name the same ranges.
real=$(grep -oE '^set_real_ip_from [^;]+' "$HERE/nginx/cloudflare-real-ip.inc" | cut -d' ' -f2 | sort)
geo=$(grep -oE '^[0-9a-f.:]+/[0-9]+' "$HERE/nginx/cloudflare-geo.inc" | sort)
if [ "$real" != "$geo" ]; then
    echo "cloudflare-real-ip.inc and cloudflare-geo.inc list different ranges"
    diff <(echo "$real") <(echo "$geo") || true
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/tls" "$work/lapi"
# Everything bootstrap.sh puts into /etc/nginx/lapi.
cp "$HERE"/nginx/* "$HERE"/crash/nginx/* "$HERE"/mirror/nginx/* "$HERE"/upd/nginx/* "$HERE"/oauth/nginx/* "$HERE"/friends/nginx/* "$HERE"/skins/nginx/* "$HERE"/mods/nginx/* "$HERE"/feed/nginx/* "$HERE"/worlds/nginx/* "$HERE"/servers/nginx/* "$work/lapi/"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=crash.lapi.sh" \
    -keyout "$work/tls/key.pem" -out "$work/tls/cert.pem" 2>/dev/null
for domain in crash.lapi.sh mirror.lapi.sh upd.lapi.sh oauth.lapi.sh friends.lapi.sh skins.lapi.sh mods.lapi.sh feed.lapi.sh worlds.lapi.sh servers.lapi.sh; do
    printf 'ssl_certificate /etc/nginx/lapi-tls/cert.pem;\nssl_certificate_key /etc/nginx/lapi-tls/key.pem;\n' \
        > "$work/tls/$domain.conf"
done
cat > "$work/nginx.conf" <<'CONF'
events {}
http {
    include /etc/nginx/lapi/*.conf;
}
CONF
docker run --rm \
    -v "$work/lapi:/etc/nginx/lapi:ro" \
    -v "$work/tls:/etc/nginx/lapi-tls:ro" \
    -v "$work/nginx.conf:/etc/nginx/nginx.conf:ro" \
    nginx:1.28 nginx -t
