# Installing the launcher

Sferum Launcher (its packages and folders are named `Launcher`) is an independent desktop launcher for Minecraft: Java Edition. You do not need
Rust, Node.js or Java: the launcher downloads a private Java runtime for each game version
when you install that version. You need a Microsoft account that owns Minecraft: Java
Edition to play.

> **Current status.** Packages are private qualification builds. They are not signed, so
> Windows and macOS warn before opening them. The project's Microsoft application is
> registered, but Minecraft services have not approved it yet, so signing in to play the
> full game is expected to fail until then (see [AUTH-SETUP.md](AUTH-SETUP.md)). Without
> an account the game starts as Minecraft's free demo.

Packages are produced by the `Package` workflow and kept as workflow artifacts. Each set
contains `SHA256SUMS` and `build-info.json` (version, commit, toolchain, signing status).
Check the hash before installing:

```sh
sha256sum -c SHA256SUMS          # Linux
shasum -a 256 -c SHA256SUMS      # macOS
Get-FileHash .\Launcher*.exe      # Windows PowerShell; compare with SHA256SUMS
```

## Windows 10 and 11 (x64 or ARM)

1. Run `Launcher_<version>_x64-setup.exe` (on Windows on ARM, the `arm64` one). It
   installs for the current user only (`%LOCALAPPDATA%\Launcher`) and needs no
   administrator rights.
2. The launcher uses Microsoft Edge WebView2. It is part of Windows 11 and current
   Windows 10. If it is missing, the installer downloads Microsoft's WebView2
   bootstrapper, which needs an internet connection.
3. Because the installer is unsigned, SmartScreen may show "Windows protected your PC".
   Choose **More info → Run anyway** only if the hash matches.

## macOS 11 or later (Apple silicon or Intel)

1. Open the DMG that matches your Mac (`aarch64` for Apple silicon, `x64` for Intel) and
   drag **Launcher** to Applications.
2. The app is not signed with a Developer ID or notarized, so Gatekeeper blocks the first
   start. Open it once with Control-click → **Open**, or allow it in **System Settings →
   Privacy & Security**.

## Linux (x64)

Qualification targets are Debian 12 and Ubuntu 24.04 (GTK 3, WebKitGTK 4.1).

- **.deb:** `sudo apt install ./launcher_<version>_amd64.deb`. Apt installs the
  runtime libraries it declares (`libwebkit2gtk-4.1-0`, `libgtk-3-0`).
- **AppImage:** `chmod +x Launcher*.AppImage` and run it. It bundles most libraries but
  still uses the system glibc (2.36 or newer), graphics drivers and desktop services. If
  FUSE is not available, run it with `APPIMAGE_EXTRACT_AND_RUN=1` or
  `--appimage-extract-and-run`.

Signing in stores credentials in the Secret Service (GNOME Keyring, KWallet). Without one,
the launcher keeps the sign-in for the current session only and says so.

## Your data

The launcher keeps its data, including game installations, worlds and settings, in:

| OS      | Location                                     |
| ------- | -------------------------------------------- |
| Windows | `%LOCALAPPDATA%\app.launcher`                |
| macOS   | `~/Library/Application Support/app.launcher` |
| Linux   | `~/.local/share/app.launcher`                |

Settings → General → _Launcher data location_ moves it to another folder. A
`portable.txt` file next to the launcher keeps the data next to the launcher instead,
so the whole folder can move to another computer (D-043).

## Updating

A launcher built with the update public key checks for new versions, downloads them and
installs them when it closes (D-063, [RELEASING.md](RELEASING.md)). Otherwise install
the newer package over the old one (run the new installer, `apt install` the new .deb,
replace the app or AppImage). Your data stays where it is either way.

## Uninstalling

Uninstalling removes the application and keeps your data and worlds:

- Windows: **Settings → Apps → Launcher → Uninstall**. The uninstaller offers a
  "Delete the application data" checkbox; it is off by default. Ticking it deletes the
  data folder above, including worlds.
- macOS: move the app to the Bin.
- Linux: `sudo apt remove launcher` (also `purge`) or delete the AppImage.

Use **Sign out** in the launcher before uninstalling to remove stored sign-in credentials
from the system credential store. Uninstalling does not end sessions on Microsoft's side.
To remove everything, delete the data folder yourself.
