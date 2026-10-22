#!/bin/bash
# Moves the files of our services (/home/www/lapi: releases, databases, the
# mirror's files, update archives, backups) to a larger disk and mounts them
# back at /home/www/lapi with a bind mount. Every path stays as it was, so
# the units, nginx, sudo rights and deploys need no change, and systemd's
# sandboxing sees the same folders (symbolic links would not pass it).
#
#   sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user> --storage /data
#   sudo bash ~/lapi-bootstrap/storage.sh /data      (the same, on its own)
#
# The files go to <folder>/lapi (here /data/lapi), mounted by the systemd
# unit home-www-lapi.mount at boot. Steps: a copy while the services run; the
# services stop; a second copy of what changed; a check that both are the
# same; the mount; the services start and must answer. Then the old files go,
# which frees /home. If a service does not answer, everything goes back as it
# was and the copy stays for a look. Downtime is the second copy only.
# Safe to run again: it does nothing once the files are there.
set -euo pipefail

PARENT="${1:?usage: sudo bash storage.sh <folder on the larger disk, such as /data>}"
ROOT=/home/www/lapi
OLD=$ROOT.moved
UNIT_NAME=home-www-lapi.mount
UNIT=/etc/systemd/system/$UNIT_NAME
MARK=.lapi-storage

[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo)"; exit 1; }
if ! [[ "$PARENT" =~ ^/[A-Za-z0-9._/-]*$ ]] || [[ "$PARENT" == *..* ]]; then
    echo "$PARENT: give an absolute folder, such as /data"
    exit 1
fi
[ -d "$PARENT" ] || { echo "no folder $PARENT"; exit 1; }
PARENT=$(realpath -e "$PARENT")
STORE=${PARENT%/}/lapi
case "$STORE/" in
    /home/*) echo "$STORE is under /home: give a folder on the larger disk"; exit 1 ;;
esac
step() { printf '\n== %s\n' "$*"; }

if [ -f "$UNIT" ]; then
    current=$(sed -n 's/^What=//p' "$UNIT")
    if [ "$current" != "$STORE" ]; then
        echo "$ROOT is mounted from $current already; nothing done"
        exit 1
    fi
    systemctl is-active --quiet "$UNIT_NAME" || systemctl enable --now "$UNIT_NAME"
    mountpoint -q "$ROOT" || { echo "$ROOT is not mounted from $STORE"; exit 1; }
    echo "$ROOT is on $STORE already"
    exit 0
fi
[ ! -e "$OLD" ] || { echo "$OLD is left from a move that stopped: look at it first"; exit 1; }
if [ ! -d "$ROOT" ] || mountpoint -q "$ROOT"; then
    echo "$ROOT is not a plain folder"
    exit 1
fi
inner=$(findmnt -rn -o TARGET | grep "^$ROOT/" || true)
[ -z "$inner" ] || { echo "a disk is mounted inside $ROOT ($inner): move it by hand"; exit 1; }
[ "$(stat -c %d "$PARENT")" != "$(stat -c %d "$ROOT")" ] ||
    { echo "$PARENT is on the same disk as $ROOT: moving frees nothing"; exit 1; }

# Root runs what is in there: no other user may own or write a folder on the
# way to it, or they could put their own files in its place.
dir=$PARENT
while :; do
    read -r owner mode < <(stat -c '%U %a' "$dir")
    if [ "$owner" != root ] || (( 8#$mode & 8#022 )); then
        echo "$dir must belong to root and be writable by root alone (it is $owner, $mode)"
        exit 1
    fi
    [ "$dir" = / ] && break
    dir=$(dirname "$dir")
done
# A folder of that name is used only if an earlier run of this made it.
if [ -e "$STORE" ] && [ ! -f "$STORE/$MARK" ] && [ -n "$(ls -A "$STORE")" ]; then
    echo "$STORE holds other files: move them away or give another folder"
    exit 1
fi

need=$(du -sx -B1 "$ROOT" | cut -f1)
free=$(df -B1 --output=avail "$PARENT" | tail -n 1)
if (( free < need + (1 << 30) )); then
    echo "$PARENT has $((free >> 20)) MB free; the files need $((need >> 20)) MB and 1 GB to spare"
    exit 1
fi
command -v rsync >/dev/null || apt-get install -y --no-install-recommends rsync >/dev/null
echo "moving $((need >> 20)) MB from $ROOT to $STORE"

install -d -o root -g root -m 0755 "$STORE"
touch "$STORE/$MARK"
copy() { rsync -aH --numeric-ids --delete --exclude="/$MARK" "$@" "$ROOT/" "$STORE/"; }

step "copy while the services run"
# 24: a file went away during the copy (the mirror replaces files); the
# second copy takes what changed.
copy || [ $? -eq 24 ]

step "stop the services"
mapfile -t RUNNING < <(systemctl list-units --type=service,timer --state=active,activating \
    --plain --no-legend 'lapi-*' | awk '{print $1}')
echo "running: ${RUNNING[*]:-none}"
start_again() {
    # Timers last: the mirror's warming needs the mirror.
    local unit
    for unit in "${RUNNING[@]}"; do
        if [[ "$unit" == *.service ]]; then systemctl start "$unit" || true; fi
    done
    for unit in "${RUNNING[@]}"; do
        if [[ "$unit" == *.timer ]]; then systemctl start "$unit" || true; fi
    done
}
trap 'echo "stopped before the switch: the services start again on $ROOT"; start_again' ERR
systemctl stop 'lapi-*.timer' 'lapi-*.service'

step "copy what changed"
copy
[ -z "$(copy -n --itemize-changes)" ] || { echo "the copy differs from $ROOT"; false; }
echo "the copy is the same as $ROOT"

step "mount $STORE at $ROOT"
trap - ERR
mv "$ROOT" "$OLD"
install -d -o root -g root -m 0755 "$ROOT"
cat > "$UNIT" <<EOF
# Written by deploy/storage.sh: the services' files are on the larger disk.
[Unit]
Description=Launcher services' files ($STORE)
RequiresMountsFor=$STORE

[Mount]
What=$STORE
Where=$ROOT
Type=none
Options=bind

[Install]
WantedBy=local-fs.target
EOF
systemctl daemon-reload
back() {
    echo "WARNING: $1; going back to $ROOT as it was"
    systemctl stop 'lapi-*.timer' 'lapi-*.service' || true
    systemctl disable --now "$UNIT_NAME" || true
    rm -f "$UNIT"
    systemctl daemon-reload
    mountpoint -q "$ROOT" || rmdir "$ROOT"
    mv "$OLD" "$ROOT"
    start_again
    echo "back on $ROOT; the copy in $STORE stays for a look"
    exit 1
}
systemctl enable --now "$UNIT_NAME" || back "the mount failed"
if ! mountpoint -q "$ROOT" || [ ! -f "$ROOT/$MARK" ]; then back "$ROOT is not the copy"; fi

step "start the services"
start_again
port_of() { case "$1" in lapi-crash.service) echo 8781 ;; lapi-mirror.service) echo 8782 ;; lapi-oauth.service) echo 8783 ;; lapi-friends.service) echo 8784 ;; lapi-skins.service) echo 8785 ;; lapi-mods.service) echo 8786 ;; lapi-feed.service) echo 8787 ;; lapi-worlds.service) echo 8788 ;; lapi-servers.service) echo 8789 ;; lapi-reviews.service) echo 8790 ;; lapi-stats.service) echo 8791 ;; esac; }
answers() {
    for _ in $(seq 1 30); do
        curl -fsS --max-time 2 "http://127.0.0.1:$1/v1/health" >/dev/null 2>&1 && return 0
        sleep 1
    done
    return 1
}
for unit in "${RUNNING[@]}"; do
    port=$(port_of "$unit")
    [ -n "$port" ] || continue
    answers "$port" || back "$unit does not answer"
    echo "$unit answers"
done

step "remove the old files"
rm -rf --one-file-system "$OLD"
df -h /home "$PARENT"
echo "Done: $ROOT is $STORE, mounted at every boot by $UNIT_NAME."
