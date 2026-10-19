# The body of both skins.lapi.sh servers (skins.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/skins.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/skins.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# Uploads from the launcher: one PNG (the service takes 2 MiB for a skin,
# 1 MiB for a cape).
location ~ ^/v1/me/(skin|cape)$ {
    include /etc/nginx/lapi/skins-proxy.inc;
    limit_req zone=lapi_skins_upload burst=5 nodelay;
    client_max_body_size 3m;
    proxy_request_buffering on;
    proxy_hide_header Cache-Control;
    add_header Cache-Control no-store always;
}
# Public lookups by nick, kept a minute (the service's Cache-Control). The
# nick is not logged.
location /v1/profile/ {
    include /etc/nginx/lapi/skins-proxy.inc;
    client_max_body_size 16k;
    access_log /var/log/nginx/skins.lapi.sh.access.log lapi_skins_lookup;
}
# Textures by their SHA-256: they never change, so Cloudflare and the game
# may keep them for good (the service's Cache-Control and ETag).
location /v1/textures/ {
    include /etc/nginx/lapi/skins-proxy.inc;
    client_max_body_size 16k;
}
# The account's own skin and cape, its export, lookups of many nicks: never
# kept anywhere.
location /v1/ {
    include /etc/nginx/lapi/skins-proxy.inc;
    client_max_body_size 16k;
    proxy_hide_header Cache-Control;
    add_header Cache-Control no-store always;
}
location / {
    return 404;
}
