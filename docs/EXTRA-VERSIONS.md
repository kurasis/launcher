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

## In the launcher

- `src-tauri/src/catalog/extra.rs` reads the list from `https://upd.lapi.sh/versions/manifest.json`
  with Mojang's catalog (every 6 hours, or when the player checks again). It takes a list only
  when its signature verifies with the update key built into the launcher, names the kind
  `versions` and its serial, and the serial is not lower than the list kept in
  `metadata/extra-versions.json`. A build without the key (`LAUNCHER_UPDATE_PUBKEY`) shows none
  of these versions; one installed before still repairs and starts from its lock.
- The versions follow Mojang's in the catalog; an April Fools version Mojang lists too is ours
  from then on. The picker shows them on the Special tab in the list's order, by name, with
  the update's name and the group.
- Their version files and game jars come from `upd.lapi.sh/versions/` only
  (`SourcePolicy::ExtraVersions`, never the mirror); libraries, assets and Java from Mojang.
- Worlds with Friends offers the 45 with a server from 1.7.6's day on behind «Show special
  versions» in the new world form (`worlds::tuple::of_entry`), by name; their server jars come
  from here too. A world made for one has `"special": true` in `world.json`. The worlds service
  takes the same ids for rooms from `data/extra-world-versions.txt`.
- The skins mod is not offered on them: it is chosen by exact game version.
- The Player test (docs/USERTEST.md) plays them from the packaged launcher on the owner's
  Windows test machine as a player does, to the title screen: `versions: special:joke` (a
  group), `special:<id>` or `special` (all 233).

## A new version

Each April, or when an archive finds a build:

1. Add an April Fools version of Mojang's list to `MANIFEST_JOKES` in
   `scripts/extra-versions.mjs` (newest first, with its update's name).
2. `node scripts/extra-versions.mjs` (behind a proxy: `NODE_USE_ENV_PROXY=1`; it writes
   `data/extra-world-versions.txt` too) and
   `node scripts/extra-versions-files.mjs <folder>` to see that every jar downloads with
   its SHA-1.
3. Open a pull request: the Versions workflow downloads and scans every jar. On main it
   publishes, and the worlds service is deployed again with the new ids.

## On the server

```text
/home/www/lapi/upd/data/versions/manifest.json        the signed list
/home/www/lapi/upd/data/versions/meta/<sha1>.json     version files
/home/www/lapi/upd/data/versions/files/<sha1>.jar     game jars
```

`lapi-upd status` says how many versions are listed and whether a file is missing. The
nginx locations come with `deploy/bootstrap.sh`.
