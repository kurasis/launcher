# The worlds service (lapi-worlds on 127.0.0.1:8788).
limit_req zone=lapi_worlds burst=60 nodelay;
limit_req_status 429;
limit_conn lapi_worlds_conn 16;
limit_conn_status 429;
client_max_body_size 32k;
client_body_timeout 15s;
proxy_pass http://127.0.0.1:8788;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
add_header X-Content-Type-Options nosniff always;
add_header Cache-Control no-store always;
add_header Referrer-Policy no-referrer always;
add_header Strict-Transport-Security "max-age=31536000" always;
