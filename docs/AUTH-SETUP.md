# Microsoft sign-in setup (owner actions)

The launcher signs players in through the system browser with **this project's own**
Microsoft application, then proves current Minecraft: Java Edition access through Minecraft
services. This document lists what the project owner must set up and what must be verified
before the authentication phase (P04) can be completed. Normative requirements:
[spec/03-AUTHENTICATION.md](spec/03-AUTHENTICATION.md).

**Current status (2026-10-17):** the owner registered the application and supplied its
client ID as the repository variable `LAUNCHER_MSA_CLIENT_ID`, so CI packages are built
with it. Microsoft accepts the launcher's authorization request (registration, redirect
and scopes reach the sign-in form; see the owner record below). Minecraft service access
is requested but **not yet approved**, and no account has signed in yet (A03 open).

## Hard rules

- Never use the client ID of the official launcher or of another launcher (Prism, Modrinth,
  MultiMC, …), even for testing.
- The application is a **public client**: do not create or ship a client secret or certificate.
- Never send account passwords, refresh tokens or authorization codes to developers, chat
  tools, issues or this repository. Account tests are operated by the owner.
- Do not switch endpoints or identities to work around a refusal.

## 1. Register a Microsoft application

In the Microsoft Entra admin center (or Azure portal) → **App registrations** → **New registration**:

| Setting                     | Value to use                                                                                                | Verify in G0                                                                                                                        |
| --------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Name                        | Your launcher's product name (provisional: Launcher)                                                        | —                                                                                                                                   |
| Supported account types     | An option that includes **personal Microsoft accounts** (the flow uses the `consumers` authority)           | Record the chosen option                                                                                                            |
| Platform                    | **Mobile and desktop applications** (public client / native)                                                | —                                                                                                                                   |
| Redirect URI                | `http://localhost` (loopback, system browser)                                                               | Port handling and the exact host must be confirmed with a real callback; do not assume `127.0.0.1` or IPv6 behave the same (doc 03) |
| Client secret / certificate | None                                                                                                        | —                                                                                                                                   |
| "Allow public client flows" | Not required for the browser authorization-code + PKCE flow; needed only if device-code login is ever added | Record the actual setting                                                                                                           |
| API permissions             | No Microsoft Graph permissions are needed                                                                   | —                                                                                                                                   |

After registration copy the **Application (client) ID** (a GUID). It is not a secret.

If saving **Personal accounts only** fails with "Property api.requestedAccessTokenVersion
is invalid", open **Manifest**, set `"requestedAccessTokenVersion": 2` in the `api` block
(`"accessTokenAcceptedVersion": 2` in the older AAD Graph manifest), save, and change the
supported account types again. Personal Microsoft accounts only accept version 2 tokens.

## 2. Request Minecraft service access

New client IDs are rejected by `api.minecraftservices.com` (typically HTTP `403`) until the
application has been approved for Minecraft services. The official reference is the
Minecraft Help article <https://help.minecraft.net/hc/en-us/articles/16254801392141>; the
CmlLib documentation describes the same allowlisting requirement
(<https://cmllib.github.io/CmlLib.Core-wiki/en/auth.microsoft/xboxauthnet.game.msal/clientid/>).

The specification does not promise an approval route, deadline or automatic acceptance.
The owner submits the request through the official channel described there and records the
outcome below. While approval is pending, a refusal is reported as `AUTH_FORBIDDEN` (or
`AUTH_APP_NOT_APPROVED` only when a recognized provider diagnostic says so) and the rest of
the launcher keeps working.

## 3. Build the launcher with the client ID

The ID is read when the Rust core is compiled:

```sh
LAUNCHER_MSA_CLIENT_ID=<application-client-id-guid> npm run tauri dev
```

The **Account** screen then offers **Sign in**; without a valid ID it explains that
sign-in is not set up in this build. The core reports the ID as `not_configured`,
`invalid` or `configured_unverified`, never "verified": verification requires a real
sign-in (still outstanding, A03). (The About screen no longer shows this status, D-040.)

Once the owner approves publishing the ID, it can be committed as non-secret build
configuration (for example a CI variable or a checked-in config file); record that change
in [DECISIONS.md](DECISIONS.md).

## 4. Values to verify in gate G0

These are taken from the specification's source review (Prism Launcher at commit
`60bcc2694d7433e2d2a657fbf62b0d022f9116e0`) and Microsoft documentation. They have **not**
been called with credentials yet.

| Item                   | Candidate                                                                                                                                                                    | Status                                                                         |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Authorization endpoint | `https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize`                                                                                                          | Accepted: the launcher request reaches the Microsoft sign-in form (2026-10-17) |
| Token endpoint         | `https://login.microsoftonline.com/consumers/oauth2/v2.0/token`                                                                                                              | Unverified                                                                     |
| Scopes                 | `XboxLive.SignIn XboxLive.offline_access` (Prism) or `XboxLive.signin offline_access` (Microsoft refresh-token convention) — choose the minimal set the registration accepts | Both sets reach the sign-in form; confirm with a real sign-in                  |
| PKCE                   | S256, fresh verifier and `state` per attempt                                                                                                                                 | Implemented (P04)                                                              |
| Xbox user token        | `POST https://user.auth.xboxlive.com/user/authenticate` (`RpsTicket=d=<token>`, relying party `http://auth.xboxlive.com`)                                                    | Unverified                                                                     |
| XSTS                   | `POST https://xsts.auth.xboxlive.com/xsts/authorize` (`RETAIL`, relying party `rp://api.minecraftservices.com/`)                                                             | Unverified                                                                     |
| Minecraft token        | `POST https://api.minecraftservices.com/launcher/login` (`platform: PC_LAUNCHER`); the older `/authentication/login_with_xbox` is not the default                            | Unverified                                                                     |
| Access check           | `GET https://api.minecraftservices.com/entitlements/license?requestId=<uuid>`                                                                                                | Unverified                                                                     |
| Profile                | `GET https://api.minecraftservices.com/minecraft/profile`                                                                                                                    | Unverified                                                                     |

The implemented contract and its validation rules are described in
[AUTH-CONTRACT.md](AUTH-CONTRACT.md).

## 5. Owner record (fill in; no secrets)

| Field                                                                             | Value                                                                                 |
| --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Client ID (non-secret)                                                            | `96c77e97-0788-4e3a-8eaf-91c8b143ba86` (repository variable `LAUNCHER_MSA_CLIENT_ID`) |
| Supported account types                                                           | Personal accounts only (`requestedAccessTokenVersion` 2)                              |
| Registered redirect URI(s)                                                        | `http://localhost` (Mobile and desktop applications)                                  |
| Minecraft service access request date / outcome                                   | Requested by 2026-10-17 / pending                                                     |
| First positive own-client sign-in (date, OS, commit)                              | — (required for P04 / A03)                                                            |
| Accounts available for owner-operated tests: owned / no access / active Game Pass | — (A03–A06)                                                                           |
