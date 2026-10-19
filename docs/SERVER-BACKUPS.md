# Server backups

The account service (oauth.lapi.sh) and friends (friends.lapi.sh) keep everything in one
SQLite database each. Once a day a timer makes a consistent copy of each, while the
service runs:

| Service  | Copies                                             | Timer                       |
| -------- | -------------------------------------------------- | --------------------------- |
| accounts | `/home/www/lapi/oauth/data/backups/accounts-*.db`  | `lapi-oauth-backup.timer`   |
| friends  | `/home/www/lapi/friends/data/backups/friends-*.db` | `lapi-friends-backup.timer` |

The newest 14 copies stay. Each copy can be read only by its service's user. Both
timers are installed by `deploy/bootstrap.sh`. A copy can also be made at any time:

```sh
sudo -u lapi-oauth /home/www/lapi/oauth/current/lapi-oauth backup
sudo -u lapi-friends /home/www/lapi/friends/current/lapi-friends backup
```

What a copy holds, and what it does not:

- Passwords are Argon2id hashes. Sign-in codes, refresh tokens and email links are
  stored only as SHA-256, so a copy gives no working token.
- Letters still waiting to be sent are sealed with `mail.key`, which is not in the
  copy. Sent letters keep no body.
- The signing key (`signing.key`) and `mail.key` are not in the copy. After a restore
  without them the service makes new ones: everyone stays signed in (the launcher gets a
  new access token on its next refresh), and letters that were waiting are dropped.
- Accounts keep the address and country they registered from, and chats keep every
  message: treat the copies as personal data.

## Off the server

The copies sit on the same disk as the databases, so they protect against a mistake or
a damaged database, not against losing the server. To survive that, copy the folders
somewhere else regularly, for example from your own computer:

```sh
rsync -a --delete <admin>@<server>:/home/www/lapi/oauth/data/backups/ ./lapi-backups/oauth/
rsync -a --delete <admin>@<server>:/home/www/lapi/friends/data/backups/ ./lapi-backups/friends/
```

Reading the folders needs root on the server (they belong to the service users); keep
the copies encrypted where they land. Never copy them through a GitHub workflow: the
repository's logs and artifacts are public.

## Restoring

```sh
sudo systemctl stop lapi-friends lapi-oauth
sudo install -o lapi-oauth -g lapi-oauth -m 0600 accounts-<time>.db /home/www/lapi/oauth/data/accounts.db
sudo rm -f /home/www/lapi/oauth/data/accounts.db-wal /home/www/lapi/oauth/data/accounts.db-shm
sudo install -o lapi-friends -g lapi-friends -m 0600 friends-<time>.db /home/www/lapi/friends/data/friends.db
sudo rm -f /home/www/lapi/friends/data/friends.db-wal /home/www/lapi/friends/data/friends.db-shm
sudo systemctl start lapi-oauth lapi-friends
```

Restore both from the same day when you can: friends refers to accounts by ID.
