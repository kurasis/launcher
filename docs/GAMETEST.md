# Game test on the test machine

The **Game test** workflow (`.github/workflows/gametest.yml`) plays the real game on the
owner's Windows x64 test machine and reports what happened. Each scenario is one game
version × loader × mod set, played by a test bot for a set number of minutes.

## What a scenario does

1. The harness (`src-tauri/tests/live_gametest.rs`, built on a GitHub runner) installs the
   game version with its loader through the launcher's own install service, into a data
   folder kept between runs (`%LOCALAPPDATA%\launcher-gametest`, or `GAMETEST_ROOT`).
2. It puts into `mods/`:
   - the test bot (`mod/gametest/`, jar `launchergametest-<loader>-<version>.jar`);
   - this commit's build of Launcher Skins (renamed `gametest-launcherskins-…` so the
     launcher leaves it alone);
   - for the `medium` set, about twenty popular mods from Modrinth with the libraries they
     need (`tests/gametest/mods.rs`), each file checked against Modrinth's SHA-512.
3. It serves test skins from the machine itself (`tests/gametest/skins.rs`), the way
   skins.lapi.sh answers: HD skins 64 to 512, slim and wide, animated strips, static and
   animated capes. Each scenario plays as one test player (`GT_<variant>`) whose variant
   rotates over the scenarios.
4. It starts the game through the launch service, offline in the full game, and waits.

The bot (`mod/gametest/core/Bot.java`) then:

- waits for the title screen and notes every screen before it: a loader's warning or
  error screen fails the check `title-screen` and is screenshotted;
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

## Results

Each scenario's job adds a summary (checks, frames per second per phase, mods, log error
count with the first error lines) to the run page, and uploads an artifact with the bot's
report, its screenshots, the game's `latest.log` and the launcher's `output.log`, and any
crash reports. Everything copied out of the game folder has the machine's user name, home
folder, computer name and data folder replaced first: the repository and its artifacts are
public.

## Running it

"Run workflow" on the Game test workflow, on `main`. Inputs pick the versions (26.3,
1.21.1), loaders (fabric, neoforge, forge), mod sets (none, medium), minutes per scenario
and the skin variant (`rotate` gives each scenario the next one). It also runs every night
at 03:00 Moscow time with the defaults: 12 scenarios of 5 minutes.

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
cd mod && ./gradlew :gametest:mc-26.3:fabric:build :mc-26.3:fabric:build
cd ../src-tauri
LAUNCHER_LIVE_ROOT=<data dir> GAMETEST_VERSION=26.3 GAMETEST_LOADER=fabric \
GAMETEST_JARS=<folder with the two jars> GAMETEST_OUT=<results> GAMETEST_MINUTES=2 \
cargo test --test live_gametest -- --ignored --nocapture
```

On Linux under Xvfb, set `SDL_VIDEO_FORCE_EGL=1` for 26.x (see `mod/README.md`).
