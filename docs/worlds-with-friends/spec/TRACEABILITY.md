# Requirements traceability

Requirement definitions are in [01](01-PRODUCT.md); case definitions are in [ACCEPTANCE](ACCEPTANCE.md). This table is a design mapping, not completion evidence. Some primary implementation phases precede final cross-platform qualification.

| Requirement | Authoritative detail | Primary phase | Acceptance coverage |
| --- | --- | --- | --- |
| FR01 | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | [P02](phases/P02-ACCOUNTS-DEVICES-AND-DATA.md) | A01, A02, A70, A72, A79, A89 |
| FR02 | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | [P07](phases/P07-ROOMS-AND-ADMISSION.md) | A04, A44, A59, A61, A78, A81, A87, A90 |
| FR03 | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | [P07](phases/P07-ROOMS-AND-ADMISSION.md) | A05, A37, A60, A71, A79, A82, A91 |
| FR04 | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | [P07](phases/P07-ROOMS-AND-ADMISSION.md) | A06, A44, A76, A79 |
| FR05 | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | [P02](phases/P02-ACCOUNTS-DEVICES-AND-DATA.md) | A03, A12, A57, A58, A70, A91 |
| FR06 | [06-SERVER-INSTALLATION.md](06-SERVER-INSTALLATION.md) | [P06](phases/P06-DEDICATED-SERVER.md) | A15, A19, A20, A43, A59, A67, A68, A78, A81, A84 |
| FR07 | [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md) | [P04](phases/P04-TRANSPORT-SPIKE.md) | A09, A11, A14, A53, A54, A55, A80, A86 |
| FR08 | [08-RELAY-OPERATIONS.md](08-RELAY-OPERATIONS.md) | [P05](phases/P05-RELAY-INFRASTRUCTURE.md) | A12, A13, A34, A38, A56, A57, A58, A91 |
| FR09 | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | [P07](phases/P07-ROOMS-AND-ADMISSION.md) | A09, A10, A53, A54, A68, A80, A83 |
| FR10 | [10-PACK-SYNCHRONIZATION.md](10-PACK-SYNCHRONIZATION.md) | [P08](phases/P08-JOIN-ORCHESTRATION.md) | A21, A22, A43, A55, A66, A83, A85, A86 |
| FR11 | [02-ARCHITECTURE.md](02-ARCHITECTURE.md) | [P10](phases/P10-COMPLETE-ROOM-UX.md) | A18, A36, A42, A67, A75 |
| FR12 | [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md) | [P09](phases/P09-WORLD-SAFETY.md) | A24, A25, A26, A27, A60, A61, A62, A63, A64, A81, A82, A83, A87, A88 |
| FR13 | [05-STORAGE-AND-CONCURRENCY.md](05-STORAGE-AND-CONCURRENCY.md) | [P06](phases/P06-DEDICATED-SERVER.md) | A16, A23, A65, A66, A85 |
| FR14 | [13-SECURITY.md](13-SECURITY.md) | [P12](phases/P12-CORE-RELEASE.md) | A30, A31, A32, A53, A54, A69, A70, A72, A73, A76, A79, A82, A84, A85, A86, A90 |
| FR15 | [14-TESTING.md](14-TESTING.md) | [P11](phases/P11-RECOVERY-AND-NETWORKS.md) | A28, A34, A35, A36, A56, A61, A62, A75, A78, A81, A88, A89, A90 |
| FR16 | [15-DEPLOYMENT.md](15-DEPLOYMENT.md) | [P12](phases/P12-CORE-RELEASE.md) | A33, A39, A41, A74 |
| FR17 | [08-RELAY-OPERATIONS.md](08-RELAY-OPERATIONS.md) | [P12](phases/P12-CORE-RELEASE.md) | A38, A40, A58, A73, A74, A84 |
| FR18 | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | [P07](phases/P07-ROOMS-AND-ADMISSION.md) | A07, A08, A12, A35, A56, A60, A71, A72, A79, A80, A82, A88, A89, A90 |
| FR19 | [12-UX.md](12-UX.md) | [P10](phases/P10-COMPLETE-ROOM-UX.md) | A29, A36, A42, A55, A73, A78, A91 |
| FR20 | [05-STORAGE-AND-CONCURRENCY.md](05-STORAGE-AND-CONCURRENCY.md) | [P06](phases/P06-DEDICATED-SERVER.md) | A17, A23, A40, A57, A62, A65, A75, A87, A89 |
| FR21 | [10-PACK-SYNCHRONIZATION.md](10-PACK-SYNCHRONIZATION.md) | [P13](phases/P13-FABRIC-AND-PACKS.md) | A45, A64, A66, A83 |
| FR22 | [10-PACK-SYNCHRONIZATION.md](10-PACK-SYNCHRONIZATION.md) | [P14](phases/P14-ADDITIONAL-LOADERS.md) | A46, A47, A68 |
| FR23 | [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md) | [P15](phases/P15-REMOTE-HOME-HOST.md) | A48, A59, A71, A75 |
| FR24 | [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md) | [P16](phases/P16-HOST-HANDOFF.md) | A49, A61, A77, A87, A88 |
| FR25 | [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md) | [P17](phases/P17-CLOUD-HOSTING.md) | A50, A72, A77 |
| FR26 | [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md) | [P18](phases/P18-IN-GAME-AND-VOICE.md) | A51, A52, A53, A57 |

Core FR01–FR20 must pass their relevant cases at P12. FR21–FR26 remain unavailable until their own milestone passes. Record implementation paths and evidence links as work proceeds, preserving this requirement-to-case mapping.

Revision 1.0.1 adds A53–A77 without renumbering earlier cases. That revision had core coverage A01–A44 plus A53–A76; A77 gates handoff. Current coverage is stated below. New authority and launcher contracts refine existing FR IDs rather than introducing unrelated scope.

Revision 1.0.2 adds A78–A91, all core, preserving previous IDs. Current core coverage is A01–A44, A53–A76 and A78–A91 (82 cases); A45–A52 and A77 remain expansion (9 cases). The latest ACCEPTANCE catalog governs classification, not numeric ranges inferred from an earlier revision.
