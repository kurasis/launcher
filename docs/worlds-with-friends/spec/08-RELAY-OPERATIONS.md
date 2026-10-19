# 08 — Relay service and operating model

## Responsibility

Relay nodes forward encrypted transport traffic and assist connectivity. They do not simulate worlds, store backups, authenticate game profiles or authorize room administration. Keep game payload off the Room API and database. Keep relay metadata limited and document what operators can observe.

Use the upstream iroh relay implementation behind a product-specific access policy, not a newly invented cryptographic protocol. Its existing access-control and limit hooks are integration points; detailed interfaces must match the pinned release. The current constructor defaults are not a production security policy. See S05/S06 in [sources](SOURCES.md).

## Access design

The Room API authorizes registered endpoint IDs to use relay infrastructure for short periods. The relay policy validates endpoint proof and a fresh backend-managed allow record. It does not accept a client-supplied arbitrary account claim. Configure fail-closed enrollment and renewal, bounded policy caching and explicit operator emergency revocation.

Room tickets and relay permission are different layers. A permitted endpoint could still attempt non-game traffic because payload is encrypted. Apply per-device/account traffic quotas, concurrent connection limits and abuse detection. Device-count limits prevent unlimited self-enrollment from bypassing an account budget.

For the initial implementation, refresh authorized endpoint records every 10 seconds with a 30-second authorization lifetime. Close revoked/expired relay connections using the actual pinned implementation's supported controls. If the upstream hook cannot meet this contract, implement and test a wrapper/patch or select another qualified transport; do not claim expiry enforcement from an initial connection check only. Active room streams separately obey host admission leases.

## Deployment topology

Development: one local API, database and relay with generated development credentials. Private alpha: one publicly reachable relay. Public core release: at least two separately restartable relay nodes, with explicitly tested discovery and failure behavior. Choose regions from measured user routes; no arbitrary worldwide latency promise.

Use a DNS name, valid TLS, automated certificate renewal, restricted administrative access and process-level restart supervision. Configure the pinned implementation's required TCP listeners and optional UDP discovery services from its release documentation. Do not assume a generic HTTP CDN/reverse proxy passes its upgrade protocol or long-lived streams. Test the exact proxy/load balancer path, idle timeouts, TLS termination and payload limits.

Separate the public listener, health endpoint and private metrics/admin access. Do not expose metrics secrets or provisioning APIs on the public relay route. A room needs no public port allocation per guest; streams are multiplexed over agent connections.

## Capacity and quotas

Publish limits for endpoints, connections, rates, burst allowance and memory/queue bounds. Enforce both per-principal and per-node limits, with bounded waiting and clear capacity errors. Gameplay must not share an unrestricted FIFO with large snapshot or mod downloads; core downloads use their original artifact sources.

A small 2-vCPU/2–4-GiB Linux node is only a test starting point, never a promised player capacity. Measure CPU, RSS, file descriptors, packet rate, ingress/egress, queue time, connection churn and long-duration stability under the exact deployment.

Cost model for one forwarding hop, using decimal GB:

`egressGB = averageRemotePlayers * relayedFraction * averageBidirectionalMbps * hours * 0.45`.

The per-player rate includes both gameplay directions, each forwarded once. Provider ingress billing, inter-relay hops, retries, protocol overhead and burst peaks are additional terms. Example assumptions of 1000 concurrent remote players, 20% relayed and 0.5 Mbps yield 45 GB/hour of egress; these are illustrative inputs, not measured game usage. Record actual provider pricing separately at deployment time.

Do not derive business billing from unauthenticated client counters. Meter server-side by authorized endpoint/account, define period boundaries, reconcile duplicates, and state whether overhead is included. Quota warnings must precede ordinary usage exhaustion when possible; protection against abuse may act immediately. No invisible mid-session paywall.

## Failure and rollout

Test node termination, blackhole, packet loss, TLS expiry, policy API outage and saturated queues. Two relays are not proof of seamless failover. Record time to detect, time to usable route and whether a game rejoin was required.

For upgrades, stop assigning new endpoints to a draining node, allow existing sessions a bounded drain window, notify clients if reconnection is necessary and then roll the pinned release. Maintain protocol compatibility across current and previous supported agent releases or schedule an explicit coordinated upgrade.

## Operator deliverables

Provide infrastructure templates, pinned image/binary digests, certificate procedures, access-policy configuration, quota settings, metrics dashboards, alert thresholds, startup checks, restore instructions for policy data and an incident runbook. Include a public health signal that does not disclose room membership or endpoint IPs.

## Permission renewal and accounting precision

Relay allow records bind the registered endpoint context, parent device/account, purpose, scope, authorityEpoch, issuedAt, expiresAt and policy revision. Apply the same conservative request-start deadline rule as admission leases; a stale allow cache must never mint fresh lifetime. A delayed signed allow response cannot reopen revoked endpoints after its original deadline. Reject unknown contexts before forwarding and cap unauthenticated connections across all keys/IPs.

For per-room cost attribution, a context is reserved to one room/session purpose and mapped by server authority, not by a plaintext room label asserted by a client. This permits attributing bytes to the authorized context; it still does not prove encrypted payload is game data. Product billing is out of core scope; operational metering is required. If several relay hops are used, count each provider-billed hop and distinguish physical traffic from logical per-context usage. Never double-bill one logical transfer merely because two metering services saw it.

The current upstream configuration exposes access control and rate-limit hooks, but this is not evidence that it periodically revalidates existing connections or implements the product's per-account quotas. P05 must qualify or add those controls before passing A12/A58. Do not switch to an open public relay if a private policy service fails.
