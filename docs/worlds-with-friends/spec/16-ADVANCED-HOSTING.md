# 16 — Expansion contracts

These features are specified for future phases; they are not part of P12 completion. Each requires a separate capability flag, acceptance evidence and operating budget.

## P15 — Separate home host

Package the agent for headless operation on an explicitly qualified platform. Enroll through an owner-approved short-lived pairing flow using the existing provider's supported mechanism. If device authorization is not available, use a browser-mediated enrollment that binds a public key; never ask the owner to paste refresh credentials into the terminal.

The owner grants selected rooms start/stop rights, CPU/RAM/disk limits, schedules and optional idle shutdown. The agent must already be running and reachable. Remote wake of a powered-off computer is a separate capability and cannot be promised for arbitrary networks. Restrict filesystem roots, installer sources and administrative commands. Starting a new executable generation requires owner consent under a persistent content policy.

Test owner revocation, lost pairing challenge, wrong account, stale remote start, offline host, restart after reboot and simultaneous local/remote operations. Publish whether the agent runs at login or as a system service and its exact privilege boundary.

## P16 — Coordinated host handoff

Implement a stop-copy-start workflow, not live JVM migration:

1. Allocate a backend maintenance operation with kind=handoff and preflight target capacity, exact pack/world references, runtime, identity capability and source/target device consent. Retain this operation through transfer; a stopped gameplay lease is not sufficient authority for a long handoff.
2. Notify players and close admissions. Stop source Java cleanly and retain the source world lock.
3. Create and verify the final snapshot. Freeze its digest and source world generation in the handoff record.
4. Transfer over an authenticated file-transfer channel with quotas; verify the complete snapshot at target.
5. Persist a source surrender receipt and a target verified-candidate receipt under the same operation. Commit the expected placement revision, target device, pack and world generation using [18](18-AUTHORITY-AND-MAINTENANCE.md). Publish the target local pointer and acknowledge it before any new start operation may allocate a gameplay host epoch.
6. Start target from the verified candidate; publish ready only after normal probes.
7. Rejoin clients with fresh tickets. Preserve the source copy as read-only history until retention allows removal.

A network partition before source surrender must not promote an old target snapshot. If surrender is uncertain, mark the operation blocked and reconcile; users may explicitly fork a new room, never silently merge two histories. After surrender, source does not resume without a new backend authority decision. If target startup fails, rollback authority only after proving no competing target process is writing; preserve both diagnostic copies.

## P17 — Cloud host

Use the same server adapter and room contract. Add per-room compute/storage budgets, region, explicit purchase authorization, spend estimates, hard service limits and idle-stop policy. A relay is not a cloud game host. Isolate untrusted modded workloads with least privilege, bounded resources and dedicated storage credentials; a generic process under a shared privileged account is insufficient.

Persistent encrypted snapshots, tested restore, deletion/export policy and recovery objectives must be defined. Authorize transfers of private worlds explicitly. Meter compute, storage and egress separately, reconcile provider usage and never claim a flat cost without measured constraints. One-click return to a home host uses the same handoff protocol.

## P18 — Integrated-world sharing and voice

Integrated-world sharing uses a qualified client bridge to discover/control the active world and publish a scoped connection. The world lives with the host's game process. Closing that game ends the session; a handoff requires a clean save/export into a dedicated server. Do not claim an unmodified client gained in-game controls automatically.

Voice is an independent opt-in subsystem with capture permission, device selection, mute, per-member blocking, moderation and explicit codec/transport tests. Positional voice needs an in-game bridge and a separate identity mapping. UDP/datagram transport and additional mod services are not automatically covered by the TCP gameplay bridge. Voice must not inherit unrestricted LAN access or share an unbounded queue with game data.

Each P18 subfeature may ship independently behind separate gates. Companion code must be tested per loader/game tuple and its failure must not take down the dedicated-server core.

Long handoffs renew the separate maintenance permit; an expired operation retains a reconciliation barrier and does not allow a second host to start. A backend commit whose response was lost is queried by operationId. Neither side guesses that a timeout means abort. A clean source snapshot transferred twice does not create two authoritative generations.

Cloud cost ceilings need a reservation model, provider metering delay and shutdown/storage/network overrun allowance. A service can enforce its own provisioning budget but cannot promise that delayed third-party billing stops at an exact currency amount. Freeze the chosen provider's bounded operating model and show the allowance before purchase; no real money is spent during specification validation.
