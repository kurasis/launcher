# Releasing a launcher version (D-063)

## Once

1. Make the update key on your own computer (keep the file and the password
   safe and offline: without them installed launchers cannot be updated):

   ```sh
   npx @tauri-apps/cli signer generate -w ~/.tauri/launcher-update.key
   ```

2. GitHub → Settings → Secrets and variables → Actions:
   - secret `TAURI_SIGNING_PRIVATE_KEY`: the content of `~/.tauri/launcher-update.key`;
   - secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password;
   - variable `UPDATE_PUBLIC_KEY`: the content of `~/.tauri/launcher-update.key.pub`.
3. Run the "Update service" workflow (deploys `lapi-upd` with the public key)
   and, once, `sudo bash ~/lapi-bootstrap/bootstrap.sh <deploy-user>` on the server.

Launchers built before the variable was set do not look for updates; the first
version built with it has to be installed by hand.

## Every version

1. Raise `version` in `src-tauri/Cargo.toml` and `package.json`; add
   `release-notes/<version>.json`; merge to `main`.
2. The "Package" workflow builds every platform and, with the key configured,
   the artifact `launcher-<version>-release` (a `.tar.gz`). Download it.
3. Upload and publish:

   ```sh
   scp launcher-<version>-release.tar.gz <deploy-user>@<server>:/home/www/lapi/upd/data/incoming/
   ssh <deploy-user>@<server>
   sudo -u lapi-upd /home/www/lapi/upd/current/lapi-upd publish <version> --rollout 10
   ```

   `publish` refuses a version that is not newer, a missing platform (add
   `--partial` to leave it on the previous version), a changed file or a
   signature that does not match or names another version.

4. Watch the crash statistics for a day (D-061), then
   `sudo -u lapi-upd /home/www/lapi/upd/current/lapi-upd rollout stable 100`.

## When something goes wrong

- Stop offering it: `lapi-upd pause stable` (`resume stable` to continue).
- Take it back: `lapi-upd revoke stable <version>`: the channel returns to the
  version before, and launchers that installed the bad one are offered that
  version (as a required update).
- `lapi-upd status` shows each channel; so does the "Update service" workflow
  with `task=reports`.
- `--mandatory` on `publish` makes an update one that cannot be skipped
  (players still choose when, never during a game).
