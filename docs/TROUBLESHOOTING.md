# Troubleshooting

Most problems show a message in the launcher with a suggested next step. This page adds
background for the common ones. When asking for help, attach a diagnostic export
(**Diagnostics → Export…**): it contains no account records, tokens, worlds or game files.

## Starting the launcher

| Symptom                                                                                                     | Cause and fix                                                                                                                                                                                              |
| ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux: exits with "Kurasis Launcher cannot start: The HTTPS client could not be created…"                   | The system has no CA certificates. Install them (`sudo apt install ca-certificates`); the `.deb` package declares this dependency.                                                                         |
| Linux AppImage: `error while loading shared libraries: libasound.so.2` (or `libGLESv2.so.2`, `libgbm.so.1`) | The AppImage uses audio and graphics libraries of the desktop. Install them, for example `sudo apt install libasound2t64 libgles2 libgbm1` on Ubuntu 24.04 (`libasound2` on Debian 12), or use the `.deb`. |
| Linux AppImage does not start without FUSE                                                                  | Run it with `APPIMAGE_EXTRACT_AND_RUN=1 ./Kurasis*.AppImage` or `--appimage-extract-and-run`.                                                                                                              |
| "Launcher data is unavailable — another launcher process is already using the launcher data directory"      | A second launcher process found the first one running but could not bring its window forward (for example from another desktop session). Switch to the existing window or quit it first.                   |
| Windows: "Windows protected your PC"                                                                        | The installer is not signed yet. Check the SHA-256 against `SHA256SUMS`, then **More info → Run anyway**.                                                                                                  |
| macOS: "cannot be opened because the developer cannot be verified"                                          | The app is not signed or notarized yet. Control-click the app → **Open**, or allow it in **System Settings → Privacy & Security**.                                                                         |
| Windows: the installer needs internet                                                                       | WebView2 is missing and the installer downloads Microsoft's bootstrapper. Connect to the internet, or install the WebView2 Runtime first.                                                                  |

## Signing in

| Symptom                                                                                                      | Cause and fix                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Account page says sign-in is not available                                                                   | This build has no Microsoft application ID. Only the project owner can register one ([AUTH-SETUP.md](AUTH-SETUP.md)); it is not a local problem.        |
| "The system credential store is unavailable, so you will need to sign in again after the launcher restarts." | No system credential store (Secret Service, Keychain, Credential Manager) was usable. Install or unlock one (GNOME Keyring, KWallet) and sign in again. |

## Installing and repairing

| Symptom / error code                                                | Cause and fix                                                                                                                                                      |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `NETWORK_ERROR` "A download source could not be reached"            | No connection to Mojang or Adoptium. Check the connection, proxy or firewall, then **Retry**; verified files are kept and not downloaded again.                    |
| `DISK_FULL` "There is not enough free disk space"                   | A first installation needs about 1.2 GB per version (less when versions share files). Free space on the drive that holds the launcher data folder, then **Retry**. |
| `PERMISSION_DENIED`                                                 | The launcher data folder is not writable (for example read-only media or changed permissions). Restore write access to the folder listed in **Settings**.          |
| Installation shows "Needs verification" after a crash or power loss | The launcher was stopped during a change. Choose **Repair**: it checks every file, downloads only damaged or missing ones and never touches worlds or options.     |
| A game file or Java was damaged                                     | **Repair** detects it by checksum and replaces exactly those files.                                                                                                |

## Playing

| Symptom                                                                                 | Cause and fix                                                                                                                                                                                                                     |
| --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Play is refused: sign-in required                                                       | See "Signing in".                                                                                                                                                                                                                 |
| `UNSUPPORTED_PATH_ENCODING` "A game path contains characters that Java cannot receive…" | The system locale cannot carry non-ASCII characters (for example a `POSIX` locale with a user name like `Jürgen`). Use a UTF-8 locale (`LANG=C.UTF-8` or your language's UTF-8 locale), or move the data folder to an ASCII path. |
| "The output log could not be written to disk"                                           | The log folder was full or not writable while the game ran. The game kept running; free space or fix permissions for the launcher data folder.                                                                                    |
| The launcher window disappeared during play                                             | Expected in the default mode: the window closes while the game runs and returns when it exits. Starting the launcher again brings the window back.                                                                                |

## Where things are

| Item                          | Location                                                                    |
| ----------------------------- | --------------------------------------------------------------------------- |
| Launcher data                 | See [INSTALL.md](INSTALL.md#your-data)                                      |
| Worlds and options            | `instances/<id>/game/` in the data folder (**Open game folder** in the app) |
| Launcher-captured game output | `logs/runs/<run>/` in the data folder (included in the diagnostic export)   |
