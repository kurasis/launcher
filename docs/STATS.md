# Statistics: reading the numbers (D-071)

Launchers send anonymous statistics without IDs to `stats.lapi.sh`: once a day
a ping with a few facts and flags, and every hour the counts of the server ads
they showed. The service adds one to sums by day and keeps nothing that
describes one launcher. Nothing is readable over HTTP. The numbers are read
only on the server, with the service's command line. The repository and its
workflow logs are public, so no number goes into issues, pull requests or
workflow logs, and no task of the "Stats service" workflow prints one.

## Where the data is

| What                 | Where                                           | Kept                                   |
| -------------------- | ----------------------------------------------- | -------------------------------------- |
| Players' sums by day | `/home/www/lapi/stats/data/stats.db`, `players` | 2 years, then summed by month for good |
| Ad counts by day     | the same database, `ads`, `ads_seen`            | 3 years (disputes, the ad register)    |
| Batch IDs            | the same database, `batches`                    | 8 days (a resend counts once)          |
| What was left out    | the same database, `dropped`                    | 2 years                                |
| Copies               | `/home/www/lapi/stats/data/backups/`            | the newest 14, made daily              |

No row holds an address, an ID of a launcher or a request as it came. The
address is used for the rate limits and the bursts below, in memory, for an
hour.

## Commands

Log in as the deploy user (allowed without a password) or as an administrator,
then:

```sh
s() { sudo -u lapi-stats /home/www/lapi/stats/current/lapi-stats "$@"; }

s overview --days 30          # players, new, retention, countries, left out
s players --days 30           # per day: players, first of the week and month, new
s cut country --days 30       # one cut, summed over the days
s cut system --period week    # day|week|month|new: which flag of the ping
s cut version --limit 50      # the first 50 values, then the rest
s retention --weeks 12        # install weeks, and how many came back after k weeks
s retention --country RU      # the same for one country
s ads --days 30               # every ad: shown, reach, Play, joined, 10 minutes
s ad 7 --days 30              # one ad: from Play to the server, refusals, online
s ord 2026-10                 # impressions per erid for the ad register
s backup                      # a copy of the database in data/backups now
s prune                       # what is kept no longer, removed (the service does it every 6 hours)
```

Every table takes `--csv` (fields that start like a formula get a `'`). From
your own computer:

```sh
ssh <deploy-user>@<server> "sudo -n -u lapi-stats /home/www/lapi/stats/current/lapi-stats ads --csv" > ads.csv
```

Output is cleaned of control and direction characters. Days are UTC; weeks
start on Monday.

### Cuts

Each ping adds one to every cut, under each flag it has:

| Cut          | Value                                  | Example                 |
| ------------ | -------------------------------------- | ----------------------- |
| `country`    | Cloudflare's country                   | `RU`                    |
| `system`     | OS / version / architecture            | `windows/11/x86_64`     |
| `version`    | launcher / channel / way installed     | `1.4.0/stable/nsis`     |
| `country_ui` | country / interface language           | `KZ/ru`                 |
| `language`   | system language / interface language   | `kk/ru`                 |
| `country_tz` | country / time zone (a VPN shows here) | `RU/Asia/Yekaterinburg` |
| `hardware`   | memory in GiB / cores                  | `8-16/5-8`              |
| `accounts`   | kinds of accounts / country            | `microsoft+notoken/RU`  |
| `cohort`     | install week / country                 | `2026-W41/RU`           |

The flags are `day` (the day's ping), `week` and `month` (the launcher's first
ping of the week, of the month) and `new` (its first ping ever). So "players
this week" is the week's `week` pings, and retention is a cohort's `week` pings
k weeks after its install week. A value outside the known words counts as
`other`; a cut holds at most 20,000 values a day, and later new values go to
`other` too.

## What the numbers look like

From a test run with four launchers (made-up data):

```text
$ s overview --days 7
Players (UTC days; launchers with statistics on, no IDs)
  today 2026-10-22 (so far)             4
  yesterday                             0
  this week (since Monday)              4
  last week                             0
  this month                            4
  last month                            0
  new in the last 7 days                4
  per day, last 7 days      ▁▁▁▁▁▁█  0 to 4
  still there after 4 weeks             -  (installed in 2026-W38: 0 of 0)

Countries this month
  RU       50.0%  ████████████████████████  2
  AE       25.0%  ████████████              1
  DE       25.0%  ████████████              1

Left out, last 7 days
  again 2

$ s ads --days 7
Ads, 2026-10-16 to 2026-10-22
   ad  name                                         shown    reach   play    CTR  joined  10 min   burst
    7  Northern Land                                    7        4      2  28.6%       1       1       0

$ s ad 7 --days 7
Northern Land · ad 7 · 2026-10-16 to 2026-10-22
Shown                  7   reach 4 launcher-days, 4 new launchers; 1.8 per launcher-day
Play                   2   28.6% of the shown
Online                51   on average while it shows; - in the week before
From Play
  pressed Play                2  100.0%  ████████████████████████
  started the game            2  100.0%  ████████████████████████
  joined the server           1   50.0%  ████████████
  played 10 minutes           1   50.0%  ████████████
  back a day later            0    0.0%
  back a week later           0    0.0%
Stopped before the game: needs a Microsoft account 0, installed first 0, install failed 0
Turned away: version 0, full 0, whitelist 1, ban 0, noanswer 0, other 0
Minutes on the server 25 (25 per player who joined)
Menu 4, version list 0, address copied 0, reported 0
Versions started: 1.21.4 100.0%
Countries (shown): RU 57.1%, DE 28.6%, AE 14.3%
Languages (shown): ru 57.1%, en 28.6%, ar 14.3%
Per day
  day             shown    reach    play  joined  minutes
  2026-10-22          7        4       2       1       25

$ s ord 2026-10
Impressions per erid, 2026-10 (UTC days, 2026-10-01 to 2026-10-31)
  erid          ads      shown  in Russia
  2VtzqxAbCdE     1          7          4
    ads 7
Each erid's impressions go to the ad register (ERIR) through the ORD by the 30th of the next month.
```

## Reading the ad numbers

- **Shown** is an impression: half of the tile on screen for a second while
  the window shows. The same ad counts again 5 minutes later at the earliest,
  and at most 300 times a launcher a day.
- **Reach** is launcher-days that saw the ad (`reach_day`) and launchers that
  saw it for the first time (`reach_ever`), so shown ÷ reach is the frequency.
- **From Play**: pressed Play; started the game (after an install when the
  version was missing); joined the server (the chat showed something, or two
  minutes passed without a refusal); played 10 minutes; came back to play it a
  day or a week after the first time. "Turned away" is the server's refusal by
  its reason, read from the game's log by fixed words. The log never leaves the
  computer.
- **Online** comes from servers.lapi.sh, which pings each ad's server every
  hour from when the ad is added; "the week before" is the week before the ad
  started, when it was added that early.
- **Burst**: impressions from one network (an IPv4 /24, an IPv6 /48) past 1,000
  of one ad in an hour. They are kept apart and are in neither the other
  numbers nor `ord`. Pings of more than 100 new launchers from one network in
  an hour are left out the same way, as `burst` under "Left out".
- **ord** sums the impressions of each erid in the month (UTC days), with
  Russia apart, as the ad register asks for. An ad's erid is the one the ads
  service showed that day.

"Left out" in `overview` names what was not counted: `again` (a batch sent
twice; normal after a lost answer), `burst`, `work` (no proof of work),
`unknown_ad` (counts for an ad that did not show that day), `bad_ping` and
`bad_counts` (a request of the wrong shape).

## How launchers send

- `POST /v1/ping` once a UTC day, at the first pass online; `POST /v1/counts`
  every hour while the launcher runs, and at exit. Bodies are JSON, gzip, at
  most 8 KiB (ping) and 64 KiB (counts) unpacked.
- A random batch ID per request makes a resend count once. Each request
  carries a proof of work: a nonce such that the SHA-256 of
  `stats.lapi.sh|<batch>|<day>|<nonce>` starts with 20 zero bits (about a
  second on a player's computer). A request without enough work is answered
  `422 {"error":"work","bits":N}` and the launcher works more, up to 24 bits.
- Answers: 204 taken; 422 or 400 refused for good (the launcher drops it); 429
  and 503 later (the launcher keeps pings of today and yesterday, and ad counts
  of 7 days, 200 batches at most).
- Limits before a body is read: 300 pings and 600 batches an hour per address
  (an IPv6 /64 counts as one), five times that per IPv6 /48, 400,000 pings and
  800,000 batches an hour in all; nginx limits the request rate too. Pings of
  yesterday to tomorrow and batches of the last 7 days are taken.
- The player sees what goes in Settings → Privacy → What is sent, and turns it
  off there; turning it off erases what waits.

## Difficulty and limits

The proof of work's difficulty is `LAPI_STATS_WORK_BITS` in
`/home/www/lapi/stats/stats.env` (20; 8 to 28), which an administrator edits.
Raising it makes launchers do more work at their next request, so a flood
costs more; each bit doubles it. Then `sudo systemctl restart lapi-stats`.
