#!/usr/bin/env bash
# The CI build of the mod folders named in $ONLY (Skins mod and Game test workflows),
# run from mod/. It lives outside mod/ so that the game test still finds the mod's code
# unchanged since its release.
#
# Setting up a Forge release downloads hundreds of files, and the loaders' mavens now and
# then cut a connection, time out or refuse a file for a while. A build that failed on a
# download runs once more a minute later, with the Maven repositories of the hosts the
# launcher's mirror stands for read through mirror.lapi.sh (mirror.gradle here): the
# origins come first, the mirror is only the fallback. Any other failure fails at once.
set -o pipefail

./gradlew --no-daemon "-Ponly=$ONLY" build 2>&1 | tee gradle.log && exit 0
grep -qE "Could not (GET|HEAD) '|Could not download |Connection reset|Connection timed out|[Rr]equest timed out|Read timed out|(HTTP response code:|status code) [45][0-9][0-9]" gradle.log || exit 1
echo "A download failed: trying once more in a minute, through the mirror"
sleep 60
./gradlew --no-daemon --init-script "$(dirname "$0")/mirror.gradle" "-Ponly=$ONLY" build
