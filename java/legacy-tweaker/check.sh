#!/bin/sh
# Starts stand-in games through Mojang's launchwrapper (Java 8 at $JAVA8):
# - before 1.6 (launchwrapper 1.5, its default tweaker): the game thread must see
#   the game folder; without the tweaker it sees none, since launchwrapper sets it
#   only at the end of main;
# - the oldest games (AlphaVanillaTweaker, launchwrapper 1.5 and 1.6): a window its
#   player closes ends the game with code 1, and only the launcher's tweaker says so
#   first. These open a window, so without a display the script runs under xvfb-run.
set -eu
[ -n "${DISPLAY:-}" ] || exec xvfb-run -a "$0" "$@"
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fetch() {
  curl -fsSL --retry 3 -o "$work/$(basename "$1")" "https://libraries.minecraft.net/$1"
  echo "$2  $work/$(basename "$1")" | sha1sum -c --quiet
}
fetch net/minecraft/launchwrapper/1.5/launchwrapper-1.5.jar 5150b9c2951f0fde987ce9c33496e26add1de224
fetch net/minecraft/launchwrapper/1.6/launchwrapper-1.6.jar 4ea0aca9c022a234ebaf14b51fb119055955fc9d
fetch org/ow2/asm/asm-all/4.1/asm-all-4.1.jar 054986e962b88d8660ae4566475658469595ef58
fetch net/sf/jopt-simple/jopt-simple/4.5/jopt-simple-4.5.jar 6065cc95c661255349c1d0756657be17c29a4fd3
mkdir "$work/game" "$work/folder"
# ASM 4.1 reads class files up to Java 7, as the games' own are.
"$JAVA8/bin/javac" -source 1.7 -target 1.7 -nowarn -d "$work/game" "$here"/check/net/minecraft/client/*.java

libs="$work/asm-all-4.1.jar:$work/jopt-simple-4.5.jar:$work/game:$here/legacy-tweaker.jar"
run() {
  (cd "$work" && "$JAVA8/bin/java" -Djava.awt.headless=true -cp "$work/launchwrapper-1.5.jar:$libs" \
    net.minecraft.launchwrapper.Launch Player token --gameDir "$work/folder" "$@" 2>&1) | grep '^game thread folder: ' || true
}
mojang=$(run)
ours=$(run --tweakClass app.launcher.legacy.LegacyTweaker)
echo "Mojang's tweaker: ${mojang#game thread folder: }"
echo "the launcher's tweaker: ${ours#game thread folder: }"
[ "$mojang" = "game thread folder: null" ]
[ "$ours" = "game thread folder: $work/folder" ]

# Prints the exit code of a closed stand-in window and whether the launcher's line came.
closed() {
  code=0
  out=$(cd "$work" && timeout 60 "$JAVA8/bin/java" -cp "$work/launchwrapper-$1.jar:$libs" \
    net.minecraft.launchwrapper.Launch Player token --gameDir "$work/folder" --tweakClass "$2" 2>&1) || code=$?
  case "$out" in
    *"Launcher: the game window was closed"*) echo "$code said" ;;
    *) echo "$code silent" ;;
  esac
}
for wrapper in 1.5 1.6; do
  mojang=$(closed "$wrapper" net.minecraft.launchwrapper.AlphaVanillaTweaker)
  ours=$(closed "$wrapper" app.launcher.legacy.AlphaTweaker)
  echo "launchwrapper $wrapper, window closed: Mojang's tweaker $mojang, the launcher's $ours"
  [ "$mojang" = "1 silent" ]
  [ "$ours" = "1 said" ]
done
