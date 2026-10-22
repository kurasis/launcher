# Reviews: reading and answering them (D-070)

Players send reviews from the launcher (More → Leave a review) to
`reviews.lapi.sh`. Nothing is readable over HTTP: reviews are read and answered
only on the server, with the service's command line. The repository and its
workflow logs are public, so nothing a player wrote goes into issues, pull
requests or workflow logs; the "Reviews service" workflow with `task=counts`
shows numbers only.

## Where the data is

| What    | Where                                     | Kept                          |
| ------- | ----------------------------------------- | ----------------------------- |
| Reviews | `/home/www/lapi/reviews/data/reviews.db`  | 180 days (suspect: 14 days)   |
| Files   | `/home/www/lapi/reviews/data/files/<id>/` | 90 days                       |
| Bans    | the same database, table `bans`           | until lifted or their end     |
| Copies  | `/home/www/lapi/reviews/data/backups/`    | the newest 14, made daily     |
| Secret  | `/home/www/lapi/reviews/data/secret`      | signs challenges, hashes nets |

## Commands

Log in as the deploy user (allowed without a password) or as an administrator,
then:

```sh
r() { sudo -u lapi-reviews /home/www/lapi/reviews/current/lapi-reviews "$@"; }

r overview --days 7          # numbers by kind, category and status
r list                       # newest first, suspected spam left out
r list --status new          # new|read|answered|closed|spam
r list --kind bug --category launch --days 30 --limit 100
r list --spam                # with the suspected spam
r show 42                    # everything about review 42 (marks it read)
r show 42 --translate ru     # and its message in Russian, through DeepL
r show 42 --file latest.log  # one text file as sent
r reply 42 Thank you, this is fixed in 0.9.2.
r reply 42 --from answer.txt # a longer answer from a file
r status 42 closed           # new|read|answered|closed|spam
r note 42 duplicate of 17    # for you; the player does not see it
r remove 42                  # gone for good, with its files
```

A picture is taken off the server to look at it (the command refuses to print
it on a terminal):

```sh
ssh <user>@<server> "sudo -n -u lapi-reviews /home/www/lapi/reviews/current/lapi-reviews show 42 --file screenshot-1.png" > shot.png
```

Output is cleaned of control and direction characters, so a review cannot
drive the terminal.

## Answers

`reply` stores the answer and sets the review to `answered`. The launcher asks
for answers when the player opens their reviews and at most every 6 hours while
one is open, and marks More with a dot when a new answer is there. The answer
is shown as plain text; links are not clickable. `status <id> closed` ends a
conversation (the answer stays visible).

## Spam and abuse

- Reviews with many links, hardly any letters, or the text of another review
  from the same network within a day are kept as suspect: `list` leaves them
  out, `list --spam` shows them, and they go after 14 days.
- `ban install|account|net <review> [days] [--reason TEXT]` bans whoever sent a
  review: its install ID, launcher account or network (an address or its /64,
  kept only as a keyed hash for 30 days). Without days, for good. The review is
  set to `spam`. A banned sender is told "sent" as anyone is; nothing is kept.
- `bans` lists bans in force; `lift <ban>` ends one.
- `pause` stops taking reviews (the launcher keeps them and tries later);
  `resume` takes them again.
- `pow` shows the proof of work's difficulty, `pow 22` raises it (each bit
  doubles the work; 20 is about a second on a player's computer, 28 at most).

Limits that hold without you: 5 reviews an hour and 15 a day per network (an
address or its IPv6 /64), five times that per IPv6 /48, 10 a day per install
and per account, 300 an hour and 1000 a day in all, 10 GiB of files.

## Privacy

- Copy a file off the server only to work on it, and check it for names,
  addresses and server names before it goes anywhere else.
- A player can delete their own review in the launcher. Asked by mail, delete
  by install ID (Settings → Privacy shows it): `r forget <install-id>`.
- Accounts → _Download my data_ includes the reviews sent with the launcher account
  (`GET /v1/export`): what was written, the answers, and the files' names and sizes.
  Notes made with `note`, spam marks and network hashes are not in it.
- `--translate` sends the message to DeepL; use it only when needed.
