# Versions Mojang's list does not have

The April Fools versions and every other Minecraft build that Mojang's version list
leaves out (docs/DECISIONS.md D-073). Players get them from upd.lapi.sh only: our signed
list, the version files and the game jars. Libraries, assets and Java come from Mojang,
as for every version.

## What is there

`data/extra-versions.json`, in the order the launcher shows it:

| Group                                         | What                                                                       |
| --------------------------------------------- | -------------------------------------------------------------------------- |
| `joke`                                        | April Fools versions, newest first: 26w14a … 15w14a, 2.0 Blue, Red, Purple |
| `unobfuscated`                                | 25w45a … 1.21.11 without obfuscation                                       |
| `experimental`                                | 1.18 Experimental Snapshot 1–7, 1.19 Deep Dark Experimental Snapshot 1     |
| `combat`                                      | Combat Test 1–8c                                                           |
| `first-build`                                 | builds Mojang replaced under the same name (1.16, 1.16.5-rc1, …)           |
| `early-release`                               | snapshots and pre-releases of 1.0–1.5 (2011–2013)                          |
| `beta`, `alpha`, `infdev`, `indev`, `classic` | builds of 2009–2011 kept by archives                                       |

Joke versions that are in Mojang's list (`inMojangList`) are served from here too. Each
entry has its version file (`descriptor`, pinned by `sha1`) and its jars (`files`,
with where they came from).

## A new version

Each April, or when an archive finds a build:

1. Add an April Fools version of Mojang's list to `MANIFEST_JOKES` in
   `scripts/extra-versions.mjs` (newest first, with its update's name).
2. `node scripts/extra-versions.mjs` (behind a proxy: `NODE_USE_ENV_PROXY=1`) and
   `node scripts/extra-versions-files.mjs <folder>` to see that every jar downloads with
   its SHA-1.
3. Open a pull request: the Versions workflow downloads and scans every jar. On main it
   publishes.

## On the server

```text
/home/www/lapi/upd/data/versions/manifest.json        the signed list
/home/www/lapi/upd/data/versions/meta/<sha1>.json     version files
/home/www/lapi/upd/data/versions/files/<sha1>.jar     game jars
```

`lapi-upd status` says how many versions are listed and whether a file is missing. The
nginx locations come with `deploy/bootstrap.sh`.
