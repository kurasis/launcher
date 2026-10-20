# Launcher Skins

The launcher's own skins mod (docs/DECISIONS.md D-065): a launcher account's skin and
cape by nick from `https://skins.lapi.sh`, HD (64×64 × 1, 2, 4 or 8) and animated
(a vertical strip of up to 32 frames). It only replaces a player's skin when the game has
none for them: a profile without textures whose UUID is not a Mojang one (version 4).

## Layout

- `core/`: plain Java 8 with no game classes: the API client, the disk cache by
  SHA-256, the texture rules, frame timing and the whose-skin rule. Its tests run in
  `build`.
- `mc-<version>/common/`: the game hook for one game version (a Mixin into
  `PlayerInfo.getSkin()`), written once in Mojang names.
- `mc-<version>/{fabric,neoforge,forge}/`: thin entry points. Each loader build takes
  `core` and `common` as its own sources, so every jar stands alone. The Fabric jar
  also runs on Quilt.
- `mc-1.12.2/forge/`: Forge 1.12.2 has no Mixin, so the hook is a coremod: an ASM
  transformer that passes `NetworkPlayerInfo`'s skin, cape, elytra and model through
  `LauncherSkins` (methods matched by MCP name in development, SRG name in a released
  game). Built with RetroFuturaGradle; the jar to ship is the reobfuscated one, not
  `-dev`.
- `mc-<version>/agent/`: a Java agent for the game with no mod loader
  (`-javaagent:launcherskins-agent-<version>.jar`). It rewrites `PlayerInfo.getSkin()`
  the way the Mixin does and uses the same game layer, so HD and animation work too.
  26.x names are not obfuscated and the JDK's class-file API rewrites the class; the
  1.21.1 jar is remapped to the released game's names and carries its own ASM
  (relocated), and finds its target from the remapped descriptor of its hook.

- `gametest/`: the test bot for the Game test workflow (docs/GAMETEST.md), never shipped
  to players: a plain Java core (`gametest/core`, the scenario and its checks) and a thin
  game layer per version with a Mixin into `Minecraft.tick()`, built for each loader as
  `launchergametest-<loader>-<game version>.jar`. It does nothing unless the game is
  started with `-Dlaunchergametest.out=<folder>`.

## Build

Gradle runs on Java 25 (Fabric Loom and RetroFuturaGradle need it); the jar for 1.12.2
is built with a Java 8 toolchain, those for 1.21.x with Java 21 and for 26.x with
Java 25. Missing toolchains are downloaded.

```
./gradlew build
```

The jars are `mc-*/*/build/libs/launcherskins-<loader>-<game version>-<version>.jar`.

## Trying it in the game

Each loader project has a `runClient` task. These properties are optional:

- `-PskinsApi=http://127.0.0.1:8899`: another API address (system property
  `launcherskins.api`); plain `http` only for a loopback address.
- `-PdevNick=Tester`: the player's nick.
- `-PdevServer=127.0.0.1:25565`: join this server at once (an offline-mode server
  gives the player a non-Mojang UUID, so the mod applies). On 1.12.2 it becomes
  `--server` and `--port`.
- `-PdevWorld=<save>` (Forge 1.21.1 and 26.3): open this singleplayer world at once.

From 26.x the game opens its window through SDL. On a virtual display (Xvfb) set
`SDL_VIDEO_FORCE_EGL=1`: Xvfb's GLX has no sRGB visuals, which the game asks for.
Forge 26.3 in a dev run stops on a vanilla server (its own config is read before it is
loaded); join a Forge server instead.
