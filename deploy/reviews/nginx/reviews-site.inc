# The body of both reviews.lapi.sh servers (reviews.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/reviews.lapi.sh.access.log lapi_anon;
# Messages about clients (rate limits, bodies too large) name their address;
# only failures of the server itself are logged.
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/reviews.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# A review with its files: JSON, pictures in base64 (the service checks 16 MiB).
location = /v1/reviews {
    limit_except POST {
        deny all;
    }
    limit_req zone=lapi_reviews_post burst=5 nodelay;
    client_max_body_size 17m;
    include /etc/nginx/lapi/reviews-proxy.inc;
}
# The player's own reviews and the answers (keys in the body).
location = /v1/status {
    limit_except POST {
        deny all;
    }
    limit_req zone=lapi_reviews burst=20 nodelay;
    client_max_body_size 16k;
    include /etc/nginx/lapi/reviews-proxy.inc;
}
# The player deletes their review.
location ~ "^/v1/reviews/[0-9]{1,12}$" {
    limit_except DELETE {
        deny all;
    }
    limit_req zone=lapi_reviews burst=20 nodelay;
    client_max_body_size 1k;
    include /etc/nginx/lapi/reviews-proxy.inc;
}
# A proof-of-work challenge, and the deploy's health check.
location ~ "^/v1/(challenge|health)$" {
    limit_except GET {
        deny all;
    }
    limit_req zone=lapi_reviews burst=20 nodelay;
    client_max_body_size 1k;
    include /etc/nginx/lapi/reviews-proxy.inc;
}
location / {
    return 404;
}
