# Player test on the test machine

The **Player test** workflow (`.github/workflows/usertest.yml`) checks game versions the
way a player meets them, on one of the owner's Windows x64 test machines. It runs only by
hand ("Run workflow"); the input `versions` takes release ids, or `26` for every 26.x
release in Mojang's list. The inputs `label` (default `TestPC`) and `machine` (default
`amd-bosgame-8gb`) pick the computer: a job goes to any free runner with the label, so
it first checks the runner's name and stops at once on another machine. A label only
that machine has makes the pick exact.

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
   download if asked) → Play. The launcher closes its window while a game runs and makes
   a new one after it, so the window is looked up again for each version.
4. In the game, with the mouse and keys as a player uses them: 26.x reads input through
   SDL3, which takes no posted mouse messages and keys only while its window has the
   focus, so the game window is brought to the front and real clicks and key presses are
   sent. The test machine therefore needs a signed-in, unlocked desktop. The game log's
   version must be the one asked for. Continue on the first-start accessibility screen
   (and Escape, which leaves it if the click missed), Singleplayer, Create (the world list
   is empty, so the game opens Create New World at once); without a world after that the
   keyboard way is tried (Escape, Tab, Enter, Enter). Some seconds in the world, then the
   window is closed. The game log must show the player joining; the game must exit with 0
   and leave no crash report. Screenshots are taken along the way (the game's F2 and a
   copy of the screen region).

The run page shows a table per version (install, launch, world, exit, log errors). The
artifact `usertest-results` has a folder per version with the screenshots, the game's
`latest.log` and any crash report, machine names replaced; on a launcher failure also
what the launcher window showed. The launcher is uninstalled at the end; its data folder
is removed at the next run's start. A run cancelled halfway leaves the test launcher (and
maybe the game) running: the next run on that machine stops what runs from its folder and
puts the machine back first, and a run with `cleanup` does only that.
