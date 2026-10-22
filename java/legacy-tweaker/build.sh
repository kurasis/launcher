#!/bin/sh
# Builds legacy-tweaker.jar from this folder (JDK 21: javac
# for Java 8, a jar with fixed dates and no manifest, so the same source gives
# the same bytes). CI checks the committed jar against a fresh build.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
javac --release 8 -encoding UTF-8 -nowarn -d "$out/stubs" $(find "$here/stubs" -name '*.java' | sort)
javac --release 8 -encoding UTF-8 -Xlint:all,-options -Werror -cp "$out/stubs" -d "$out/classes" $(find "$here/src" -name '*.java' | sort)
jar --create --no-manifest --date=2026-01-01T00:00:00Z \
  --file "$here/legacy-tweaker.jar" -C "$out/classes" app
