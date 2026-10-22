# Game test on the test machine

The **Game test** workflow (`.github/workflows/gametest.yml`) plays the real game on the
owner's Windows x64 test machine and reports what happened. Each scenario is one game
version × loader × mod set, played by a test bot for a set number of minutes.

## What a scenario does

1. The harness (`src-tauri/tests/live_gametest.rs`, built on a GitHub runner) installs the
   game version with its loader through the launcher's own install service, into a data
   folder kept between runs (`%LOCALAPPDATA%\launcher-gametest`, or `GAMETEST_ROOT`).
   The loader is its newest stable build, not Forge's "recommended" one, since mods
   follow the newest builds; a kept installation moves to it at the next run. A
   `medium` scenario with a locked set (below) plays the build that set passed on.
2. It puts into `mods/`:
   - the test bot (`mod/gametest/`, jar `launchergametest-<loader>-<version>.jar`);
   - this commit's build of Launcher Skins (renamed `gametest-launcherskins-…` so the
     launcher leaves it alone);
   - for the `medium` set, about twenty popular mods from Modrinth with the libraries they
     need (`tests/gametest/mods.rs`), each file checked against Modrinth's SHA-512 and
     kept on the machine. A file Modrinth does not give comes from our catalog
     (mods.lapi.sh, docs/DECISIONS.md D-066), found by its SHA-1 and checked the same
     way. The versions are the **locked set** for the game version and loader (see
     "Locked mod sets"). With none locked yet, and in the run with the newest mods, each
     mod gets its newest version that fits the loader build: one whose jar accepts it
     (`versionRange` of `forge`/`neoforge` in `mods.toml`, `depends.fabricloader` in
     `fabric.mod.json`; on Quilt, the `fabricloader` and `mixinextras` versions Quilt
     provides, read from its jar's `quilt.mod.json`) and that has not crashed on it on
     this machine. Older releases come next, then newer pre-releases; with none, the mod
     (and any mod that needs it) is left out. Sodium before 0.6 comes with Indium, as
     players add it: mods that draw through Fabric's rendering API stop the game
     without one.

   With no loader (`vanilla`) both are Java agents instead, passed with `-javaagent:`:
   the bot (`launchergametest-agent-<version>.jar`, `mod/gametest/agent/`) rewrites
   `Minecraft.tick()` to call it first, as the loaders' Mixin does, and the skins mod is
   its own agent. Forge 1.12.2, 1.8.9 and 1.7.10 have their own bots
   (`mod/gametest/mc-<version>/forge/`), driven by Forge's client tick event, which turn
   the bot's commands into that release's syntax (1.7.10 has no slim model to check). The
   bots for 1.16.5, 1.17.1, 1.18.2, 1.19.2, 1.19.4 and 1.20.1 live in
   `mod/gametest/mc-1.16.5-1.20.1/` (one game layer per release under `game/`), those for
   1.14.4 and 1.15.2 in `mod/gametest/mc-1.14.4-1.15.2/` (on Forge, which has no Mixin
   there, driven by Forge's client tick event), those for 1.20.4 and 1.20.6 in
   `mod/gametest/mc-1.20.2-1.20.6/`, those for 1.21.x in
   `mod/gametest/mc-<version>/`.

3. It serves test skins from the machine itself (`tests/gametest/skins.rs`), the way
   skins.lapi.sh answers: HD skins 64 to 512, slim and wide, animated strips, static and
   animated capes. Each scenario plays as one test player (`GT_<variant>`) whose variant
   rotates over the scenarios.
4. It starts the game through the launch service, offline in the full game, and waits.

The bot (`mod/gametest/core/Bot.java`) then:

- waits for the title screen and notes every screen before it: a loader's warning or
  error screen fails the check `title-screen` and is screenshotted; a mod's own notice
  (a welcome, "install this mod too") is closed once after 20 seconds, as a player
  would, and listed under the bot's problems without failing the check;
- creates a world with a fixed seed and commands allowed;
- plays in rounds of six phases, 30 seconds each: **skin** (third person from the front
  and back), **walk** (survival, sprinting and jumping), **fly**, **far** (five teleports
  across the map, so new chunks generate), **mobs** (night, thunder, fifteen mobs),
  **build** (blocks, items, the inventory screen);
- every second measures frames per second, chunks, entities, the integrated server's tick
  and memory;
- checks that the skin and cape the skins mod draws are the ones served (model, frames of
  an animated skin or cape, or nothing for the `none` variant);
- closes any screen that a mod opens over the world, writes `report.json` and quits.

A scenario passes when every bot check passes, the game exits with code 0 within the time
limit and leaves no crash report.

A crash the launcher's diagnosis blames on a mod of the `medium` set, or whose stack
trace (crash report, else the log from the exception that ended the game) first reaches
a class of one of its jars, is that mod's problem, not ours: the harness notes that version as crashing on this loader build
(`gametest-cache/incompatible.json` in the data folder, kept between runs), and plays the
scenario again with another version of the mod or without it, up to three plays. The
earlier plays' files are in `attempt-<n>/` of the scenario's folder. To try a remembered
version again, delete its entry (or the file).

Only our own failures turn the run red and hold stable back. Each scenario ends in one
of these, in its `result.json` and the results table:

| Status    | When                                                                                                                                              | The run          |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| passed    | every check passed                                                                                                                                | green            |
| warned    | passed with a mod of the set replaced or left out (it crashed here before, could not be downloaded, or Modrinth did not answer)                   | green, a warning |
| theirs    | another mod crashed the game in all three plays, or the game, the loader or the mod set could not be downloaded: nothing was shown about our mods | green, a warning |
| unchanged | in the run with the newest mods: they are the locked set, already played                                                                          | green            |
| failed    | a bot check failed, or the game crashed on the skins mod, the bot or nothing in the set, or did not close                                         | red              |

## Locked mod sets

`src-tauri/tests/gametest/medium.lock.json` holds, for each game version and loader, the
`medium` set that last passed with the newest versions: the loader build and each mod's
Modrinth version and file with its hashes. The run plays exactly that set, so a mod's
new release does not change what is tested from one night to the next; a locked file
that crashed here since, cannot be downloaded or refuses the build is left out with the
mods that need it (warned).

The nightly run then plays the `medium` scenarios again with the newest versions and the
loader's newest build (job "Play with the newest mods"). A failure there is a warning: it
tells what a mod's new release breaks for players, and holds nothing. A scenario whose
newest versions are the locked ones is not played again. A set that passes, there or in
a `medium` scenario with nothing locked yet, leaves a `lock.json` in its folder; the
job "Lock the mod sets that passed" asks Modrinth for each of its versions' files and
commits them to the lock on `main` as `github-actions[bot]`, the newest run's sets over
the gate's. A set that lacks a mod the locked one has is not locked: the locked set
stays until the mod has a version that passes again. The job's summary lists what
changed.

## Results

All scenarios play one after another in a single job on the test machine, so it downloads
the jars and the harness once per run and uploads the results once. The run page shows a
table of the scenarios with their results, then each scenario's summary (checks, frames per
second per phase, mods, log error count with the first error lines); a failed scenario fails
the job without stopping the rest. The artifact `gametest-results` has a folder per
scenario with the bot's report, its screenshots, the game's `latest.log` and the
launcher's `output.log`, and any crash reports. Everything copied out of the game folder has the machine's user name, home
folder, computer name and data folder replaced first: the repository and its artifacts are
public.

A green nightly run also gives the skins mod release of the tested commit to the stable
channel of the signed list on upd.lapi.sh (docs/DECISIONS.md D-065), when the mod's code
is the release's, beta has it, and at least one scenario was played to the end; a run by
hand does so with "promote". The run with the newest mods has its own results
(`gametest-newest-results`) and does not hold this.

## Running it

"Run workflow" on the Game test workflow, on `main`. Inputs pick the versions (26.3,
1.21.11, 1.21.10, 1.21.8, 1.21.4, 1.21.1, 1.20.6, 1.20.4, 1.20.1, 1.19.4, 1.19.2, 1.18.2, 1.17.1, 1.16.5,
1.15.2, 1.14.4, 1.12.2, 1.8.9, 1.7.10: one release of each group the mod builds a jar for, and from 1.16 to 1.20.1 the
releases where the game's own code changes most), loaders (vanilla, fabric, quilt, neoforge, forge;
Quilt plays the Fabric jars and Fabric mods), mod sets (none, medium), minutes per
scenario and the skin variant (`rotate` gives each scenario the next one; `animcape`
wears a cape that counts 1 to 9 in rainbow colours, so its animation shows in a
screenshot) and "newest" (then play the `medium` scenarios with the newest mods and lock
what passes). 1.12.2, 1.8.9 and 1.7.10 are played with Forge only, 1.14.4 to 1.20.1 without NeoForge, and
the game with no loader without mods. It also runs every night at 03:00 Moscow time with
the defaults (26.3, 1.21.11, 1.21.1, 1.20.1, 1.12.2): 36 scenarios of 5 minutes, then the
`medium` ones with the newest mods.

The workflow never runs for pull requests or from other branches.

## Setting up the test machine (owner)

1. Repository **Settings → Actions → Runners → New self-hosted runner**, Windows x64, and
   run the commands shown there on the test machine. When asked for labels, add
   `gametest`.
2. Start the runner with `run.cmd` **in the signed-in desktop session**, not as a
   Windows service: a service runs without a desktop or the graphics card, and the game
   cannot open its window there. To start it after sign-in, add a Task Scheduler task "At
   log on" that runs `run.cmd` in the runner's folder. Turn off sleep; a locked screen is
   fine.
3. **Settings → Actions → General → Fork pull request workflows from outside
   collaborators → Require approval for all external contributors.** A self-hosted runner
   on a public repository otherwise lets a stranger's pull request run code on this
   machine.
4. Nothing else needs installing: the launcher downloads Java, the harness and jars come
   from GitHub runners. A data folder takes a few gigabytes per game version.

Optional: set the environment variable `GAMETEST_ROOT` for the runner to keep the data
folder elsewhere.

## Running a scenario locally

```
cd mod && ./gradlew -Ponly=mc-26.3,gametest:mc-26.3 build
cd ../src-tauri
LAUNCHER_LIVE_ROOT=<data dir> GAMETEST_VERSION=26.3 GAMETEST_LOADER=fabric \
GAMETEST_JARS=<folder with the two jars> GAMETEST_OUT=<results> GAMETEST_MINUTES=2 \
cargo test --test live_gametest -- --ignored --nocapture
```

`GAMETEST_MODS=medium` adds the mod set (the locked one; `GAMETEST_MOD_VERSIONS=newest`
for the newest versions).

On Linux under Xvfb, set `SDL_VIDEO_FORCE_EGL=1` for 26.x (see `mod/README.md`).
