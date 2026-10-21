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
   version → Create installation → Install (allowing the download if asked) → Play.
4. In the game, through window messages: mouse clicks only, since 26.x reads input
   through SDL, which gives keys only to the focused window, while mouse messages reach
   it in the background (so a locked screen does not stop it). Continue on the
   first-start accessibility screen, Singleplayer, Create (the world list is empty, so
   the game opens Create New World at once), some seconds in the world, then the window
   is closed. The game log must show the player joining; the game must exit with 0 and
   leave no crash report. Screenshots are taken along the way (the game's F2 when its
   window could be brought to the front, else a copy of the screen region).

The run page shows a table per version (install, launch, world, exit, log errors). The
artifact `usertest-results` has a folder per version with the screenshots, the game's
`latest.log` and any crash report, machine names replaced; on a launcher failure also
what the launcher window showed. The launcher is uninstalled at the end; its data folder
is removed at the next run's start.
