# 02 — Architecture and base integration

## Components

| Component | Owns | Must not own |
| --- | --- | --- |
| Tauri/React desktop UI | Presentation, local intentions, confirmations | Refresh credentials, authoritative room policy, Java process lifetime |
| Per-user Rust agent | Local installation jobs, Java supervision, tunnels, snapshots, resource reservations | Room ownership decisions based on cached UI state |
| Room API | Membership, invites, device registry, host leases, admission grants, command log | World simulation or hot-path game packet forwarding |
| Relay service | Authorized endpoint connectivity and encrypted packet forwarding | Game interpretation, room database ownership, backup storage |
| Existing own OAuth service | Product account authentication and API authorization | Automatic device trust without registration |
| Game identity adapter | Verified relation between product identity and actual game session identity | Trusting a client-supplied display name |
| Optional storage/hosting services | Snapshots and cloud compute in later milestones | Implicit access to every local world |

Prefer extending the existing room backend. If none exists, use a small Rust service with Tokio, Axum and PostgreSQL, SQLx migrations, generated OpenAPI and a transactional outbox. These are proposed implementation defaults; pin compatible versions during P01. Redis, orchestration clusters and message brokers are not core prerequisites.

## Agent process

The agent is a separately packaged executable, not a task whose lifetime depends on a Tauri window or its parent process. Run per OS user and DATA_ROOT. Use OS-supported authenticated local IPC and one OS-backed storage-owner lock. Tauri may start or reconnect to it. Packaged process ownership and reparenting behavior must be proven on each OS; do not assume a generic sidecar command guarantees independence.

An updater must drain active sessions before replacing the agent. Never hot-replace the executable or migrate its database while server/tunnel work uses it. Keep an explicit UI/agent protocol range and produce an actionable incompatibility state.

The agent handles restricted typed operations. No arbitrary shell, command line, environment variable or filesystem access from the UI. The frontend receives redacted DTOs and resumable snapshots; it never receives the agent's unrestricted local credential or device private key.

## Base launcher changes

Inspect actual storage/process ownership first. If the current launcher has a global activity mutex or another managed-storage writer, migrate the affected capabilities to the lock model in [05](05-STORAGE-AND-CONCURRENCY.md). Reuse an existing qualified agent when possible. Only one owner writes a shared managed root; Tauri delegates those operations instead of introducing a second writer. If storage roots are separate, document their boundary rather than refactoring unrelated features. The adapter contract is in [19](19-LAUNCHER-INTEGRATION.md).

Maintain one hosted server plus one local game client. Permit independent downloads with immutable atomic cache publication; forbid replacing/deleting objects pinned by any live process. Instance-specific game-facing projections prevent two pinned generations from changing each other's asset indexes or library layout.

## Interfaces to implement

Define typed interfaces for `AccountSession`, `DeviceIdentityStore`, `RoomRepository`, `RoomControlClient`, `GameIdentityAdapter`, `ServerAdapter`, `ClientLaunchAdapter`, `RuntimeProvider`, `ArtifactStore`, `TunnelTransport`, `RelayAccessPolicy`, `HostLeaseStore`, `SnapshotStore`, `ProcessSupervisor`, `PlatformServiceManager` and `Clock`.

Transport, client/server build and UI must not depend on one another's concrete libraries. Rust types generate or validate TypeScript DTOs and API schemas. Test doubles are allowed only under explicit test configuration with a visible build marker; they cannot satisfy release gates.

## Suggested repository additions

| Path | Purpose |
| --- | --- |
| `src/features/rooms/` | Room catalog, join, host and recovery views |
| `crates/wwf-contracts/` | Versioned DTOs and errors |
| `crates/wwf-agent/` | Independent background executable |
| `crates/wwf-transport/` | Stream admission and TCP forwarding |
| `crates/wwf-server/` | Runtime/install/server adapters |
| `crates/wwf-storage/` | Locks, content objects, receipts and snapshots |
| `services/wwf-api/` | Only if an existing backend cannot be extended |
| `services/wwf-relay/` | Pinned relay deployment and access integration |
| `infra/wwf/` | Development stack and deployment templates |
| `docs/worlds-with-friends/` | Specification, decisions and evidence |

Adapt paths to the real repository in an ADR rather than duplicating existing facilities. No Node runtime is required on users' machines; Vite is a build tool. Managed Java and platform webview dependencies remain explicit installer responsibilities.
