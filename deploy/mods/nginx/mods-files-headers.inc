# Headers of every kept file and picture (mods-files.inc).
# Only on answers that found the file: a missing one is not kept for a year.
add_header Cache-Control "public, max-age=31536000, immutable";
add_header X-Content-Type-Options nosniff always;
add_header Content-Security-Policy "sandbox" always;
add_header Referrer-Policy no-referrer always;
add_header Access-Control-Allow-Origin "*" always;
