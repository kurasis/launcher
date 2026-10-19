# Worlds with Friends — Implementation Specification

Version: **1.0.2**. Revision date: **2026-10-19** (Asia/Dubai).
Audience: an autonomous development chatbot and human maintainers.
All repository artifacts are written in English; user-facing development chat is in Russian.

## Product

Create a persistent private room, choose a world, invite friends, and play without manual router configuration. The launcher prepares compatible clients and a local dedicated server. A background agent manages the server and encrypted connectivity, including a relay fallback. Rooms and device access use the product's own OAuth account service.

This archive is a specification, not working software. No game session, provider integration, deployment, capacity figure, or platform qualification is claimed to have been tested by preparing these documents.

## Start

1. Read [START-HERE](START-HERE.md).
2. Use [BOT-START](BOT-START.md) as the development chatbot's initial instruction.
3. Follow [PHASES](PHASES.md); all implementation phases initially remain `not_started` in [PROGRESS](PROGRESS.md).
4. Resolve the real integration inputs in [17 — Integration gates](17-INTEGRATION-GATES.md) before claiming end-to-end success.

## Delivery boundaries

| Milestone | Phases | Deliverable |
| --- | --- | --- |
| Core MVP | P01–P12 | Private rooms, own OAuth accounts, device binding, one local server plus one client per device, direct/relay transport, safe snapshots, recovery, packaged clients |
| Modded rooms | P13–P14 | Reproducible client/server packs; Fabric first; individually qualified additional loader tuples |
| Persistent hosting | P15–P17 | Remote home agent, coordinated host handoff, optional cloud hosting |
| Enhanced cooperation | P18 | Optional in-game bridge for integrated-world sharing and separately engineered voice |

Core scope starts with one explicitly pinned game/server/client tuple whose identity integration is proven. Prefer an unmodified vanilla tuple only if it meets that gate; otherwise qualify the minimal required bridge and bootstrap honestly in P06/P07. Do not advertise every game version or every loader. The architecture supports multiple room versions through separate locks and shared immutable artifacts. An additional tuple is supported only after its acceptance matrix passes.

## Document map

- [01 — Product and requirements](01-PRODUCT.md)
- [02 — Architecture](02-ARCHITECTURE.md)
- [03 — OAuth and identity](03-OAUTH-AND-IDENTITY.md)
- [04 — Rooms and permissions](04-ROOMS-AND-PERMISSIONS.md)
- [05 — Storage and concurrency](05-STORAGE-AND-CONCURRENCY.md)
- [06 — Server installation and lifecycle](06-SERVER-INSTALLATION.md)
- [07 — Tunnel protocol](07-TUNNEL-PROTOCOL.md)
- [08 — Relay operations](08-RELAY-OPERATIONS.md)
- [09 — API and IPC contracts](09-API-AND-IPC.md)
- [10 — Pack synchronization](10-PACK-SYNCHRONIZATION.md)
- [11 — Worlds and backups](11-WORLDS-AND-BACKUPS.md)
- [12 — UX](12-UX.md)
- [13 — Security](13-SECURITY.md)
- [14 — Testing and qualification](14-TESTING.md)
- [15 — Deployment and observability](15-DEPLOYMENT.md)
- [16 — Advanced hosting](16-ADVANCED-HOSTING.md)
- [17 — Integration gates](17-INTEGRATION-GATES.md)
- [18 — Authority, maintenance and recovery](18-AUTHORITY-AND-MAINTENANCE.md)
- [19 — Existing launcher integration contract](19-LAUNCHER-INTEGRATION.md)
- [Second audit findings and fixes](AUDIT-REPORT-v1.0.2.md)
- [Previous audit history](AUDIT-REPORT-v1.0.1.md)
- [Requirements traceability](TRACEABILITY.md), [acceptance cases](ACCEPTANCE.md), [sources](SOURCES.md), [document audit](SPEC-AUDIT.md), [change log](CHANGELOG.md)

## Authority

MUST/MUST NOT are release requirements. SHOULD requires a documented tradeoff if not followed. MAY is optional. Project-specific endpoint shapes and limits below are proposed normative contracts to implement or explicitly map to an existing backend; they are not claims about APIs already deployed.

This add-on owns the Worlds with Friends feature. Reuse base launcher installation, runtime, credential-store, and UI capabilities through explicit adapters. Do not copy the base specification wholesale or silently replace working account infrastructure. If requirements conflict, record the exact conflict and resolve it in P01. Feature-specific identity is the product's own OAuth account system.

## Standalone scope and contract precedence

Only Worlds with Friends is specified here. No earlier specification archive is required. The actual existing launcher repository is the integration target; [19](19-LAUNCHER-INTEGRATION.md) defines the required capabilities without prescribing unrelated launcher work. A shared integration refactor is limited to dependencies genuinely needed by this feature.

[18](18-AUTHORITY-AND-MAINTENANCE.md) is the canonical source for maintenance authority, recovery epochs and commit ordering. [07](07-TUNNEL-PROTOCOL.md) owns wire admission; [03](03-OAUTH-AND-IDENTITY.md) owns account/device identity. The audit report describes historical defects and is not an alternative runtime contract.
