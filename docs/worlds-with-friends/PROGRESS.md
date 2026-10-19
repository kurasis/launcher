# Progress

States: `not_started`, `in_progress`, `blocked`, `done`. A phase whose code is finished but
whose live check waits for an outside input is `blocked`, with the input named. Evidence says
whether it is a unit test, a local run, CI or a live run with real accounts and networks.

| Phase                         | Status      | Evidence                                                                                        | Blocker / notes                                                                  |
| ----------------------------- | ----------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| W1 Foundation                 | done        | This directory and [D-068](../DECISIONS.md#d-068--worlds-with-friends); documents only, no code | —                                                                                |
| W2 Game and server together   | not_started |                                                                                                 |                                                                                  |
| W3 Server on the own computer | not_started |                                                                                                 |                                                                                  |
| W4 Tunnel                     | not_started |                                                                                                 | iroh version chosen by the spike                                                 |
| W5 worlds.lapi.sh             | not_started |                                                                                                 | Owner: DNS record and certificate for worlds.lapi.sh, `deploy/bootstrap.sh` once |
| W6 Relay                      | not_started |                                                                                                 | Owner: a VPS, DNS record relay.lapi.sh (not proxied), `RELAY_SSH_*` secrets      |
| W7 Play by invitation         | not_started |                                                                                                 | Live check with licensed accounts needs Minecraft service approval               |
| W8 Worlds tab                 | not_started |                                                                                                 |                                                                                  |
| W9 World safety               | not_started |                                                                                                 |                                                                                  |
| W10 Failures and networks     | not_started |                                                                                                 | Two test players on different home networks, CGNAT access                        |
| W11 Release                   | not_started |                                                                                                 | Windows and macOS machines; signing as for the launcher                          |
| W12 Worlds with mods          | not_started |                                                                                                 |                                                                                  |

## Outside inputs

| Input                                                                                                      | State                                                      | Needed for                                             |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------ |
| Minecraft service approval for the project's Microsoft client                                              | Pending (see [docs/spec/PROGRESS.md](../spec/PROGRESS.md)) | Verified licenses in release builds, live W7 and later |
| worlds.lapi.sh DNS, certificate, bootstrap                                                                 | Not started                                                | W5 deployment                                          |
| Relay VPS, relay.lapi.sh DNS, `RELAY_SSH_HOST`, `RELAY_SSH_KEY`, `RELAY_SSH_KNOWN_HOSTS`, `RELAY_SSH_USER` | Owner chose a separate VPS (2026-10-19); not provided yet  | W6                                                     |
| Test players and networks                                                                                  | Not arranged                                               | W10                                                    |

## W1 report

```text
Phase: W1 — Foundation
Status: done
Delivered: specification 1.0.2 verbatim in spec/, ADAPTATION.md (owner decisions of
  2026-10-19), INTEGRATION.md (adapter map against main at ac201e2), PLAN.md (W1–W12 with exit
  criteria), this file, D-068.
Checks: documentation only; prettier on the new files; no code changed.
Not claimed: any implementation, live provider behaviour, transport or game session.
Next: W2.
```
