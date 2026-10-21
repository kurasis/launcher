# Launcher Skins

The launcher's own skins mod (docs/DECISIONS.md D-065): a launcher account's skin and
cape by nick from `https://skins.lapi.sh`, HD (64×64 × 1, 2, 4 or 8) and animated
(a vertical strip of up to 32 frames). It only replaces a player's skin when the game has
none for them: a profile without textures whose UUID is not a Mojang one (version 4).

## Layout

- `core/`: plain Java 8 with no game classes: the API client, the disk cache by
  SHA-256, the texture rules, frame timing and the whose-skin rule. Its tests run in
  `build`.
- `mc-<versions>/common/`: the game hook for the game versions in the folder's name,
  which share the same game code (a Mixin into `PlayerInfo.getSkin()`), written once in
  Mojang names. `mc-1.21.11` takes its code from `mc-26.3/common`.
- `mc-<versions>/{fabric,neoforge,forge}/`: thin entry points, one jar per loader for
  every version in the folder's name (`launcherskins-<loader>-<first>-<last>.jar`). Each
  loader build takes `core` and `common` as its own sources, so every jar stands alone.
  The Fabric jar also runs on Quilt.
- `mc-1.16.5-1.20.1/`: one plain Java 8 game layer for 1.16 to 1.20.1. What it uses keeps
  its intermediary names (Fabric) and SRG names (Forge 1.17.1 on) from release to
  release, so the jar built against 1.20.1 is byte for byte the one built against any of
  them: one Fabric jar serves 1.16 to 1.20.1 and one Forge jar 1.17.1 to 1.20.1 (ModDevGradle's
  legacy plugin, reobfuscated to SRG). Forge 1.16.5, the first Forge with Mixin, has a jar
  of its own (Architectury Loom). Each is a project in the folder of the release it is
  built against, whose gradle.properties name its band and range.
- 1.20.2 to 1.20.6 use `mc-1.21.1/common` (the skin is a `PlayerSkin` from 1.20.2 on): its
  Fabric jar serves 1.20.2 to 1.21.1 and its NeoForge jar 1.20.6 to 1.21.1.
  `mc-1.20.2-1.20.6/` builds the rest with Architectury Loom (`arch.gradle`): Forge
  1.20.2 to 1.20.4 (SRG names, which keep their numbers across these releases), Forge
  1.20.6 and NeoForge 1.20.2 to 1.20.4 (Mojang names; NeoForge before 20.5 reads
  `mods.toml`).
- `mc-1.12.2/forge/`: Forge 1.12.2 has no Mixin, so the hook is a coremod: an ASM
  transformer that passes `NetworkPlayerInfo`'s skin, cape, elytra and model through
  `LauncherSkins` (methods matched by MCP name in development, SRG name in a released
  game). Built with RetroFuturaGradle; the jar to ship is the reobfuscated one, not
  `-dev`.
- Java agents for the game with no mod loader
  (`-javaagent:launcherskins-agent-<version>.jar`). They rewrite `PlayerInfo`'s skin
  getters the way the Mixin does and use the same game layer, so HD and animation work
  too. 26.x names are not obfuscated: `mc-26.3/agent` rewrites the class with the JDK's
  class-file API. Older releases have one jar each (`agents/<version>/`, built by
  `agents/agent.gradle` from `agent/`), remapped to that release's names with its own
  ASM (relocated); it reads its targets from `AgentHooks.targets`, whose calls the
  remapping rewrites.

- `gametest/`: the test bot for the Game test workflow (docs/GAMETEST.md), never shipped
  to players: a plain Java core (`gametest/core`, the scenario and its checks) and a thin
  game layer per tested version with a Mixin into `Minecraft.tick()`, built for each
  loader as `launchergametest-<loader>-<game version>.jar`. It does nothing unless the
  game is started with `-Dlaunchergametest.out=<folder>`.

## Build

Gradle runs on Java 25 (Fabric Loom and RetroFuturaGradle need it); the jars for 1.12.2
to 1.20.1 are built for Java 8 (with Java 8 and 17 toolchains), those for 1.20.2 to 1.21.1
with Java 17 (1.20.6's Forge jar 21), the later 1.21.x with Java 21 and 26.x with Java 25. Missing toolchains are downloaded.

```
./gradlew build
./gradlew -Ponly=mc-1.21.11,agents build   # just these folders (and the cores)
```

Every Forge release is downloaded and decompiled when its project is configured, before
any task runs; `-Ponly` skips the folders not named. The jars are
`**/build/libs/launcherskins-<loader>-<game versions>-<version>.jar`.

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
