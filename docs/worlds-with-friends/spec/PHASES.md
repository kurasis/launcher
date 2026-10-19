# Implementation phases

These P01–P18 IDs are local to Worlds with Friends. The initial chatbot request completes P01 only. On continuation, select the first requested phase whose dependencies are complete. P01–P12 deliver core; later phases are independent feature milestones where their graph allows it.

| Phase | Outcome | Dependencies | Milestone |
| --- | --- | --- | --- |
| [P01](phases/P01-FOUNDATION-AND-CONTRACTS.md) | Foundation and integration contracts | — | Core |
| [P02](phases/P02-ACCOUNTS-DEVICES-AND-DATA.md) | Own accounts, device registration and data model | P01 | Core |
| [P03](phases/P03-BACKGROUND-AGENT.md) | Independent agent, IPC and resource locks | P02 | Core |
| [P04](phases/P04-TRANSPORT-SPIKE.md) | Transport and identity-aware TCP bridge spike | P03 | Core |
| [P05](phases/P05-RELAY-INFRASTRUCTURE.md) | Private relay infrastructure and access enforcement | P04 | Core |
| [P06](phases/P06-DEDICATED-SERVER.md) | Exact dedicated-server install and supervision | P03 | Core |
| [P07](phases/P07-ROOMS-AND-ADMISSION.md) | Rooms, permissions, leases and verified admission | P02, P04, P05, P06 | Core |
| [P08](phases/P08-JOIN-ORCHESTRATION.md) | Isolated profiles and invitation-to-game flow | P07 | Core |
| [P09](phases/P09-WORLD-SAFETY.md) | World import, verified snapshots and restore | P06, P08 | Core |
| [P10](phases/P10-COMPLETE-ROOM-UX.md) | Complete desktop room and hosting experience | P08, P09 | Core |
| [P11](phases/P11-RECOVERY-AND-NETWORKS.md) | Fault recovery and real network qualification | P05, P07, P09, P10 | Core |
| [P12](phases/P12-CORE-RELEASE.md) | Core security, packaging and release qualification | P11 | Core |
| [P13](phases/P13-FABRIC-AND-PACKS.md) | Modpack generations and first Fabric room | P12 | Expansion |
| [P14](phases/P14-ADDITIONAL-LOADERS.md) | NeoForge, Forge and Quilt qualification | P13 | Expansion |
| [P15](phases/P15-REMOTE-HOME-HOST.md) | Remote home agent and delegated start | P12 | Expansion |
| [P16](phases/P16-HOST-HANDOFF.md) | Verified host handoff and conflict control | P15, P09 | Expansion |
| [P17](phases/P17-CLOUD-HOSTING.md) | Optional budgeted cloud hosting | P16 | Expansion |
| [P18](phases/P18-IN-GAME-AND-VOICE.md) | Optional integrated worlds and voice | P14 | Expansion |

P15 may follow P12 without waiting for every loader. P16 depends on P09 transitively and lists it explicitly because safe snapshots are critical. P18 does not depend on cloud hosting. This graph describes prerequisites, not permission to start parallel agents or deploy production services.

All phases begin as `not_started`. Allowed states: `not_started`, `in_progress`, `blocked`, `completed`. Only one phase is actively implemented by default. A blocked dependency prevents dependent completion but not useful independent research. Every completed phase needs its own evidence and required CI.

## Stage ownership (current contract)

The dependency graph is unchanged. P02 validates account libraries in a headless harness; P03 becomes their production owner. P04 freezes transport pins after its spike. P06 qualifies real server execution without public admission; P07 implements production session/identity authority. P07 owns initial world authority publication; P09 extends backend/local data-commit integration to general maintenance. P12 requires A01–A44, A53–A76 and A78–A91. Minimal identity bootstrap dependencies are pulled into P06/P07; general modpack support remains P13/P14.

## Revision 1.0.2 phase clarification

P06 supplies first-world generation in isolated mode. P07 must implement the minimal maintenance publication transaction needed for the first room; it cannot defer that prerequisite to P09. P09 adds general import/snapshot/restore/export and recovery workflows. P02 owns shared operation schemas; later phases qualify actual game/room behavior. The dependency graph stays unchanged. Current release scope is 82 core acceptance cases and 9 separately gated expansion cases.
