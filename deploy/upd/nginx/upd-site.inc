# The body of both upd.lapi.sh servers (upd.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/upd.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/upd.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

limit_req zone=lapi_upd burst=50 nodelay;
limit_req_status 429;
limit_conn lapi_upd_conn 8;
limit_conn_status 429;
root /home/www/lapi/upd/data;

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}

# What a launcher reads: one manifest per channel, platform and install type.
# Nothing published yet is "no update" (204), as the updater expects.
location ~ "^/v1/(stable|beta)/([a-z0-9_]+-[a-z0-9_]+-[a-z]+)\.json$" {
    default_type application/json;
    add_header Cache-Control "public, max-age=60" always;
    add_header X-Content-Type-Options nosniff always;
    try_files /channels/$1/$2.json =204;
}

# Published files never change at their address.
location ~ "^/releases/[0-9][0-9A-Za-z.+-]*/[0-9A-Za-z._+-]+$" {
    add_header Cache-Control "public, max-age=31536000, immutable" always;
    add_header X-Content-Type-Options nosniff always;
    try_files $uri =404;
}

# Launcher Skins jars (D-065): players download them only from here, as
# `lapi-upd skins-mod-jars` put them. They never change at their address.
location ~ "^/skins-mod/[0-9]+\.[0-9]+\.[0-9]+/launcherskins-[a-z0-9.-]+\.jar$" {
    types {}
    default_type application/java-archive;
    add_header Cache-Control "public, max-age=31536000, immutable" always;
    add_header X-Content-Type-Options nosniff always;
    try_files $uri =404;
}

# Minecraft versions Mojang's list does not have (D-073): the signed list, the
# version files and the game jars, as `lapi-upd versions` put them. Players
# download these jars only from here.
location = /versions/manifest.json {
    default_type application/json;
    add_header Cache-Control "public, max-age=300" always;
    add_header X-Content-Type-Options nosniff always;
    try_files $uri =404;
}

location ~ "^/versions/meta/[0-9a-f]{40}\.json$" {
    default_type application/json;
    add_header Cache-Control "public, max-age=31536000, immutable" always;
    add_header X-Content-Type-Options nosniff always;
    try_files $uri =404;
}

location ~ "^/versions/files/[0-9a-f]{40}\.jar$" {
    types {}
    default_type application/java-archive;
    add_header Cache-Control "public, max-age=31536000, immutable" always;
    add_header X-Content-Type-Options nosniff always;
    try_files $uri =404;
}

location = /download/ {
    add_header Cache-Control "public, max-age=60" always;
    add_header X-Content-Type-Options nosniff always;
    try_files /download/index.html =404;
}

location = / {
    return 302 /download/;
}

location / {
    return 404;
}
