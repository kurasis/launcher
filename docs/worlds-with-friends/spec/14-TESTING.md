# 14 — Verification and qualification

## Evidence policy

Tests must protect real boundaries and failure windows. Do not substitute tests that merely repeat implementation constants. Record commands, commit, release artifact digest, dependency locks, hardware, network topology, OS, Java build and observation. Separate unit fixtures, integration environments, live own-provider tests and real gameplay evidence.

## Qualification matrix

| Dimension | Core obligation |
| --- | --- |
| Desktop targets | Windows x64; macOS Intel and Apple Silicon; Linux x64 on explicitly named supported distributions |
| Baseline build | One pinned client/server/runtime/identity tuple, runtime flavor honestly labelled |
| Mixed OS | At least one host/client cross-OS pair per target; macOS architectures separately exercised |
| Network | Same LAN; separate residential NATs; CGNAT; double NAT; IPv6 where available; blocked UDP/forced relay |
| Privacy | Packet-captured relay-only with no direct candidate publication or peer probes |
| Membership | Two legitimate users, revoked user, wrong device, identity substitution, expired/replayed ticket |
| Lifecycle | UI close/reopen, game exit while host continues, agent crash, Java crash, relay outage, sleep/resume |
| Data | Unicode paths, low disk, snapshot interruption, restore failure, corrupt artifact, concurrent world attempts |

Emulated NAT cases are useful but cannot replace every real network case. A missing required platform/network test leaves the corresponding gate blocked. An owner-approved reduced preview may be published with explicit limitations, but it does not complete the original full core-release gate. The chatbot cannot silently narrow the Windows/macOS/Linux promise to pass P12.

## Initial measurable budgets

These are engineering acceptance targets, not customer guarantees. Measure on named reference hardware with a prepared baseline world and all artifacts already installed.

- Control API p95 below 500 ms at the declared alpha load, excluding provider login and long jobs.
- New admitted tunnel p95 below 5 seconds when both agents and one healthy relay are reachable; measure at least 100 attempts per tested route class.
- Relay-added processing/queue latency p95 below 10 ms at the declared design load, measured separately from geographic network RTT.
- Host revocation observation target 5 seconds with a healthy control channel; expiry upper bound from [03](03-OAUTH-AND-IDENTITY.md).
- Detached idle agent CPU below 1% of one logical core averaged over 5 minutes on reference hardware; record RSS rather than claiming a universal memory number.
- 2-hour five-person gameplay soak; eight-player controlled capacity test; 8-hour transport soak under bounded synthetic traffic with no unbounded RSS or descriptor growth.
- Clean stop, snapshot verification and restore have data-integrity assertions; completion time depends on world size and is measured, not hardcoded as universal.

Freeze reference hardware, load profile, sample window and budgets before the release measurement. If a budget fails, optimize or record a failed gate; any approved product-target change must be explicit, justified and followed by a new run under the revised conditions. Do not lower the load after seeing results and retroactively report the original test as passed.

## Fault injection

Inject crash before/after download publication, lock commit, process spawn receipt, snapshot commit, world pointer swap and command acknowledgment. Drop events, replay requests, race last invite uses, revoke while joining, rotate keys, expire leases during CPU stalls, blackhole relay routes and fill disk while logs are being drained.

Assert world locks remain held until actual process termination, stale commands never affect new sessions, active artifact pins survive GC and corrupted cache repair never overwrites a running object's bytes.

## Required test layers

Unit: policy decisions, parsers, bounded framing, path rules and state transitions.
Integration: real database transactions/migrations, local IPC permissions, two agents, private relay, server process management and generated API clients.
End-to-end: real own OAuth accounts, real selected game/server build, two devices, live identity binding and network paths.
Release: packaged binaries on supported OS targets with update/drain behavior, signed artifacts as applicable and recovery runbooks.

Use the concrete cases in [ACCEPTANCE](ACCEPTANCE.md). Every core requirement has at least one release case in [TRACEABILITY](TRACEABILITY.md). Later milestones add cases without weakening the core.

Re-run original cases where affected and all applicable audit regressions through A91. Core requires A01–A44, A53–A76 and A78–A91 (82 cases). A45–A52 and A77 retain expansion scope (9 cases). Do not classify later case IDs as optional by number.

Revision 1.0.2 additionally exercises cancellation/claim linearization, authorized idempotency reads, game-side release barriers, first-world publication, non-pointer result disclosure, snapshot-bound restore, actual on-disk log sinks, native fetch policies, lossless local parsing, world-reference uniqueness, fenced recovery claims, lease-worker saturation/clock epochs, retention horizons and privacy-policy transitions. Record each failure injection on both sides of its commit/disclosure boundary.
