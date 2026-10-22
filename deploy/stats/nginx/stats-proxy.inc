# The statistics service (lapi-stats on 127.0.0.1:8791). Included by each
# location of stats-site.inc, which says the methods and body size.
limit_req zone=lapi_stats burst=60 nodelay;
limit_req_status 429;
limit_conn lapi_stats_conn 16;
limit_conn_status 429;
client_body_timeout 15s;
# The whole body arrives here before the service sees it.
proxy_request_buffering on;
proxy_pass http://127.0.0.1:8791;
proxy_http_version 1.1;
proxy_set_header Host $host;
# The address only for the service's rate limits in memory; the country is
# the one thing about the player that is counted.
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Country $http_cf_ipcountry;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 15s;
proxy_hide_header X-Content-Type-Options;
add_header X-Content-Type-Options nosniff always;
add_header Referrer-Policy no-referrer always;
add_header Strict-Transport-Security "max-age=31536000" always;
add_header Cache-Control "no-store" always;
