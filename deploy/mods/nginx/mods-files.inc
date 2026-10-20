# The body of both files-mods.lapi.sh servers (mods.conf): kept files and
# pictures straight from disk, by SHA-256 (services/mods/src/blobs.rs). They
# never change, so Cloudflare keeps them; the launcher checks every file's
# hash. Nothing goes to the service.
include /etc/nginx/lapi/cloudflare-real-ip.inc;
server_tokens off;
access_log /var/log/nginx/files-mods.lapi.sh.access.log lapi_anon;
limit_req_log_level info;
limit_conn_log_level info;
error_log /var/log/nginx/files-mods.lapi.sh.error.log crit;

if ($lapi_via_cloudflare = 0) {
    return 444;
}

limit_req zone=lapi_mods_files burst=1000 nodelay;
limit_req_status 429;
limit_conn lapi_mods_files_conn 32;
limit_conn_status 429;

location /.well-known/acme-challenge/ {
    root /home/www/lapi/acme;
}

# A file under its own name: /f/<sha256>/<name>. Always a download.
location ~ "^/f/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})/[^/]{1,300}$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type application/octet-stream;
    add_header Content-Disposition attachment always;
    include /etc/nginx/lapi/mods-files-headers.inc;
}

# The same by its hash alone: the reserve of cdn-mods.lapi.sh (R2), whose
# addresses are /f/<sha256>; the launcher comes here when R2 fails.
location ~ "^/f/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type application/octet-stream;
    add_header Content-Disposition attachment always;
    include /etc/nginx/lapi/mods-files-headers.inc;
}

# Pictures: /i/<sha256>.<extension>, with the type the extension names.
location ~ "^/i/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})\.png$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type image/png;
    include /etc/nginx/lapi/mods-files-headers.inc;
}
location ~ "^/i/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})\.jpg$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type image/jpeg;
    include /etc/nginx/lapi/mods-files-headers.inc;
}
location ~ "^/i/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})\.gif$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type image/gif;
    include /etc/nginx/lapi/mods-files-headers.inc;
}
location ~ "^/i/([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{60})\.webp$" {
    alias /home/www/lapi/mods/data/blobs/$1/$2/$1$2$3;
    types { }
    default_type image/webp;
    include /etc/nginx/lapi/mods-files-headers.inc;
}

location / {
    return 404;
}
