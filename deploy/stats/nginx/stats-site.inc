# The body of both stats.lapi.sh servers (stats.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/stats.lapi.sh.access.log lapi_anon;
# Messages about clients (rate limits, bodies too large) name their address;
# only failures of the server itself are logged.
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/stats.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# The day's flags and the hourly counts: small gzip JSON (the service checks
# the unpacked size).
location ~ "^/v1/(ping|counts)$" {
    limit_except POST {
        deny all;
    }
    client_max_body_size 16k;
    include /etc/nginx/lapi/stats-proxy.inc;
}
# The deploy's health check.
location = /v1/health {
    limit_except GET {
        deny all;
    }
    client_max_body_size 1k;
    include /etc/nginx/lapi/stats-proxy.inc;
}
location / {
    return 404;
}
