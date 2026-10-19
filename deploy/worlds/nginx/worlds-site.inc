# The body of both worlds.lapi.sh servers (worlds.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/worlds.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/worlds.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# Live events: one long WebSocket per launcher (it lives as long as its
# access token, 10 minutes, and is pinged every 25 seconds).
location = /v1/live {
    include /etc/nginx/lapi/worlds-proxy.inc;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 15m;
    proxy_send_timeout 15m;
}
location = /.well-known/jwks.json {
    include /etc/nginx/lapi/worlds-proxy.inc;
    proxy_read_timeout 30s;
}
location /v1/ {
    include /etc/nginx/lapi/worlds-proxy.inc;
    proxy_read_timeout 30s;
}
location / {
    return 404;
}
