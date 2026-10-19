# 13 — Security and privacy requirements

## Threat model

Protect against stolen invitation links, malicious or removed room members, forged device registration, replayed grants, untrusted pack contents, compromised relay operators, network attackers, stale commands and accidental concurrent world writers. A malicious local administrator or the world host can inspect/modify their own machine; do not claim protection from that host's full control.

## Trust boundaries

The webview is not trusted to authorize operations. Room API validates identity and membership. Host agent validates device consent, epoch, ticket and peer binding. Game identity adapter verifies actual player binding. Relay enforces infrastructure access/quotas without interpreting encrypted payload. Each boundary has negative tests.

Room invitations select only a known backend room. They never configure an issuer, arbitrary callback URL, artifact executable, relay trust root or command. Validate protocol links before invoking actions; preview and consent are required for joining a new room.

## Mandatory controls

- Credentials stay in agent/backend secret facilities; redact headers, grant tokens, invitation fragments, player session material and private keys before logging.
- No dynamic shell commands from webview, room metadata or pack manifests. Use explicit executable paths and argument arrays; sanitize Java injection variables.
- Apply the outbound source policy below to backend and desktop native fetches alike, including metadata, artifacts, covers and pack-provided resource references. Local file imports use separate explicit user handles.
- Archive extraction is confined to owned staging with decompressed size/count/ratio limits, duplicate-path handling and symlink/hardlink protections. Test Windows reserved names, case collisions, Unicode and long paths.
- Verify signed metadata against fixed trusted keys where used. Do not accept a signature key supplied by the same untrusted manifest as proof of publisher trust.
- Limit input size, nesting, string lengths, decompression, concurrent tasks and buffer allocations.
- Restrict local sockets/pipes and server management endpoints. No public console or arbitrary destination proxy.
- Rotate backend signing keys with key IDs, overlap for the maximum grant lifetime and emergency revocation. Never ship private signing keys in desktop bundles.
- Revalidate role and epoch at execution time for delayed commands, not only at enqueue time.
- Signed desktop/agent update metadata and platform packaging controls must protect background executables. Drain sessions before incompatible updates.

## Revocation

Implement member removal, ban, invite revoke, device revoke, account session revoke and room deletion as distinct operations. Invitation revocation prevents future redemption; it does not silently revoke an already-created membership. Membership removal closes active admission channels. Device revocation affects all rooms using that device. Each operation leaves a minimal audit record and obeys the deadlines in [03](03-OAUTH-AND-IDENTITY.md).

Grant renewal fails closed during authority outages. Do not extend lease validity because an HTTP request is slow. Once deadlines expire, disconnect guest channels and mark hosting unavailable while preserving the world and managing graceful stop.

## Privacy and diagnostics

Default product analytics are opt-in; operational relay byte counters and short-lived security records necessary to run the service are separately disclosed. Never upload gameplay payload, chat, world contents or complete launch commands as routine telemetry. Distinguish structured agent logs from unstructured Java stdout/stderr, which may contain chat, tokens or user-controlled text. Always drain pipes, but by default persist only adapter-classified operational events and bounded sanitized errors; unknown raw lines are discarded. An explicit local debug-capture option may retain bounded raw game logs with a warning and preview, but they are never automatically uploaded. Redaction is defense in depth and is not a guarantee that arbitrary mod output contains no secrets.

Proposed defaults: local logs 7 days with a 50 MiB cap; operational server logs 14 days; security audit events 30 days; aggregated capacity metrics 90 days. Freeze deployment-specific retention in P12 and enforce deletion jobs. Endpoint IPs are restricted operational metadata; avoid high-cardinality identifiers in public dashboards.

Diagnostic export is local preview-first: select components, run secret/path redaction, show size/content list, then save or explicitly upload. No automatic message or upload to support contacts. Delete product account data through the existing account service's policy; do not infer that this deletes local or shared world copies on other hosts.

## Release blocker examples

Room member can target another local port; rejected participant keeps sending after lease expiry; UI can execute arbitrary Java arguments; private key appears in an archive; restore modifies the live world before validation; lost host lease permits a second authoritative writer; relay defaults allow unauthenticated public use. Any of these blocks release regardless of happy-path demos.

The gameplay authorization gate precedes world access; a post-join nickname check is a release blocker. The status UI is not backed by a less-protected raw tunnel. Private transport contexts must not share direct-discovery state with public contexts. Secret-bearing retry receipts follow short encrypted retention, not generic 24-hour plaintext journaling. Source and restored database authority follow [18](18-AUTHORITY-AND-MAINTENANCE.md).

## Game-created files are a separate logging surface

Filtering stdout/stderr does not control files written directly by Java, its logging framework, crash handlers or mods. For each qualified tuple, enumerate those sinks and configure the supported game logging path to disable raw file persistence by default or route only classified operational events to the managed log facility. Disable automatic heap dumps by default; an opted-in diagnostic dump may contain credentials and world data. Never quietly collect crash directories in a backup, export, upload or support bundle.

P06 proves the baseline policy by inspecting the run directory after normal play, synthetic chat/error output and a crash. P13/P14 repeat sink qualification for each supported pack. Arbitrary executable mods can write arbitrary files under their OS permissions; a failed sink audit cannot be labelled secret-free. If a tuple cannot satisfy the default logging policy, gate it as unsupported under that policy or obtain a concrete disclosed local-debug exception; no automatic upload follows. Unknown files remain excluded from diagnostics until reviewed, and portable-world classification still follows [11](11-WORLDS-AND-BACKUPS.md).

## Outbound fetch and credential boundary

Use provider-scoped HTTPS origin policies on both backend and native desktop downloaders. Validate every redirect and the actual resolved destination used by the connection; reject loopback, private, link-local, multicast and other non-public targets, including IPv4-mapped IPv6 representations, in untrusted remote-content fetches. Bound redirects and DNS retries, preserve certificate/hostname verification and prevent a second unchecked resolution after validation. Explicit trusted development services use separate deployment configuration and cannot be selected by a room manifest. Required private artifact services need a separately reviewed origin policy, never a generic bypass.

Separate account/control HTTP clients from content clients. Never forward Room API access tokens, device proofs, invitation secrets, browser cookies or ambient OS credentials to artifact/cover origins. A provider-specific download credential is scoped to its reviewed origin and is removed before a cross-origin redirect; sign scoped requests only after destination validation. Reject URLs with embedded credentials and redact signed URL query secrets from receipts/logs, retaining an opaque source handle where necessary. Redirect failure cannot fall back to an unrestricted webview fetch. See S11/S12 in [sources](SOURCES.md); the concrete egress policy is a product requirement.
