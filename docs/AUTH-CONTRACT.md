# Authentication contract

Non-secret description of the authentication contract implemented in
`src-tauri/src/auth/` (contract revision `2026-10-candidate-1`, see
`src-tauri/src/auth/contract.rs`). Normative requirements: [spec/03](spec/03-AUTHENTICATION.md).

**Status: candidate, not yet accepted by the services.** No project-owned Microsoft client
ID exists yet, so no request in this contract has been sent to Microsoft, Xbox or Minecraft
services by this launcher. Everything below is verified only against deterministic local
fixtures. Gate G0 and phase P04's live check require the owner's client ID
([AUTH-SETUP.md](AUTH-SETUP.md)).

## Sources

- Prism Launcher at commit `60bcc2694d7433e2d2a657fbf62b0d022f9116e0` (files listed in the
  specification's source register), read for behaviour only; no code was copied.
- Microsoft identity platform authorization-code flow with PKCE (public client, no secret).

## Flow

| Step               | Request                                                                                                                                                                                                                                                  | Validation in the launcher                                                                                                                                                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1. Authorize       | Browser GET `https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize` with `client_id`, `response_type=code`, `redirect_uri`, `response_mode=query`, `scope`, `state`, `code_challenge`, `code_challenge_method=S256`, `prompt=select_account` | URL built with a URL encoder; opened only through the Rust allowlist (HTTPS, exact host and path)                                                                                                                                                                  |
| 2. Callback        | `GET http://localhost:<port>/?code=…&state=…` on the loopback listener                                                                                                                                                                                   | Path must be `/`; `state` compared in constant time; duplicate parameters, other paths and malformed requests are answered 400/404 without consuming the attempt; 8 KiB / 5 s per connection; 5 min attempt timeout; listener closed on success, cancel or timeout |
| 3. Token           | POST form to `…/consumers/oauth2/v2.0/token`: `grant_type=authorization_code`, `code`, `redirect_uri`, `code_verifier`, `scope`, `client_id` (no secret)                                                                                                 | `token_type` must be Bearer; `access_token` and `expires_in` required; a refresh token is required at sign-in. The code exchange is never retried automatically                                                                                                    |
| 4. Xbox user token | POST JSON `https://user.auth.xboxlive.com/user/authenticate`: `AuthMethod=RPS`, `SiteName=user.auth.xboxlive.com`, `RpsTicket=d=<MSA access token>`, `RelyingParty=http://auth.xboxlive.com`, `TokenType=JWT`                                            | `Token`, RFC 3339 `NotAfter`, `DisplayClaims.xui[].uhs` required                                                                                                                                                                                                   |
| 5. XSTS            | POST JSON `https://xsts.auth.xboxlive.com/xsts/authorize` with header `x-xbl-contract-version: 1`: `SandboxId=RETAIL`, `UserTokens=[user token]`, `RelyingParty=rp://api.minecraftservices.com/`, `TokenType=JWT`                                        | Same shape; the XSTS `uhs` must equal the user token's `uhs`; `xid` is kept as the authenticated XUID only when present and numeric                                                                                                                                |
| 6. Minecraft token | POST JSON `https://api.minecraftservices.com/launcher/login`: `xtoken="XBL3.0 x=<uhs>;<xsts token>"`, `platform="PC_LAUNCHER"`                                                                                                                           | `access_token` and `expires_in` required                                                                                                                                                                                                                           |
| 7. Access          | GET `https://api.minecraftservices.com/entitlements/license?requestId=<fresh UUID>` with `Authorization: Bearer`                                                                                                                                         | `items` array required (missing = protocol error); every item needs a string `name`                                                                                                                                                                                |
| 8. Profile         | GET `https://api.minecraftservices.com/minecraft/profile` with `Authorization: Bearer`                                                                                                                                                                   | `id` (32 hex) and `name` (1–16 of `[A-Za-z0-9_]`) required; requested only after play access                                                                                                                                                                       |

All credentialed requests use a dedicated HTTPS-only client with redirects disabled (an
unexpected 3xx is a protocol error), 15 s connect / 30 s total timeouts and a 1 MiB
response limit. Response bodies and token values are never logged or returned to the UI.

## Scopes

Requested: `XboxLive.SignIn XboxLive.offline_access` (as in the reviewed source).
Alternative to evaluate in G0 if the registration rejects it: `XboxLive.signin offline_access`.
No Microsoft Graph or mail scopes. The accepted spelling must be recorded here after the
first live sign-in.

## Redirect

Registered redirect: `http://localhost` (Mobile and desktop applications platform). The
launcher binds `127.0.0.1:<ephemeral port>` and, when possible, `[::1]` on the same port,
because `localhost` may resolve to either; it never binds `0.0.0.0`. The redirect sent is
`http://localhost:<port>/`. Microsoft's documentation states that the port is ignored for
loopback redirects of public clients; this must be confirmed with the real registration
(including whether the trailing `/` path matches the registered URI).

## Access decision

| Result                                                | Account state                                          | Error code                              |
| ----------------------------------------------------- | ------------------------------------------------------ | --------------------------------------- |
| `game_minecraft` present + valid profile              | `verified_access`                                      | —                                       |
| No `game_minecraft` (even with `product_minecraft`)   | `no_access`                                            | `NO_GAME_ACCESS`                        |
| Play access + profile 404                             | `profile_setup_required`                               | `PROFILE_SETUP_REQUIRED`                |
| XSTS `XErr` 2148916233 (no Xbox profile)              | `profile_setup_required`                               | `PROFILE_SETUP_REQUIRED`                |
| XSTS `XErr` 2148916227/229/234/235/236/237/238        | `restricted`                                           | `AUTH_ACCOUNT_RESTRICTED`               |
| Other XSTS `XErr`                                     | `restricted`                                           | `AUTH_FORBIDDEN`                        |
| 403 whose message contains `Invalid app registration` | `app_not_approved`                                     | `AUTH_APP_NOT_APPROVED`                 |
| Any other 403                                         | `restricted`                                           | `AUTH_FORBIDDEN` (never "no purchase")  |
| 401, `invalid_grant`, `interaction_required`          | `reauth_required` (after one refresh where applicable) | `AUTH_REAUTH_REQUIRED`                  |
| 429                                                   | `service_unavailable`                                  | `RATE_LIMITED`                          |
| 5xx, timeout, connection failure                      | `service_unavailable`                                  | `SERVICE_UNAVAILABLE` / `NETWORK_ERROR` |
| Malformed response, identity mismatch                 | `protocol_error`                                       | `AUTH_PROTOCOL_ERROR`                   |

`product_minecraft` (ownership) is recorded but never required: subscription play access is
accepted. The `Invalid app registration` text is the publicly reported Minecraft services
response for a client ID without Minecraft API approval; it has not yet been observed by this
project and must be confirmed in G0.

## Tokens and storage

- Microsoft refresh token: OS credential vault, service `app.launcher`,
  account `msa-refresh:<profile id>`. Persisted before an older token is discarded; when a
  refresh response does not rotate it, the existing entry stays.
- Microsoft access token, Xbox tokens and the Minecraft token: Rust memory only.
- `accounts.json`: profile ID, profile name and the vault key — no secrets.
- No usable vault: explicit session-only mode (memory only, nothing persisted, the UI says
  sign-in is needed after restart). No plaintext fallback.
- Launch authorization (`verify_for_launch`) reuses the Minecraft token only while it is
  valid with a 5-minute skew margin and always requests fresh entitlements and profile.

## Concurrency

One login or refresh transaction runs at a time. Every result is bound to an account
generation; cancel and sign-out advance it before touching the vault, and the
generation check plus every vault write share one commit lock, so a late callback or
refresh can never restore a signed-out account. Sign-in and sign-out are refused while a
launch or game owns the account. A failed vault deletion leaves a non-secret
`signOutPending` marker; restart retries deletion before anything is restored.

## XUID

The XSTS `xid` claim is the only XUID source. If the accepted contract does not return it,
the launch compiler must handle an empty `auth_xuid` as a compatibility candidate to test in
P09/P13 (doc 05). Nothing synthesizes an XUID.

## Owner-supplied values still missing

Client ID, confirmed redirect form, accepted scope spelling, Minecraft API approval, and
the first live positive sign-in (A03). Accounts for no-access and Game Pass tests (A04–A06).
