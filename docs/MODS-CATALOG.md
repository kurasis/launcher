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

   Publishing needs a confirmed email once the account service sends letters, as
   friends does (`LAPI_MODS_REQUIRE_EMAIL` in the service's environment: `auto` by
   default, `1` always, `0` never). An own pack's slug starts with `player-`; the packs
   published before that get the prefix at the first start of this version (a slug
   another project holds gets a tail of the pack's ID). `slug <project> <slug>` changes
   one by hand if needed.

   A room pack (worlds with mods, W12) comes the same way with `?room=true` and is
   checked the same. It is never searched, shown or listed among the account's packs;
   its guests install it by the version ID the world's session at worlds.lapi.sh names
   (`GET /v1/versions/{id}/pack`). Each new generation of the world is a new version.

## What mods need

Sources' pages say only part of what a mod needs, and sometimes for the wrong game
version. So the service reads every jar (and zip) it keeps the way the loaders read it
at start, with the shared crate `crates/mod-meta` the launcher uses too:
`fabric.mod.json`, `quilt.mod.json`, `META-INF/neoforge.mods.toml`,
`META-INF/mods.toml` and `mcmod.info`, jars inside jars included, every size, count and
depth bounded. A loop reads 50 jars at a time; one that cannot be read, or is no mod,
gives `[]`. A new reader version (`MANIFEST_RULES` in `import.rs`) reads them all again.

- `GET /v1/versions/{id}` gives each kept file's `manifests`: per loader, the mods the
  jar gives (`mods`, with `nested` for those inside it), what they need (`needs`) and
  what they do not start with (`breaks`), version ranges in the loader's own words.
  Lists of versions leave them out.
- `GET /v1/provides/{mod}?game=&loader=` gives up to five ready projects whose kept
  jars give that mod ID (their own or one they `provide`), most downloaded first:
  where a jar needs a mod its page does not name, this finds it. A held jar gives
  nothing.
- `GET /v1/clashes` gives the owner's list of mods that do not start together though
  neither jar says so (`{"a", "b"}`: `a` is shown as not starting with `b`), started
  from `mod_meta::CLASHES` (OptiFine with Sodium, Iris, Embeddium, Rubidium and Oculus;
  OptiFine's jar goes by `optifine`). The launcher checks a set against it at install
  and before a launch; offline, against the list it last had, else the built-in one.

## Browsing (D-072)

- Categories are one shared list in `services/mods/src/categories.rs`: an ID like
  `mod.optimization` or `shader.realistic` maps the slugs Modrinth and CurseForge use
  for that kind. Projects keep the sites' slugs; answers add `categoryIds`.
  `GET /v1/search?category=a,b` takes up to 8, shared IDs or slugs, and finds a
  project in any of them. `GET /v1/categories` takes the search's own parameters and
  counts the projects in each category of the kind, the categories asked for aside.
- A search also takes `source` (`own` for players' packs), `since` (added in the last
  1 to 365 days), `collection` (a key) and `sort=trending` (downloads gained in the
  last week). Hits carry `loaders`, `categoryIds` and `cover` (the gallery's featured
  picture, small).
- `GET /v1/collections?kind=&game=&loader=` lists the collections that have projects
  there: `popular-week` (the 20 that gained the most downloads), `new` (added in 7
  days), `players-packs` (modpacks only) and the owner's, each with `query` (what to
  search for to list it), `count`, four icons and two titles. Answers are kept for a
  minute. Weekly gains come from a daily copy of every project's download count, kept
  two weeks.
- `GET /v1/projects/{id}/similar?game=&loader=` gives up to five ready projects of the
  same kind sharing most categories, libraries left out unless it is one.
- `GET /v1/projects/{id}/versions?game=&loader=&channel=&offset=&limit=` gives `total`
  and, whatever the filters, the `games` (newest first) and `loaders` its versions are
  for. Without `limit` it gives them all, as before.
- A version's files say `checked` when the catalog keeps them and its check found
  them clean (or the owner let them go); dependencies carry `title`, `slug` and
  `iconThumb` when the catalog has the project.
- `POST /v1/lookup` and a modpack's files (`GET /v1/versions/{id}/pack`) carry, for
  a file the catalog has, `projectId`, `versionId`, the version's `source` and, while
  the project is shown, its `title`, `iconThumb` and the version's `versionNumber`. The
  launcher asks it each time it lists an installation, so a modpack's files and the
  player's own show their names and icons, and every file where it came from. A
  version says its `source` too.

## The owner's commands

On the server, as `sudo -u lapi-mods /home/www/lapi/mods/current/lapi-mods <command>`:

| Command                                                                                           | What it does                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `find <address, slug, ID, SHA-256 or SHA-1>`                                                      | a project with its versions and files, or a file: state, verdict, signs, where it is used, decisions                                                                                    |
| `review`                                                                                          | what waits: files the scanner held back (`scan`), versions their source took down (`gone`), reported packs (`report`)                                                                   |
| `review <n> keep` or `review <n> block [reason]`                                                  | decides item `n`: `keep` serves a held file again or dismisses reports, `block` blocks the files or removes the reported pack                                                           |
| `block <SHA-256 or SHA-1> [reason]`                                                               | takes the file out of service at once, tells launchers, clears Cloudflare's cache; also for a file not here yet                                                                         |
| `unblock <SHA-256 or SHA-1>`                                                                      | lifts a block, or lets a file the scanner held go; no later scan holds it                                                                                                               |
| `remove <project> [reason]`                                                                       | at its author's request: the project leaves search and the API for good, files only it needs are held back                                                                              |
| `forget <SHA-256> [--yes]`                                                                        | deletes a held file for good (asks for `--yes`); the catalog never fetches it again                                                                                                     |
| `restore`                                                                                         | brings back to the disk, checked by SHA-256, the files only R2 still has                                                                                                                |
| `add <address>...`                                                                                | adds up to 32 projects by their Modrinth or CurseForge address, as a player's "Add to the catalog" does; one already here is fetched anew at once                                       |
| `add --status`                                                                                    | the owner's additions by state, and those that failed with why                                                                                                                          |
| `slug <project> <slug>`                                                                           | gives a project another slug (small letters, digits, inner hyphens; not another project's); the old one leads on only via a source                                                      |
| `clash list`, `clash add <mod> <mod>`, `clash remove <mod> <mod>`                                 | the list of mods that do not start together, by mod ID (`/v1/clashes`); a known one removed stays off                                                                                   |
| `collection add <key> <kind> [game\|-] [loader\|-]`, `collection remove <key>`, `collection list` | the owner's collections (D-072): a key of small letters, digits and hyphens, for a kind and optionally a game version and loader                                                        |
| `collection title <key> <ru\|en\|ar> [text]`, `collection note …`                                 | its title (64 characters) and line (160) by language; no text takes it away                                                                                                             |
| `collection put <key> <project>...`                                                               | its projects (up to 100, by ID or slug) in the order shown                                                                                                                              |
| `categories [kind]`                                                                               | how many projects of a kind are in each shared category, and how many of the sites' slugs the list leaves out                                                                           |
| `health`                                                                                          | the service's own checks, one line each (`ok`, `problem` or `off` with numbers); fails when any finds a problem                                                                         |
| `packs`, `packs read`                                                                             | how far the reading of each modpack's newest version went; `read` queues those not read, so their files are kept and get cards before an install (workflow tasks `packs`, `packs-read`) |

Projects from sites that are not catalogs (D-066, external source) are added by these
commands only:

| Command                                                                                          | What it does                                                                    |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `external add optifine [--days N]`                                                               | adds OptiFine from optifine.net (checked every 3 days unless `--days`)          |
| `external manual <slug> --title T --kind K --page URL [--summary S] [--icon URL] [--days N]`     | adds or changes a project described by hand (checked every 7 days unless set)   |
| `external version <project> --file URL --game G[,G] [--loader L[,L]] [--number N] [--channel C]` | adds a version of a project described by hand: one file, kept here at once      |
| `external days <project> <N or default>`                                                         | days between checks, 0 never; for a Modrinth or CurseForge project, the longest |
| `external trust <project> on\|off`                                                               | trusts its files: one the scanner only suspects is served (malware stays held)  |
| `external check <project>`                                                                       | checks it now                                                                   |
| `external list`                                                                                  | the owner's projects: kind, days, versions, versions kept, trust, last read     |

Kinds are `mod`, `modpack`, `resourcepack`, `shader`, `datapack` and `world`; channels
`release`, `beta` and `alpha`. A file must be a `.jar`, `.zip` or `.mrpack` on an HTTPS
address of a public site.

VirusTotal's word on the files the scanner suspects (D-066) shows in `find` and
`review` as `virustotal: 2 / 60` (engines that call it malicious / engines that
answered, then the analysis date), or `waiting`, `sent, analysing`, `unknown` (only
players' own packs carry it, so it is not sent), `too big`, `error, again <day>`, `off`
(no key). `virustotal status` prints the requests made today and this month and the
files by state; `virustotal check <SHA-256 or SHA-1>` asks now, from the owner's share
of the quota.

`find` and `review` show titles and names: run them in a terminal, not in a workflow.
Without SSH, Actions → Mods service → Run workflow with `task=block` or `task=unblock` and
the file's hash, or `task=review-count`. They wait for the production approval and print
fixed words, numbers and the hash only; a reason can be given over SSH only.
`task=external` runs `external` with `args`, its arguments as a JSON array (for
example `["add", "optifine"]`, `["days", "optifine", "3"]` or `["trust", "optifine", "on"]`); they reach the service on
its standard input, never through a shell. `task=virustotal` runs `virustotal` the same
way (`["status"]` or `["check", "<hash>"]`), `task=add` runs `add` (a list of
addresses, or `["--status"]`), `task=slug` runs `slug` (`["<project>", "<slug>"]`),
`task=collection` runs `collection` (`["put", "fps", "sodium", "lithium"]`) and
`task=categories` runs `categories` (`["mod"]`, or nothing for mods).

`health` runs every morning in the workflow Mods health; a problem fails the run, and
GitHub mails it. It checks: a job due over 12 hours and still waiting (`queue`); at
least 20 failed jobs in a day and over a quarter of them (`imports`); at least 5 failed
requests to add projects in a day and over half (`requests`); any job failed in a day
because an answer or file was past a bound of ours (`bounds`); a review open over 3
days (`review`); free disk under twice the reserve or kept files over 90% of the quota
(`disk`); no copy of the database in `data/backups` made in 2 days (`backups`);
VirusTotal's background over 90% of its month (`virustotal`); a wrong copy in
R2 or no database copy in 2 days (`r2`); no look for the most downloaded projects in 2
days (`popular`).

Every day (and at each start) the service also looks for Modrinth's 100 most downloaded
mods, modpacks, resource packs, shaders and data packs it does not have yet
(`LAPI_MODS_POPULAR` in the service's environment sets fewer, 100 being the most
Modrinth gives at once; 0 turns it off). It queues them two at a time, and only while
fewer than 20 jobs are due, so what players ask for never waits behind them for long.
Those it has are checked as any other; one the owner removed stays removed.

The mods, resource packs and shaders of the modpacks it keeps become catalog projects
the same way, so each has a page, a title and an icon. A file whose address is on
Modrinth's CDN names its project; any other is looked up at Modrinth by its SHA-1, ten
a minute. Their projects are queued two at a time while fewer than 20 jobs are due.
A file Modrinth does not know but whose address is CurseForge's (a mod only on
CurseForge) is added by its project number: `https://www.curseforge.com/projects/<number>`
leads to the project's page, and the page found there is queued as any address a player
adds. A file found nowhere is looked up again after 30 days. What was found is in the
`pack_lookup` table.

OptiFine's icon is made for the catalog (`services/mods/assets/optifine-icon.png`): its
site has none.

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
the size, and for pages whether they carry their data; for RLCraft's page, the number
the reader chooses and each candidate's number and likeness. On the server:

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
