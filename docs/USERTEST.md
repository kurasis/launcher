# Player test on the test machines

The **Player test** workflow (`.github/workflows/usertest.yml`) checks game versions the
way a player meets them, on the owner's Windows x64 test machines. It runs only by hand
("Run workflow"). The input `versions` takes, comma-separated: `26` (every 26.x
release), `all` (every Mojang release vanilla, and with Fabric, Quilt, Forge and
NeoForge every release each supports), a loader name alone (`vanilla`, `fabric`,
`quilt`, `forge`, `neoforge`: every release it supports), a release id (vanilla) or
`loader:id`. Which releases a loader supports comes from the lists the launcher reads,
with its rules (`scripts/usertest/plan.py` mirrors `src-tauri/src/loader/meta.rs`). The
items go newest release first and are cut into parts of `part_size` (default 12, about
45 minutes each); each part is a job of its own.

The launcher's own versions, the ones Mojang's list does not have (docs/EXTRA-VERSIONS.md,
the form's Special tab), are picked with `special` (all of them), `special:<group>`
(`joke`, `unobfuscated`, `experimental`, `combat`, `first-build`, `early-release`,
`beta`, `alpha`, `infdev`, `indev`, `classic`) or `special:<id>`. They come after the
Mojang items, in the list's order. The plan reads the list the launcher reads
(`upd.lapi.sh/versions/manifest.json`) for each version's name: its row starts with it and
the form says "Selected: <name>" (an earlier build: its id). The test opens the Special
tab, types the id into the search box, installs and plays as for a release, and stops at
the title screen: the game files come from the launcher's server, and some of these games
have menus of their own (the oldest none), so no world is made (World: `not checked`). A
special version passes when the game reached its title screen and closed normally, and
the launcher did not add Launcher Skins to it.

The inputs `label` (default `TestPC`) and `machine` pick the computers: a job goes to any
free runner with the label, so it first checks the runner's name and stops at once on
another machine. `machine: any` lets every machine with the label take parts, which is
how a long list is shared between them. A label only one machine has makes the pick
exact.

The input `downloads` is the launcher's Settings → Network choice, written into the
fresh data folder before the first start: `automatic` (default, what a new player has:
the origin first, our mirror only when it fails or is very slow, as the project's
mirror rule wants), `mirror_first` (the mirror, the origin when it fails) or
`origin_only`.

1. A GitHub runner takes the installer from the newest green Package run on `main`
   (`launcher-windows-x64`) and the interface texts of every language.
2. The test machine installs it silently into `%LOCALAPPDATA%\launcher-usertest\app`
   with `portable.txt` beside it, so its data stays apart from the owner's launcher. The
   run stops at once when a launcher is open there (the installer would close it). The
   owner's uninstall entry and shortcuts are put back afterwards.
3. In the launcher window, through UI Automation as a player clicks: Accounts → Offline
   and a profile `Tester`; then for each version Installations → New installation → the
   version typed into the search box and clicked (the form's "Selected" line must name
   it, since the newest is chosen at first) → Create installation → Install (allowing the
   download if asked) → Play. With a loader, the loader is chosen in the form first (the
   version list then shows only what it supports), and the loader version the form
   offers (the recommended or newest stable one) is kept. The launcher closes its window while a game runs and makes
   a new one after it, so the window is looked up again for each version.
4. In the game, with the mouse and keys as a player uses them: 26.x reads input through
   SDL3, which takes no posted mouse messages and keys only while its window has the
   focus, so the game window is brought to the front and real clicks and key presses are
   sent. The test machine therefore needs a signed-in, unlocked desktop. The window title
   (and from 26.x the log) must name the version asked for. The buttons sit at the same
   spots in every version since 1.0, at the GUI scale the game picks for its window:
   Continue on the first-start accessibility screen (1.19.4 and later; Escape, which
   leaves it too, for every version), Singleplayer, Create New World in the world list
   (newer versions open Create New World at once for an empty list, and the spot is
   empty there), Create, and from 1.19.3 Yes when the game asks to confirm experimental
   features. Before 1.7 the world list drops the mouse events of a frame drawn without
   the button down, which loses synthetic clicks, so the game's own touchscreen mode is
   set in a fresh `options.txt` for those versions. That mode came in 1.4.6; before it a
   click counts only when a game tick reads it before the next frame is drawn. Those
   versions take the window's messages in just before a frame, unless F7 (their
   debugging key) is held, when they take them in after it; so F7 is held around each
   click, and they also get the power saver frame limit (a tick due about every second
   frame) and eight clicks. Without a world after that the
   keyboard way is tried (Escape, Tab, Enter, Enter). Some seconds in the world, then the window is closed. The game's
   log (or for versions before 1.7, the output the launcher kept) must show the player
   joining (1.4.6 to 1.6.x log it as the player logged in); up to 1.4.5, whose server
   inside logs nothing (before 1.3 there was none), a file the new world wrote counts
   (when it fails, `saves.txt` lists what the game saved), and the game's own F2
   screenshot must appear in the world: those versions show a crash in the window and
   exit normally afterwards, after the world's files were written.
   The game must exit with 0 (before 1.6 with 1 after its "Someone is closing me!",
   which is how those versions end when the window is closed) and leave no crash report. Where the launcher has a
   Launcher Skins build for the version, the column Launcher Skins says whether it got
   in: `not added` when the launcher's log warns it could not put it in, `failed` when
   the game's logs show the mod's agent missing its targets or an error naming
   `launcherskins`, `not loaded` when a loader's logs never name the mod (Fabric and
   Quilt list their mods, Forge and NeoForge scan them), `ok` otherwise; vanilla takes
   it as an agent, so the launcher's line is enough there. Anything but `ok` or empty
   fails the item. A graphics driver that crashed during the item (Windows' System event
   4101 for a driver restarted, a LiveKernelEvent report, or the AMD driver's bug report
   window, which the test closes since it takes the focus from the game) is noted in
   the `driver` field; an item that failed with it is played once more at the end of
   the part, and if it fails again the tables mark it ⚠️ instead of ❌. Screen copies are taken along
   the way; the game's F2 screenshots and logs are kept for a failure only.

Each part's page shows its table; the last job, **All results**, joins them into one
table on the run page and the artifact `usertest-report` (`results.md`, `results.json`),
listing what a failed or cancelled part did not reach as not run. The artifacts
`usertest-results-<part>` have a folder per item with the screen copies and, for a
failure, the logs, the game's screenshots and any crash report, machine names replaced;
on a launcher failure also what the launcher window showed. The launcher is uninstalled at the end; its data folder
is removed at the next run's start. The game files all installations share (libraries
and assets, which the launcher checks before use) are kept in
`%LOCALAPPDATA%\launcher-usertest-cache` between the parts a machine plays, up to
25 GiB, so only the first part on a machine downloads them; a run with `cleanup`
removes them. A run cancelled halfway leaves the test launcher (and
maybe the game) running: the next run on that machine stops what runs from its folder and
puts the machine back first, and a run with `cleanup` does only that.
