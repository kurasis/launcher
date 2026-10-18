# The crash service's API (lapi-crash on 127.0.0.1:8781).
limit_req zone=lapi_crash burst=20 nodelay;
limit_req_status 429;
limit_conn lapi_crash_conn 8;
limit_conn_status 429;
# A report is at most 1 MiB gzip; the service checks the unpacked size.
client_max_body_size 2m;
client_body_timeout 20s;
# The whole body arrives here before the service sees it.
proxy_request_buffering on;
proxy_pass http://127.0.0.1:8781;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 30s;
add_header X-Content-Type-Options nosniff always;
add_header Cache-Control no-store always;
add_header Referrer-Policy no-referrer always;
