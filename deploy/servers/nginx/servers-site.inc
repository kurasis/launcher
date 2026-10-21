# The body of both servers.lapi.sh servers (servers.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/servers.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/servers.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# The ads showing now (the service says how long they may be kept), their
# icons, and the deploy's health check. Nothing else reaches the service.
location = /v1/ads {
    include /etc/nginx/lapi/servers-proxy.inc;
}
location /v1/icons/ {
    include /etc/nginx/lapi/servers-proxy.inc;
}
location = /v1/health {
    include /etc/nginx/lapi/servers-proxy.inc;
}
location / {
    return 404;
}
