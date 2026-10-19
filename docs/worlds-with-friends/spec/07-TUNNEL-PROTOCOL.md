# 07 — Transport, admission and forwarding

## Selected design and wire version

Use a pinned, tested transport adapter; iroh is the candidate. P04 proves stream semantics, privacy controls and the exact relay compatibility before freezing versions. Application wire protocol is `wwf-tunnel/1`; the revised application service allowlist contains only `gameplay`. If any old prototype implements a broader v1 service set, it must fail capability negotiation against this revision rather than silently remain compatible.

Each actual gameplay TCP connection maps to one authorized bidirectional stream. The guest binds a loopback listener before launching Java but does not consume a gameplay ticket/slot while Java is loading. When the local client opens a gameplay connection, the guest rechecks room/session/generation, obtains a fresh ticket and begins admission. A slow client launch therefore cannot use an expired ticket minted before launch.

## Status is not a privileged raw tunnel

Room status comes from the authorized Room API status DTO: observedAt, room/session state, player count, capacity, supported game protocol and safe display metadata. It has no arbitrary destination and no executable content. There is no `status` transport service that forwards arbitrary bytes to the server without player admission. The host rejects every non-gameplay service, even if an old or forged ticket names it.

If the supported client performs a native server-list ping against its local listener, a version-qualified local adapter handles only the bounded status handshake/request/ping and synthesizes a response from a membership-authorized status snapshot no older than 30 seconds. It closes that connection after completion and never forwards it to the gameplay destination. Initial handshake cap 8 KiB, deadline 5 seconds; reject malformed/unsupported states. A login request follows full gameplay admission. This parser is a narrow local compatibility adapter, not a generic remote proxy. A malicious guest cannot obtain weaker host permissions by labelling gameplay as status.

## Admission protocol

The guest obtains host endpoint identity through the authenticated Room API, not an invitation. After transport peer verification, send a 4-byte unsigned big-endian frame length followed by UTF-8 JSON, maximum 16 KiB. Fields: protocolVersion, requestId, ticket, sessionId, expectedHostEpoch and service=`gameplay`. Unknown critical fields, duplicate keys, unsupported versions, invalid lengths and wrong services are rejected. Host returns one bounded success/error frame. The generic bridge forwards raw ordered binary bytes only after admission succeeds; it does not rewrite game packets.

Admission framing deadline is 10 seconds from stream acceptance. Permit at most four unadmitted streams per registered endpoint context and 64 per host in total; also impose a global pre-auth transport limit, because an attacker can generate unlimited unregistered keys. Backend/account/device caps supplement these limits.

## Ticket and consumption

Use a standard signed token with an algorithm allowlist and managed signing keys. Claims bind issuer, host-agent audience, authorityEpoch, ticketId, iat/nbf/exp, accountId, deviceId, endpointBindingId, endpointId, expected profile reference, roomId, sessionId, hostDeviceId, hostEndpointId, hostEpoch, placementRevision, packGeneration, accessRevision, routePolicyRevision and service. TTL is 60 seconds; no refresh credential or local path is included.

The host allocates connectionId; the guest cannot choose a known connection ID to replay an idempotent consume. Host verifies its local session and the actual remote cryptographic endpoint, then consumes the ticket through a device-authenticated backend call. Consumption atomically checks current room membership, device/endpoint validity, world placement, access revision, host lease, generation and available slot. A retry for the same host-generated connection returns the original decision and expiry; another stream cannot reuse it. Consumed decisions never outlive current authorization.

Backend admission lease: 30 seconds, refreshed every 10 seconds; use the conservative deadline algorithm in [03](03-OAUTH-AND-IDENTITY.md). Identity status starts pending. The host opens the fixed local game connection, registers its trusted socket correlation with the game adapter, and requires `authorizeBeforePlay` before world access or gameplay. Pre-play binding timeout is 120 seconds; failure closes both sockets, enters the release barrier and marks identity failure. This is distinct from the 10-second tunnel framing deadline.

## Slots, replacement and revocation

One pending/active gameplay slot per account per room, including the host's own player. Status consumes no slot and has its own rate limit. A duplicate admission returns ALREADY_CONNECTED unless the current account explicitly authorizes replacement with an expected connection sequence. Replacement moves the old binding into closing and requires a trusted game-adapter release acknowledgment before admitting the next. Lease expiry alone is not proof of game-side teardown. Never overlap two accepted player identities under one slot; use the release barrier below.

Host renewals may batch connections. Backend rechecks membership, bans, devices, host authority, placement and pack generation. Push revocation closes promptly; lease expiry is the bounded fallback. Close the transport stream, corresponding local TCP socket and any verified game binding. A host-local player follows the same policy even if no network relay is needed.

## Privacy contexts

Endpoint bindings are scoped to room/session/purpose and route policy, under the stable device identity. Do not share a globally discoverable transport endpoint between an automatic room and a relay-only room. Configure relay-only before initializing network discovery: disable direct transports, probes and direct-candidate publication/distribution. P04 proves that the selected library can isolate these contexts; otherwise concurrent mixed-policy contexts must be disabled explicitly.

A stricter policy change requires a fresh endpoint context and coordinated reconnect; an already-public address cannot be made historically secret by changing a UI switch. Do not promise that participants who learned an IP earlier can forget it. An ordinary automatic-route migration may preserve a stream only if transport tests prove that behavior. Display observed routes, not guessed route labels.

## Bounds and failure

Application forwarding buffers are capped at 256 KiB per direction per stream and 64 MiB total per desktop agent. Include transport internals in total memory measurements. Propagate backpressure, half-close, cancellation and errors. Local listeners bind loopback; same-user/root compromise is outside this boundary, so pre-play player binding still matters.

On transport failure close the corresponding local TCP connection. Never replay captured game bytes into a new stream. A fresh Java join needs a fresh ticket; generic reconnection is not restoration of game state. Additional UDP/voice services require separate authorization and transport qualification. No guest can supply a destination address, management port, filesystem path or shell command.

## Game-side release barrier

Admission states are pending, active, closing and released. Guest DELETE/replacement, transport failure, identity timeout and revocation request closure; they do not immediately free a slot. Close both byte paths, invalidate the pending/pre-play authorization and ask the trusted adapter to detach the exact connectionId. The adapter acknowledges only after that connection can no longer reach gameplay and its player entity/pending login is removed, with any required player save completed under the qualified runtime contract. For a never-opened server connection, the host can prove that no adapter binding exists instead.

While closing, retain a per-account/profile exclusion barrier for that session independently of the expired admission lease. A late release acknowledgment must name sessionId, connectionId and binding sequence and cannot free its replacement. If the adapter cannot prove teardown, reject rejoin with CONNECTION_CLOSING and show recovery guidance; confirmed server exit clears session-local barriers. Do not turn a cleanup timeout into permission for a second player binding. Expired authority grants no right to continue gameplay while cleanup is pending.

## Local handshake ownership and bounds

One local connection state machine owns classification and forwarding. It reads the minimum version-qualified initial handshake using incremental bounded parsing, accepting fragmentation and coalesced packets. Preserve every consumed gameplay byte exactly once and in order; after full admission, send that prefix before bytes still waiting in the socket. Do not discard, reinterpret as JSON or duplicate the initial handshake when handing off to the raw bridge. If a read contains later login bytes, retain them within the same 8 KiB pre-admission buffer or apply backpressure. No unbounded pre-auth queue is allowed. Status responses never consume a gameplay ticket.

Bind each loopback listener to accountId, local client runId, roomId, expected session and pack generation. A generation/session change rejects that attempt with rejoin guidance instead of silently retargeting a running client to another world. Core limits local pending sockets to eight per listener and sixteen per agent, with the existing five-second classification deadline; enforce limits before allocating per-connection tasks or performing API work. Close the listener and pending sockets when its client run ends or account authority changes. An authenticated adapter registration must be ready before the game-side pre-play hook can run; either reserve correlation before connect or gate the accepted server socket until the registration is acknowledged.
