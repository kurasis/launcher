# The body of both crash.lapi.sh servers (crash.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/crash.lapi.sh.access.log lapi_anon;
# Messages about clients (rate limits, bodies too large) name their address;
# only failures of the server itself are logged. Refusals still show in the
# access log, without addresses.
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/crash.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
location /v1/ {
    include /etc/nginx/lapi/crash-proxy.inc;
}
location / {
    return 404;
}
