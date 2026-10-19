# Catalog of mods (mods.lapi.sh)

The `lapi-mods` service (D-066) keeps the catalog of mods, modpacks, resource packs,
shaders and worlds that players add from Modrinth and CurseForge, with every page,
picture and file, for good. Its API is `https://mods.lapi.sh`; nginx serves the kept
files at `https://files-mods.lapi.sh` straight from disk. The workflow
`.github/workflows/mods-service.yml` tests, builds and deploys it from `main`.

The launcher's Mods tab reads it through the core's `mods_*` commands; a test build can
point at another server with `LAUNCHER_MODS_URL` at build time (release builds never set
it). What the launcher puts into an installation is listed in its `content.lock.json`.

## Owner actions

1. **Cloudflare DNS:** proxied (orange cloud) records `mods` and `files-mods` pointing at
   the server, like the other services.
2. **Cloudflare cache:** a Cache Rule for hostname `files-mods.lapi.sh`: _Eligible for
   cache_, edge TTL _Use cache-control header if present_. Its files never change (the
   address holds their SHA-256) and answer with `Cache-Control: immutable` for a year.
   Leave mods.lapi.sh with the default rules: its answers say themselves how long they
   may be kept (60 seconds at most, private ones `no-store`).
3. **On the server, once,** after the first run of the workflow on `main`:
   `sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user>`. It makes the user
   `lapi-mods`, the folders, the units (with a daily database copy), one certificate
   for both names and the nginx files. Then re-run the workflow (Actions → Mods service
   → Run workflow).
4. **Optional, the repository variable `MODS_MAX_GB`:** the disk quota in GB (default
   500). The service keeps 5 GB of the disk free whatever the quota.
5. **Optional, the secret `LAPI_MODS_PROXIES`:** proxies toward Modrinth and
   CurseForge, as `http://`, `https://`, `socks5://` or `socks5h://` URLs (with
   `user:password@` if needed) separated by spaces, commas or lines. Without it the
   server connects directly; with it, the direct connection is used too. A deploy
   writes it to `/home/www/lapi/mods/secrets.env` (0640); no log shows it.

## curl-impersonate

Sources are fetched with curl-impersonate by lexiforest (Chrome 150's fingerprint),
shipped in each release under `curl/`. The workflow downloads the release named in
`CURL_IMPERSONATE_TAG` and checks its archive against `CURL_IMPERSONATE_SHA256` before
anything in it runs. To move to a newer release, empty both values: the next run fails
and its summary names the newest release and its SHA-256 to pin.

## Looking at it

Actions → Mods service → Run workflow with `task=counts` shows numbers only (projects,
versions, kept files and GB, requests, jobs waiting and failed). On the server:

`task=probe` shows how CurseForge answers the server, directly and through each proxy
(numbered, never named): a status, `challenge` when Cloudflare asked for a browser check,
the size, and for pages whether they carry their data. On the server:

```sh
sudo -u lapi-mods /home/www/lapi/mods/current/lapi-mods counts
sudo -u lapi-mods /home/www/lapi/mods/current/lapi-mods probe
journalctl -u lapi-mods
```

## Restoring the database

```sh
sudo systemctl stop lapi-mods
sudo install -o lapi-mods -g lapi-mods -m 0600 catalog-<time>.db /home/www/lapi/mods/data/db/catalog.db
sudo rm -f /home/www/lapi/mods/data/db/catalog.db-wal /home/www/lapi/mods/data/db/catalog.db-shm
sudo systemctl start lapi-mods
```

Kept files stay on disk; a version whose file the copy does not know yet is fetched
again on request.
