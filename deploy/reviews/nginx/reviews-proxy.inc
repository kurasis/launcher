# The reviews service (lapi-reviews on 127.0.0.1:8790). Included by each
# location of reviews-site.inc, which says the methods and body size.
limit_conn lapi_reviews_conn 8;
limit_conn_status 429;
limit_req_status 429;
client_body_timeout 30s;
# The whole body arrives here before the service sees it.
proxy_request_buffering on;
proxy_pass http://127.0.0.1:8790;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 60s;
proxy_hide_header X-Content-Type-Options;
add_header X-Content-Type-Options nosniff always;
add_header Referrer-Policy no-referrer always;
add_header Strict-Transport-Security "max-age=31536000" always;
add_header Cache-Control "no-store" always;
