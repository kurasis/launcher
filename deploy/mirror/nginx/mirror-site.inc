# The body of both mirror.lapi.sh servers (mirror.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/mirror.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/mirror.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

limit_req zone=lapi_mirror burst=2000 nodelay;
limit_req_status 429;
limit_conn lapi_mirror_conn 64;
limit_conn_status 429;
add_header X-Content-Type-Options nosniff always;

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}

location = /v1/health {
    proxy_pass http://127.0.0.1:8782;
    proxy_set_header Host $host;
}

# The mirrored hosts (services/mirror/src/origin.rs). A kept file is served
# from disk; anything else (not kept yet, metadata, a query) goes to the
# service.
location ~ "^/(piston-meta\.mojang\.com|piston-data\.mojang\.com|launcher\.mojang\.com|launchermeta\.mojang\.com|libraries\.minecraft\.net|resources\.download\.minecraft\.net|api\.adoptium\.net|github\.com|meta\.fabricmc\.net|maven\.fabricmc\.net|meta\.quiltmc\.org|maven\.quiltmc\.org|maven\.minecraftforge\.net|files\.minecraftforge\.net|maven\.neoforged\.net)/" {
    root /home/www/lapi/mirror/data/files;
    error_page 418 = @service;
    if ($args) {
        return 418;
    }
    add_header Cache-Control "public, max-age=31536000, immutable" always;
    add_header X-Content-Type-Options nosniff always;
    add_header X-Mirror-Cache kept always;
    try_files $uri @service;
}

location @service {
    proxy_pass http://127.0.0.1:8782;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For "";
    # Files stream to the player while the service keeps them.
    proxy_buffering off;
    proxy_read_timeout 120s;
}

location / {
    return 404;
}
