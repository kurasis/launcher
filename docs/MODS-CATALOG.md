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

6. **Optional, the secrets `LAPI_MODS_CF_ZONE` and `LAPI_MODS_CF_TOKEN`:** the zone ID
   of lapi.sh (Cloudflare → the domain → Overview → API, Zone ID) and an API token
   with one permission, _Zone → Cache Purge → Purge_, for that zone only (My Profile →
   API Tokens → Create Token → Custom token). With them a block clears the file from
   Cloudflare's cache; without them the file stays there up to a year. A deploy writes
   them to `secrets.env` like the proxies.

7. **Optional, Cloudflare R2** (files served from Cloudflare, our server in reserve):
   1. Cloudflare → R2 Object Storage: turn it on, then create two buckets, `mods-files`
      and `mods-private` (location Automatic). Leave public access off on both.
   2. R2 → Manage API tokens → Create API token: permission _Object Read & Write_,
      applied to these two buckets only. Put its _Access Key ID_ into the secret
      `LAPI_MODS_R2_KEY`, its _Secret Access Key_ into `LAPI_MODS_R2_SECRET`, and the
      account ID (R2's overview page) into `LAPI_MODS_R2_ACCOUNT`. Re-run the workflow:
      the service starts copying files and the database.
   3. Bucket `mods-files` → Settings → Custom Domains → Connect Domain:
      `cdn-mods.lapi.sh`. Add a Cache Rule for that hostname like the one for
      files-mods.
   4. On the server, re-run `bootstrap.sh` once (nginx learns the address without a
      name, the launcher's reserve).
   5. When `task=counts` shows `blobsToCopy` at 0 or close to it, set the repository
      variable `MODS_CDN_URL` to `https://cdn-mods.lapi.sh` and re-run the workflow:
      from then on the API gives R2's addresses for files already copied.

8. **Players' own packs:** after the deploy that brings them, re-run `bootstrap.sh` once
   on the server, so nginx takes a pack's archive (up to 91 MB) on `/v1/packs` and its
   parts (up to 65 MB each) on `/v1/packs/uploads/`.

## The owner's commands

On the server, as `sudo -u lapi-mods /home/www/lapi/mods/current/lapi-mods <command>`:

| Command                                          | What it does                                                                                                                  |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| `find <address, slug, ID, SHA-256 or SHA-1>`     | a project with its versions and files, or a file: state, verdict, signs, where it is used, decisions                          |
| `review`                                         | what waits: files the scanner held back (`scan`), versions their source took down (`gone`), reported packs (`report`)         |
| `review <n> keep` or `review <n> block [reason]` | decides item `n`: `keep` serves a held file again or dismisses reports, `block` blocks the files or removes the reported pack |
| `block <SHA-256 or SHA-1> [reason]`              | takes the file out of service at once, tells launchers, clears Cloudflare's cache; also for a file not here yet               |
| `unblock <SHA-256 or SHA-1>`                     | lifts a block, or lets a file the scanner held go; no later scan holds it                                                     |
| `remove <project> [reason]`                      | at its author's request: the project leaves search and the API for good, files only it needs are held back                    |
| `forget <SHA-256> [--yes]`                       | deletes a held file for good (asks for `--yes`); the catalog never fetches it again                                           |
| `restore`                                        | brings back to the disk, checked by SHA-256, the files only R2 still has                                                      |

Projects from sites that are not catalogs (D-066, external source) are added by these
commands only:

| Command                                                                                          | What it does                                                                    |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `external add optifine [--days N]`                                                               | adds OptiFine from optifine.net (checked every 3 days unless `--days`)          |
| `external manual <slug> --title T --kind K --page URL [--summary S] [--icon URL] [--days N]`     | adds or changes a project described by hand (checked every 7 days unless set)   |
| `external version <project> --file URL --game G[,G] [--loader L[,L]] [--number N] [--channel C]` | adds a version of a project described by hand: one file, kept here at once      |
| `external days <project> <N or default>`                                                         | days between checks, 0 never; for a Modrinth or CurseForge project, the longest |
| `external check <project>`                                                                       | checks it now                                                                   |
| `external list`                                                                                  | the owner's projects: kind, days, versions, versions kept, last read            |

Kinds are `mod`, `modpack`, `resourcepack`, `shader`, `datapack` and `world`; channels
`release`, `beta` and `alpha`. A file must be a `.jar`, `.zip` or `.mrpack` on an HTTPS
address of a public site.

`find` and `review` show titles and names: run them in a terminal, not in a workflow.
Without SSH, Actions → Mods service → Run workflow with `task=block` or `task=unblock` and
the file's hash, or `task=review-count`. They wait for the production approval and print
fixed words, numbers and the hash only; a reason can be given over SSH only.
`task=external` runs `external` with `args`, its arguments as a JSON array (for
example `["add", "optifine"]` or `["days", "optifine", "3"]`); they reach the service on
its standard input, never through a shell.

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
