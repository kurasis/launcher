# The account service (lapi-oauth on 127.0.0.1:8783): pages and API. It sets
# its own security headers (CSP, frame and referrer rules, no-store).
limit_req zone=lapi_oauth burst=40 nodelay;
limit_req_status 429;
limit_conn lapi_oauth_conn 16;
limit_conn_status 429;
# Forms and JSON bodies only.
client_max_body_size 32k;
client_body_timeout 15s;
proxy_request_buffering on;
proxy_pass http://127.0.0.1:8783;
proxy_http_version 1.1;
proxy_set_header Host $host;
# Kept with the account at registration, for recovery by hand (D-064).
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Country $http_cf_ipcountry;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 30s;
add_header Strict-Transport-Security "max-age=31536000" always;
