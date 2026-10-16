# 03 — Authentication and legitimate game access

## Mandatory early gate

Before assuming the product can ship, register this project's own Microsoft public-client application, verify its desktop redirect configuration and establish the currently required Minecraft service access. CmlLib's maintained documentation explicitly describes client-ID allowlisting and `403` failures. The linked Minecraft help page was reachable during research but its procedural article body was not retrievable. Therefore this brief does not promise an approval route, approval deadline or automatic acceptance.

Do not borrow the official launcher, Prism, Modrinth or another application's client ID. A public desktop client must not contain a client secret. If access is refused, report `AUTH_APP_NOT_APPROVED` only when a recognized provider diagnostic establishes that cause; otherwise use `AUTH_FORBIDDEN` or the precise registration error and continue unrelated implementation work. Do not switch endpoints or identities to circumvent the refusal.

## Current endpoint evidence versus older tutorials

Research inspected Prism source at commit `60bcc2694d7433e2d2a657fbf62b0d022f9116e0` (see source register). It currently uses the following service sequence. These Minecraft endpoints are implementation evidence, not a formally guaranteed public SDK contract, and were not called with credentials during research.

| Step | Method and endpoint | Important fields |
| --- | --- | --- |
| Microsoft authorization | Browser GET `https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize` | Own client ID, code response, redirect URI, scopes, state, S256 PKCE challenge |
| Microsoft token / refresh | POST `https://login.microsoftonline.com/consumers/oauth2/v2.0/token` | Form-encoded code + verifier or refresh grant; public client, no secret |
| Xbox user token | POST `https://user.auth.xboxlive.com/user/authenticate` | RPS properties, `RpsTicket` built from Microsoft token; JWT type |
| Xbox authorization | POST `https://xsts.auth.xboxlive.com/xsts/authorize` | RETAIL sandbox, user token, relying party `rp://api.minecraftservices.com/` |
| Minecraft token | POST `https://api.minecraftservices.com/launcher/login` | `xtoken` containing `XBL3.0 x=<uhs>;<xsts-token>` and `platform: PC_LAUNCHER` |
| Current access | GET `https://api.minecraftservices.com/entitlements/license?requestId=<random-uuid>` | Minecraft Bearer token; parse service items |
| Java profile | GET `https://api.minecraftservices.com/minecraft/profile` | Minecraft Bearer token; profile UUID and player name |

Older examples use `/authentication/login_with_xbox` and `/entitlements/mcstore`. Do not make these old paths the default simply because an old tutorial uses them. Put the Minecraft exchange and entitlement request in one versioned adapter. Gate G0 must confirm which contract is actually accepted for this project's registration. A deliberate compatibility change requires documentation and positive/negative account tests; no silent auth fallback chain.

Sources: [Microsoft authorization-code protocol](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow), [Prism auth flow](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/AuthFlow.cpp), [current exchange](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/LauncherLoginStep.cpp), [current access request](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/EntitlementsStep.cpp).

## System-browser OAuth implementation

1. Bind the loopback callback before opening the browser. Listen only on loopback, never `0.0.0.0`. Use the exact URI scheme/host/path matching the registered native-client redirect and Microsoft's documented loopback port handling.
2. Generate cryptographically random state and a fresh PKCE verifier per attempt. Use S256. Keep both in Rust memory and associate them with one expiring attempt ID.
3. Open the authorization URL through a constrained external-browser operation. Do not render a Microsoft password form inside React and never collect credentials.
4. Require the exact callback path, matching state and a pending attempt. Reject replayed callbacks; stop listening on success, cancel or timeout (default five minutes).
5. Exchange the one-use code in Rust. Do not leak the callback query into HTTP logs, browser UI, analytics or IPC events. Return a minimal local success page with no external resources.
6. Use minimal Xbox sign-in and refresh scopes supported by the registration. The inspected Prism profile uses `XboxLive.SignIn XboxLive.offline_access`; Microsoft's v2 documentation uses `offline_access` for refresh consent generally. Verify the exact Xbox scope spelling/audience in G0 and record it; do not request Graph/mail scopes or combine recipes blindly.
7. Validate required response fields, expiry and account consistency at each exchange. The Xbox user hash from XSTS must correspond to the user-token identity. Respect service errors rather than continuing with partially parsed data.

Use an OAuth library for PKCE/token handling where possible. If manual service requests remain necessary, keep protocol parsing small and tested. Device-code login is deferred unless the approved registration cannot support the chosen browser flow; if added, implement server-specified polling intervals, slow-down, expiration and cancellation rather than a tight polling loop.

## Access decision

The MVP grants `verified_access` only after a fresh successful authenticated entitlement response that explicitly permits Java play and a valid Java profile for the same authenticated session. In the inspected current Prism parser, `game_minecraft` means play access and `product_minecraft` means ownership. Do not require both: subscriptions must not be rejected merely because they are not permanent purchases. G0/P04 require a real positive access/profile result with this project’s client ID and deterministic decision tests. Actual owned, no-access and active Game Pass evidence is mandatory for G5/P13 (A03–A06), and may be collected earlier. Missing subscription evidence blocks final qualification, not positive-path integration after P04.

Do not treat a public name/UUID lookup, Microsoft login alone, an arbitrary nonempty entitlement array, or an old cached profile as proof of access. Strictly validate the expected JSON shape; a missing `items` array is a protocol error, not a valid empty license list. Do not interpret decoded JWT fields as cryptographically verified evidence. For this online-only MVP, the authoritative result is the validated HTTPS service response; no unsigned local entitlement token is accepted offline.

| Entitlement / profile result | User state | Launch |
| --- | --- | --- |
| Play access present + valid Java profile | Verified access | Allow |
| Play access absent in a valid successful response | No current Java access | Block; offer official purchase/account link |
| Play access present + profile `404` | Profile setup needed | Block; direct user to official profile setup |
| `401` | Expired/revoked session | Refresh once, then require sign-in |
| `403` | App/account/service restriction | Block with specific diagnostic; do not assume no purchase |
| `429`, timeout or server error | Verification temporarily unavailable | Block new launch; allow retry |
| Malformed response or unsupported contract | Service protocol error | Block; preserve a sanitized diagnostic |

Source: [Prism entitlement parser](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/Parsers.cpp). The design intentionally distinguishes current access from historical ownership and profile setup.

## Timing and refresh

- Before each launch, ensure token validity with a small clock-skew margin, then request fresh access/profile data. An access decision belongs to that launch attempt; it is not a reusable permanent license.
- If installation takes long, repeat authorization preflight immediately before process creation.
- Serialize refreshes to avoid rotating-token races. Persist a new refresh token before discarding the prior valid vault entry; if the response does not rotate it, retain the existing one.
- Clear login on confirmed revocation or invalid grant, not on network outages. A retryable failure must not erase useful account metadata.
- A running game is not forcibly terminated by a transient auth outage or a periodic license poll. The MVP does not poll entitlement during gameplay.
- Local sign-out deletes saved secrets and clears the active identity. It does not claim to sign out of all Microsoft browser sessions or revoke every remote token.

## Secret storage

Use the OS credential vault: Windows credential storage, macOS Keychain, Linux Secret Service or a supported desktop equivalent via a reviewed Rust adapter. Store refresh credentials under an application-specific service name and an account-specific identifier. Keep short-lived tokens only as long as needed in Rust.

When a Linux session has no usable/unlocked vault, offer an explicit session-only login: credentials remain in memory, sign-in is required after restart. No plaintext fallback in JSON, localStorage, browser IndexedDB, `.env` or a hard-coded encryption key. Stronghold may be adopted later only with a real key-unlocking design; naming an encrypted file is not sufficient key management.

Expose only display name, UUID, account state and sanitized errors to React. Do not derive secret-bearing Debug/Serialize implementations for token structures. Redact Bearer values, Microsoft/Xbox/Minecraft tokens, codes, verifier, callback URLs and identity-token strings before any log sink.

The Java client requires a Minecraft access token for launch. Keep it out of persisted command previews and diagnostics. MVP argfiles contain non-secret JVM/classpath arguments only; supply the token directly in the process argument vector as specified in document 05. Document that local same-user process inspection remains outside the launcher's protection boundary. Do not promise that any desktop launcher can protect tokens from a fully compromised user session.

## Error handling

Handle denied consent, user cancellation, callback timeout, invalid state, refresh failure, missing Xbox profile, family/age restrictions, regional/service restrictions and unapproved client ID. Display the upstream non-secret error code where useful, but do not echo full raw response bodies. Link users to official account/support pages; do not attempt to bypass restrictions.

Registration references: [CmlLib Client ID guide](https://cmllib.github.io/CmlLib.Core-wiki/en/auth.microsoft/xboxauthnet.game.msal/clientid/), [Minecraft API review page](https://help.minecraft.net/hc/en-us/articles/16254801392141).

## OAuth callback and account concurrency details

The system browser's close event is not reliably observable. Provide Cancel and an expiring attempt; do not wait for a browser process to exit. Reject unrelated callback paths, wrong state and duplicate parameters without consuming the valid pending attempt. Bound request size and connection lifetime. A valid one-use callback completes or fails only its matching attempt.

Document the exact registered native redirect, including host/path and verified port behavior. Microsoft's localhost exception must not be assumed to apply identically to an arbitrary IP literal. If using `127.0.0.1`, verify the registration/manifest configuration and actual callback; if using `localhost`, ensure its loopback address resolution reaches the listener. Do not advertise IPv6 literal redirects as supported without provider evidence. See the [Microsoft redirect guidance](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url).

Allow one login/refresh transaction at a time. Bind async callbacks and refresh writes to an account-generation counter; cancel/sign-out invalidates it so a late result cannot restore a signed-out account. MVP rejects account replacement/sign-out while launch or gameplay owns the account. Keep the run's redaction secrets until both output streams have finished draining.

On sign-out, clear in-memory launch authorization immediately. If vault deletion fails, return `SECRET_STORE_DELETE_FAILED`, retain a non-secret sign-out-pending marker, disable automatic credential restoration and offer retry. Do not report successful removal while saved credentials remain. On restart, process this marker before loading credentials.

## Request construction contract

Implement structured JSON serialization, not string substitution of tokens. The pinned Xbox source uses `AuthMethod=RPS`, `SiteName=user.auth.xboxlive.com`, `RpsTicket=d=<Microsoft access token>`, relying party `http://auth.xboxlive.com`, and token type `JWT` for the user exchange. This relying-party value is an identifier inside JSON, not permission to send credentials over HTTP. The XSTS request contains `SandboxId=RETAIL`, `UserTokens=[<user token>]`, the Minecraft relying party from the table and token type `JWT`. Validate the chosen scope/audience and these details with the project's registration before freezing the adapter.

All credential-bearing requests use HTTPS with redirects disabled; unexpected redirects become protocol errors. Do not automatically replay a one-use authorization-code exchange after an ambiguous transport failure. Restart sign-in when its outcome cannot be recovered. Honor retry limits for safe requests and surface invalid grants distinctly. Upstream debug logging of raw auth payloads must never be copied into this launcher.

## Sign-out transaction entry

Sign-out during an ordinary background refresh cancels/joins that work and advances the account generation before credential deletion. During explicit interactive login, cancel the login attempt first. A committed launch or running game still blocks account replacement/sign-out until its final capture cleanup. The generation check and vault write must share the account transaction ordering so an old refresh cannot repopulate a deleted credential.
