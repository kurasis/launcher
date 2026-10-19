# The skins service (lapi-skins on 127.0.0.1:8785). How long an answer may be
# kept is the service's to say (textures for good, lookups a minute), or the
# location's; the body size is the location's too.
limit_req zone=lapi_skins burst=200 nodelay;
limit_req_status 429;
limit_conn lapi_skins_conn 32;
limit_conn_status 429;
client_body_timeout 15s;
proxy_pass http://127.0.0.1:8785;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 30s;
proxy_hide_header X-Content-Type-Options;
add_header X-Content-Type-Options nosniff always;
add_header Referrer-Policy no-referrer always;
add_header Strict-Transport-Security "max-age=31536000" always;
