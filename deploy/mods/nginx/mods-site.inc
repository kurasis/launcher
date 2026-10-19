# The body of both mods.lapi.sh servers (mods.conf): the API of lapi-mods on
# 127.0.0.1:8785.
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/mods.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/mods.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
location /v1/ {
    limit_req zone=lapi_mods burst=120 nodelay;
    limit_req_status 429;
    limit_conn lapi_mods_conn 16;
    limit_conn_status 429;
    client_max_body_size 32k;
    client_body_timeout 15s;
    proxy_pass http://127.0.0.1:8785;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For "";
    proxy_read_timeout 30s;
    add_header X-Content-Type-Options nosniff always;
    add_header Referrer-Policy no-referrer always;
    add_header Strict-Transport-Security "max-age=31536000" always;
}
location / {
    return 404;
}
