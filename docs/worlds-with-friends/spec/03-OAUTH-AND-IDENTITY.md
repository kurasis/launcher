# 03 — Own OAuth accounts, devices and game identity

## Provider integration contract

The product already owns its accounts. Reuse its OAuth issuer and room authorization model. P01 records the real issuer, public desktop client ID, redirect strategy, authorization/token endpoints, resource audience, scopes, token validation method, account lookup, refresh behavior, logout and device revocation facilities. Placeholder domains are never usable production configuration.

Use a system browser and Authorization Code with PKCE S256 for the desktop public client. Bind each attempt to unpredictable state, expected issuer and redirect endpoint. A loopback callback must bind only loopback, accept the expected path once, expire promptly and reject unrelated callbacks. Do not bundle a confidential client secret. For provider-supported OIDC, also validate the ID token and nonce; an ID token is not a Room API access token. See S01–S03 in [sources](SOURCES.md).

OAuth alone does not define a trustworthy human identity claim. If the provider supplies no OIDC identity, resolve the stable account subject through its documented validated token/account endpoint. Opaque tokens require the supported validation path; JWT parsing without signature/audience/issuer/expiry checks is not authentication. P02 must demonstrate the real provider contract.

## Identity layers

| Identity | Stable authority | Use |
| --- | --- | --- |
| `accountId` | Validated issuer + provider subject mapped to an internal UUID | Room membership and ownership |
| `deviceId` | Backend record bound to an account and stable control-key proof | Device trust and revocation |
| `endpointId` | Scoped transport key registered under a device | Cryptographic peer verification for one room/session context |
| `gameProfileId` | Existing project game identity adapter | Player identity in a game session |
| `roomId`, `sessionId`, `hostEpoch`, `authorityEpoch` | Room backend and non-rollback recovery authority | Scope of gameplay authorization |

Email addresses, nicknames and display names are not durable keys. Never derive room ownership from a game nickname or allow the client to assign its own trusted gameProfileId.

## Game identity integration is a real prerequisite

Implement `resolveProfile(accountId)`, `prepareClientSession(accountId, deviceId, sessionId)`, `registerPendingConnection(connectionId, localSocketIdentity, expectedProfile, admissionLease)`, `authorizeBeforePlay(connectionId, observedVerifiedProfile)` and `disconnectBinding(sessionId, connectionId)` against the project's trusted game identity integration. Keep credential handling inside the agent/adapter and redact it. The verification hook MUST run before the player reaches gameplay or receives private world data. Merely checking a display field after join is insufficient.

All remote clients appear local at a TCP bridge. Correlate each accepted server connection with an unambiguous trusted socket/connection identifier and its pending admission; IP address alone cannot identify a player. Only an authenticated local adapter channel may register pending bindings. Reject a missing, mismatched, duplicated or late binding. The 120-second pre-play binding deadline begins after tunnel admission; expired/unbound connections lose gameplay authority and enter the release barrier before their reserved slot can be reused. A qualified adapter may use a smaller documented deadline.

A raw byte-forwarding tunnel proves which agent connected; it does not by itself prove which player identity the Java server accepted. Therefore P01 identifies the existing trusted verification point, and P07 tests an identity substitution attempt against a real server. If an unmodified runtime lacks the required integration, mark that tuple unsupported or implement and qualify the minimum bridge-equipped tuple in core P06/P07; do not claim a pure vanilla result. A loader/bootstrap strictly required by that bridge is a core dependency, not something deferred until P13. General modpack support still starts in P13. Runtime flavor (`unmodified` or `bridge_required`) is recorded separately from loader family. The first runnable tuple depends on this gate.

Room roles do not automatically grant in-game operator permissions. Core exposes no general in-game privilege delegation. Later delegation requires a verified player binding and a separate permission contract.

## Credential ownership

The agent owns refresh rotation and serializes refresh per account. Store refresh credentials and private keys in the OS vault; if unavailable, show a blocked secure-storage state or use an explicitly designed encrypted alternative, never silent plaintext. Access tokens remain short-lived in memory where possible. No tokens in frontend storage, logs, URLs, diagnostics, exported rooms or pack files.

Use an account generation counter to prevent a late refresh from resurrecting a signed-out account. Persist pending credential deletion until vault cleanup succeeds. Account switch while hosting requires a deliberate stop or owner-authorized service identity flow; do not silently transfer running authority to the next signed-in user.

## Device enrollment

Generate a stable device control key locally. After validated account login, register it using a single-use backend challenge and proof of possession. The challenge binds expected account, purpose, key, expiry and nonce. The server binds one active account to the device record and records enrollment/revocation timestamps. Device control-key rotation requires re-enrollment and invalidates its endpoint bindings/grants; do not silently attach it to another account.

Register transport endpoint bindings separately. Each contains endpointId/public key, deviceId, roomId, sessionId or approved pre-session purpose, role, routePolicy, routePolicyRevision, protocol version, expiry and binding revision. Prove possession of both device authority and the new endpoint key. One device may have multiple bounded endpoint contexts. Expiring an endpoint does not revoke the entire device, while revoking a device invalidates all its endpoints.

Remote headless enrollment is deferred to P15. Desktop core does not ship a shared secret that authorizes all installed copies.

Device-scoped control calls (host heartbeat, command acknowledgment, admission consumption/renewal and relay enrollment) MUST demonstrate possession of the registered device's control key as well as valid account/service authorization. A deviceId in a JSON body is not proof. Use the existing backend's supported sender-constrained credentials, such as correctly validated DPoP or mutually authenticated TLS; do not invent a signature format. If control and transport keys differ, bind both during enrollment with independent proofs. P01 freezes the mechanism and P02 tests its exact guarantees: DPoP validation includes method/target/token binding and replay/nonce controls; mutually authenticated TLS validates the enrolled certificate/key and channel, with application idempotency and authorization for requests. Neither mechanism alone authorizes a room operation or signs arbitrary body fields. Preserve HTTPS request integrity and explicit business validation. See S10 in [sources](SOURCES.md).

## Product authorization leases

Defaults: host lease 30 seconds, renewed every 10 seconds; participant admission lease 30 seconds, refreshed every 10 seconds; initial stream ticket valid for 60 seconds and single use. The ticket is permission to attempt admission, not 60 seconds of unrenewed gameplay. See [07](07-TUNNEL-PROTOCOL.md) for consumption and channel binding.

Push revocations should close affected streams within 5 seconds under a healthy control connection; lease expiry bounds stale access to 30 seconds after the last successful authorization refresh, plus a documented 2-second scheduling budget on a responsive host. During a control outage, reject new admissions and close existing guest tunnels when their leases expire. Keep world data intact and request a graceful server stop after host-lease expiry. Do not advertise offline collaborative access in core.

Use backend time for authority and conservative request-start deadlines. Each renewal response echoes a nonce/sequence and returns serverIssuedAt, expiresAt and authorityEpoch. Let remainingAtIssue = expiresAt - serverIssuedAt. Set the local deadline no later than local monotonic requestStart + remainingAtIssue; never responseReceiveTime + TTL. This deliberately subtracts all request latency. Reject responses arriving after that deadline, stale sequences, a previous authority/session epoch or a lower access revision. Idempotent retries return the original expiry, never mint extra lifetime.

At most 5 seconds of wall-clock skew may be used for initial token checks; it never extends an authorization lease. Revalidate after sleep/resume before reopening tunnels. On frozen/compromised hosts no wall-clock enforcement bound is claimed; user-facing timeouts and the 2-second scheduling allowance apply to responsive cooperating processes. Backend, host and relay each enforce their own expiry boundary.

## Connection teardown capability

GameIdentityAdapter also provides an idempotent detach request and `bindingReleased(sessionId, connectionId, bindingSequence)` observation through its trusted channel. A TCP close event is not this observation. Qualify pending-login cancellation and active-player removal/save ordering before advertising safe automatic replacement. The release barrier in [07](07-TUNNEL-PROTOCOL.md) is required even after an admission lease expires.

## Lease clock and scheduling ownership

Lease watchdogs and refresh requests must remain responsive independently of filesystem hashing, compression, Java pipe processing and ordinary UI/API queues. Use bounded workers for blocking work and reserved control-request capacity. Baseline control requests have a five-second total attempt deadline, further capped by the remaining lease lifetime; retries use bounded jitter without resetting either deadline. The 10/20-second renewal schedules are targets, not permission to queue indefinitely. Watchdogs close access locally even if a renewal task is stuck. No backend write transaction waits for a client/network response.

Local monotonic timestamps are valid only within their recorded agent/clock epoch. Do not deserialize a deadline across process restart or reboot as fresh authority. Renew/reconcile against the backend before permitting access; an already expired host session stays terminal. Suspend/resume or detected clock discontinuity closes admission gates pending fresh validation, even on platforms whose monotonic clock excludes sleep. Backend nodes use one authoritative lease-time contract, reject unsafe clock health and atomically reject renewals after expiry; a wall-clock rollback must not prolong accepted authority. P02 chooses and tests the actual database/time service behavior rather than assuming all clocks agree.
