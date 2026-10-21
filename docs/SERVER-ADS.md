# Server ads

Ads for Minecraft servers in the launcher (D-069) live in the servers service
(servers.lapi.sh). From outside it can only be read; ads are added and changed with these
commands on the server, over SSH, as the service's user:

```sh
sudo -u lapi-servers /home/www/lapi/servers/current/lapi-servers <command>
```

Do not run them from a GitHub workflow: this repository's workflow logs are public.

## Adding an ad

```sh
lapi-servers add --name "Northern Land" --address play.example.net \
    --version 1.21.4 --from 1.16.5 --to 1.21.11 \
    --text "Survival with clans and weekend events." \
    --text-ru "Выживание с кланами и ивентами по выходным." \
    --start "2026-10-21 14:00" --for 7d
```

| Flag                                  | What it is                                                                                         |
| ------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `--name`                              | The server's name, one line of up to 40 characters                                                 |
| `--address`                           | `host`, `host:port`, an IPv4 address or `[IPv6]:port`; without a port the game's SRV lookup works  |
| `--version`                           | The version Play starts by default                                                                 |
| `--from`, `--to`                      | The oldest and newest release players may pick (both default to `--version`)                       |
| `--text`                              | The text shown to everyone: up to 200 characters, 3 lines, no links                                |
| `--text-en`, `--text-ru`, `--text-ar` | The text in that language, for players whose launcher uses it                                      |
| `--text-file`, `--text-ru-file`, …    | The same from a file (4 KB at most); `-` reads standard input                                      |
| `--start`                             | When it starts showing (default: now), in `LAPI_SERVERS_TZ` (`+03:00`), or RFC 3339                |
| `--for` / `--until`                   | How long it shows (`1m`, `90m`, `12h`, `7d`, `2w`, `3mo`, `1y`; 1 minute to 5 years) or until when |
| `--weight`                            | 1–100 (default 10): heavier ads show more often among those showing at once                        |
| `--place`                             | Where it shows: `feed` (the only place for now)                                                    |
| `--erid`                              | The ad's token from the Russian ad register, if it has one                                         |
| `--if-new`                            | Adds nothing if an ad of this address still shows                                                  |
| `--no-ping`, `--quiet`                | Do not ping the server now; print one line of fixed words and the ad's number                      |

Versions must be releases in Mojang's manifest. The command pings the server at once and
prints what players will see: whether it answers, players online, its own version text and
whether it has an icon. Players see a change within about 30 seconds.

A file in your home folder cannot be read by `lapi-servers`; pass it on standard input:

```sh
sudo -u lapi-servers /home/www/lapi/servers/current/lapi-servers add … --text-ru-file - < ad-ru.txt
```

## Other commands

| Command                     | What it does                                                          |
| --------------------------- | --------------------------------------------------------------------- |
| `list [--all]`              | Ads showing or to come, one per line (with `--all`, ended ones too)   |
| `show <id>`                 | Everything about one ad and its last ping                             |
| `edit <id> [flags of add]`  | Changes only the flags given; `--clear-erid` removes the erid         |
| `extend <id> <30d>`         | Shows longer                                                          |
| `pause <id>`, `resume <id>` | Hides it for a while and brings it back                               |
| `stop <id>`                 | Ends it now; it stays in `list --all`                                 |
| `remove <id>`               | Deletes it for good                                                   |
| `ping <id>`                 | Asks the server now (the service also asks every server every hour)   |
| `icon <id> <file.png>`      | Uses this square PNG instead of the server's icon; `--auto` goes back |
| `counts`                    | Numbers only                                                          |
| `backup`                    | A copy of the database in `data/backups` (also daily, by a timer)     |

## What the service does on its own

Every hour it pings each ad's server (never an address that is not public) and keeps its
icon, made 64×64, and the number of players online; when a ping fails, the last icon stays
and the tile says the server does not answer. Every six hours it reads Mojang's list of
releases. Nothing about players is kept or logged.
