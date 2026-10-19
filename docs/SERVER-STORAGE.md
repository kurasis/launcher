# Where the services keep their files

Everything the services on the server keep lives under `/home/www/lapi/<service>`:
releases, databases, the mirror's files, update archives and the daily backups. When
`/home` is small and another disk (say `/data`) has the room, move it all there once:

```sh
sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user> --storage /data
```

The files go to `/data/lapi` and are mounted back at `/home/www/lapi` with a bind
mount (the systemd unit `home-www-lapi.mount`, mounted at every boot). Every path stays
the same, so the units, nginx, sudo rights, deploys and this repository's docs need no
change. Symbolic links would not do: the services' sandboxing (`ProtectHome`,
`BindPaths`) and nginx would not see the files through them.

What the move does (`deploy/storage.sh`):

1. Checks first, and changes nothing if one fails:
   - `/data` must be on another disk than `/home`;
   - it must have room for the files plus 1 GB;
   - every folder on the way to it must be root's and writable by root alone;
   - no disk may be mounted inside `/home/www/lapi`;
   - `/data/lapi` must be empty, or left by an earlier run.
2. Copies the files while the services run.
3. Stops the services and their timers, then copies what changed. This copy is all
   the downtime, usually seconds.
4. Checks that the copy is the same as `/home/www/lapi`: owners, modes, links and
   contents.
5. Mounts `/data/lapi` at `/home/www/lapi`, starts the services and waits until each
   one answers.
6. Deletes the old files, which frees `/home`.

If a service does not answer, everything goes back as it was and the copy in
`/data/lapi` stays for a look. Do not merge to `main` while it runs: a deploy then
would write to the folder being moved.

Running it again does nothing once the files are there, and later bootstrap runs keep
the mount without `--storage`. The deploy user's `~/.lapi-bootstrap` (the last ten
uploads of `deploy/`, a few MB each) stays in its home.

The mirror's size limit is the `MIRROR_MAX_GB` repository variable, applied by the
next "Mirror service" deploy. With the room on `/data`, it can be raised.

## Moving back

```sh
sudo systemctl stop 'lapi-*.timer' 'lapi-*.service'
sudo systemctl disable --now home-www-lapi.mount
sudo rm /etc/systemd/system/home-www-lapi.mount && sudo systemctl daemon-reload
sudo rsync -aH --numeric-ids --exclude=/.lapi-storage /data/lapi/ /home/www/lapi/
sudo systemctl start lapi-crash lapi-mirror lapi-oauth lapi-friends \
    lapi-mirror-warm.timer lapi-oauth-backup.timer lapi-friends-backup.timer
```
