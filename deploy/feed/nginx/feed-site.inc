# The body of both feed.lapi.sh servers (feed.conf).
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/feed.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/feed.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}
# A player's own channel: a link or an @handle, never kept anywhere.
location = /v1/channels/resolve {
    include /etc/nginx/lapi/feed-proxy.inc;
    add_header Referrer-Policy no-referrer always;
    limit_req zone=lapi_feed_lookup burst=10 nodelay;
    limit_except POST {
        deny all;
    }
    client_max_body_size 4k;
}
# The player window's page embeds YouTube or Twitch, whose players need to
# know the page's origin: the service sends its own Referrer-Policy.
location /play/ {
    include /etc/nginx/lapi/feed-proxy.inc;
    limit_except GET {
        deny all;
    }
    client_max_body_size 1k;
}
# The feed, notes, pictures and videos of a player's channels: the service
# says how long each is kept.
location /v1/ {
    include /etc/nginx/lapi/feed-proxy.inc;
    add_header Referrer-Policy no-referrer always;
    limit_except GET {
        deny all;
    }
    client_max_body_size 1k;
}
location / {
    return 404;
}
