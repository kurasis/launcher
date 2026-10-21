# The servers service (lapi-servers on 127.0.0.1:8789): reads only.
limit_except GET {
    deny all;
}
limit_req zone=lapi_servers burst=60 nodelay;
limit_req_status 429;
limit_conn lapi_servers_conn 16;
limit_conn_status 429;
client_max_body_size 1k;
client_body_timeout 10s;
proxy_pass http://127.0.0.1:8789;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 15s;
proxy_hide_header X-Content-Type-Options;
add_header X-Content-Type-Options nosniff always;
add_header Referrer-Policy no-referrer always;
add_header Strict-Transport-Security "max-age=31536000" always;
