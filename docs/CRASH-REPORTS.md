# Crash reports: reading them and acting on them (D-061)

What players send from the launcher, where it is, and how it turns into fixes.

## Where the data is

| What       | Where                                               | Holds                                                                                                                                 |
| ---------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Reports    | `crash.lapi.sh` server, `/home/www/lapi/crash/data` | One gzip JSON per report: launcher, OS, game, loader, cause, signature, the scrubbed files the player kept, their note. Kept 90 days. |
| Statistics | same database, table `stats`                        | Per day: source, cause, signature, versions, OS, exit code, count. No install ID, no files. Kept 400 days.                            |
| Answers    | table `feedback`                                    | Per day: cause, signature, "helped" or not, count.                                                                                    |

Nothing is readable over HTTP. The repository and its workflow logs are
public, so reports are read only on the server; the "Crash service" workflow
with `task=reports` shows counts per cause and nothing else.

## Reading on the server

Log in as the deploy user (allowed without a password) or as an
administrator (asked for theirs), then:

```sh
c() { sudo -u lapi-crash /home/www/lapi/crash/current/lapi-crash "$@"; }

c overview --days 7            # counts per cause, statistics, answers
c groups --days 30             # biggest groups: cause + signature, reports, installs
c groups --source launcher     # the launcher's own failures
c list --kind unknown          # newest reports the analyser could not explain
c list --signature 'missing_dependency:geckolib>fabric-api'
c show <id>                    # metadata, note, list of files
c show <id> --file output.log  # one file, as sent
c stats --days 30              # statistics and answers by cause
c forget <install-id>          # delete one install's reports (a player asked)
```

Output is cleaned of control characters, so a crafted report cannot drive the
terminal. To work on a file locally:

```sh
ssh <deploy-user>@<server> "sudo -n -u lapi-crash /home/www/lapi/crash/current/lapi-crash show <id> --file output.log" > case.log
```

## Turning reports into fixes

Once a week, in this order:

1. **The launcher's own failures** (`groups --source launcher`). A `panic`
   names its source line in the signature (`panic:src/…:42`); `frontend` names
   the first line of the error; `unclean_exit` means killed, hung or power
   loss. Fix the biggest group first, and add a test that reproduces it.
2. **Unexplained game crashes** (`list --kind unknown`, `game_crash`,
   `mod_failed_to_load`). Each is a missing rule in `src-tauri/src/launch/crash.rs`:
   - take the scrubbed log (`show <id> --file output.log`);
   - cut it down to the lines that show the cause and add it to the analyser's
     tests as a fixture;
   - add the rule (cause, details, actions) and its texts in every language;
   - raise `ANALYZER_VERSION`, so later reports show whether the rule works.
3. **Explanations that did not help** (`stats`, `notHelped` by cause). A cause
   with many "no" answers needs a better text or a better action, or it groups
   different problems that need their own rules.
4. **Trends** (`overview` week to week, `groups` by `latest_version`). A group
   that grows after a release is a regression of that release; one that stops
   after a fix confirms it.

How to rank: by **installs** (different computers) before **reports**: one
player retrying is one problem, a hundred installs is a hundred players. A
group seen on one install only may also be made up: the client is open
source, so anyone can send reports, and only groups seen across many installs
are evidence.

## Privacy rules for whoever reads reports

- Reports stay on the server; copy a file out only to make a fixture, and
  check it once more for names, addresses and server names before committing.
- Never paste report contents into issues, pull requests or workflow logs:
  all of them are public.
- A player can ask for their reports to be deleted by their install ID
  (Settings → Privacy shows it): `c forget <install-id>`.
