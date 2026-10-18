# The crash service's API (lapi-crash on 127.0.0.1:8781).
limit_req zone=lapi_crash burst=20 nodelay;
limit_req_status 429;
# A report is at most 1 MiB gzip; the service checks the unpacked size.
client_max_body_size 2m;
proxy_pass http://127.0.0.1:8781;
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For "";
proxy_read_timeout 30s;
access_log /var/log/nginx/crash.lapi.sh.access.log;
