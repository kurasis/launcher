# Audit report — revision 1.0.1

Date: 2026-10-19 (Asia/Dubai). Input: all 45 Markdown documents in the delivered v1.0.0 archive. Output: a separate corrected specification; the original is preserved. This is a requirements/design audit, not a software test or penetration test.

## Findings

Severity describes consequences of implementing the ambiguous original requirements. It does not assert a deployed vulnerability. Every issue has a normative correction and regression case; none is marked as a passed implementation test.

| Finding | Severity | Original problem | Corrected contract | Regression |
| --- | --- | --- | --- | --- |
| F01 | Critical | Raw status stream could bypass gameplay admission | [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md) | A53 |
| F02 | Critical | Identity verification could occur after world access | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | A54 |
| F03 | High | Ticket and slot were consumed before slow Java startup | [12-UX.md](12-UX.md) | A55 |
| F04 | High | Delayed renewal responses could extend revoked authority | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | A56 |
| F05 | High | Mixed privacy modes could share direct-discovery identity | [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md) | A57 |
| F06 | High | Relay hooks did not prove active expiry or account metering | [08-RELAY-OPERATIONS.md](08-RELAY-OPERATIONS.md) | A58 |
| F07 | High | Start command required a session that did not exist yet | [09-API-AND-IPC.md](09-API-AND-IPC.md) | A59 |
| F08 | High | Local world operations lacked renewable backend authority | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A60 |
| F09 | Critical | Local pointer and backend placement were not one transaction | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A61 |
| F10 | High | Snapshot locks could deadlock or release custody early | [05-STORAGE-AND-CONCURRENCY.md](05-STORAGE-AND-CONCURRENCY.md) | A62 |
| F11 | High | Restore validation could mutate the restore candidate | [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md) | A63 |
| F12 | High | Live copy was treated as a consistent import/test world | [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md) | A64 |
| F13 | High | Alternative cache location lacked a frozen-lock migration contract | [05-STORAGE-AND-CONCURRENCY.md](05-STORAGE-AND-CONCURRENCY.md) | A65 |
| F14 | High | Pack identity was self-referential and platform-ambiguous | [10-PACK-SYNCHRONIZATION.md](10-PACK-SYNCHRONIZATION.md) | A66 |
| F15 | High | Early phases required later implementation facilities | [START-HERE.md](START-HERE.md) | A67 |
| F16 | High | Core identity bootstrap could be deferred to expansion | [06-SERVER-INSTALLATION.md](06-SERVER-INSTALLATION.md) | A68 |
| F17 | Medium | Integration assumed an earlier architecture/specification | [19-LAUNCHER-INTEGRATION.md](19-LAUNCHER-INTEGRATION.md) | A69 |
| F18 | Medium | Device proof conflated distinct protocol guarantees | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | A70 |
| F19 | High | Owner transfer retained ambiguous consent and delegation | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | A71 |
| F20 | Critical | Database restore could resurrect counters and revoked access | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A72 |
| F21 | High | Raw Java logs contradicted telemetry privacy claims | [13-SECURITY.md](13-SECURITY.md) | A73 |
| F22 | Medium | Missing platforms or failed budgets could become a pass | [14-TESTING.md](14-TESTING.md) | A74 |
| F23 | High | Agent crash lost locks and pipes despite a living child | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A75 |
| F24 | High | Digest-only invites conflicted with persisted secret results | [09-API-AND-IPC.md](09-API-AND-IPC.md) | A76 |
| F25 | High | Long handoff reused expired gameplay authority | [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md) | A77 |

## Resolution notes

- F01–F06 close admission, expiry and routing ambiguities: no raw status tunnel, verification before world access, just-in-time tickets, conservative lease deadlines, isolated privacy contexts and active relay enforcement.
- F07–F12 define idle-room start, renewable maintenance authority, recoverable backend/local publication, lifecycle-actor lock ownership, disposable restore validation and consistent source copying.
- F13–F18 clarify deferred live-cache repair, detached cross-platform pack identity, staged gates, minimal core bootstrap support, standalone integration and mechanism-specific device proof.
- F19–F24 address ownership consent, non-rollback disaster-recovery authority, raw game logs, honest release gates, orphan-process control and encrypted short-lived secret results.
- F25 gives long host handoffs independent authority and recoverable publication after gameplay stops.

Product scope remains Worlds with Friends with own OAuth accounts and rooms. The earlier launcher archive is not required. Russian development chat, English repository artifacts and the authorized Git workflow are preserved.

## Remaining work and limits

The actual repository/provider contracts, trusted game identity integration, dependency/runtime versions, public relay behavior and platform qualification still require implementation evidence. These corrections do not guarantee bug-free future software. The original packaging validation established structural consistency; it did not detect all semantic problems. The revised check includes explicit regression contracts and corrected normative wording.

## Changed-file inventory

- [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md)
- [SPEC-AUDIT.md](SPEC-AUDIT.md)
- [PHASES.md](PHASES.md)
- [14-TESTING.md](14-TESTING.md)
- [06-SERVER-INSTALLATION.md](06-SERVER-INSTALLATION.md)
- [CHANGELOG.md](CHANGELOG.md)
- [15-DEPLOYMENT.md](15-DEPLOYMENT.md)
- [TRACEABILITY.md](TRACEABILITY.md)
- [12-UX.md](12-UX.md)
- [PROGRESS.md](PROGRESS.md)
- [13-SECURITY.md](13-SECURITY.md)
- [02-ARCHITECTURE.md](02-ARCHITECTURE.md)
- [START-HERE.md](START-HERE.md)
- [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md)
- [09-API-AND-IPC.md](09-API-AND-IPC.md)
- [10-PACK-SYNCHRONIZATION.md](10-PACK-SYNCHRONIZATION.md)
- [05-STORAGE-AND-CONCURRENCY.md](05-STORAGE-AND-CONCURRENCY.md)
- [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md)
- [17-INTEGRATION-GATES.md](17-INTEGRATION-GATES.md)
- [16-ADVANCED-HOSTING.md](16-ADVANCED-HOSTING.md)
- [ACCEPTANCE.md](ACCEPTANCE.md)
- [README.md](README.md)
- [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md)
- [08-RELAY-OPERATIONS.md](08-RELAY-OPERATIONS.md)
- [SOURCES.md](SOURCES.md)
- [BOT-START.md](BOT-START.md)
- [phases/P17-CLOUD-HOSTING.md](phases/P17-CLOUD-HOSTING.md)
- [phases/P04-TRANSPORT-SPIKE.md](phases/P04-TRANSPORT-SPIKE.md)
- [phases/P06-DEDICATED-SERVER.md](phases/P06-DEDICATED-SERVER.md)
- [phases/P13-FABRIC-AND-PACKS.md](phases/P13-FABRIC-AND-PACKS.md)
- [phases/P08-JOIN-ORCHESTRATION.md](phases/P08-JOIN-ORCHESTRATION.md)
- [phases/P03-BACKGROUND-AGENT.md](phases/P03-BACKGROUND-AGENT.md)
- [phases/P01-FOUNDATION-AND-CONTRACTS.md](phases/P01-FOUNDATION-AND-CONTRACTS.md)
- [phases/P18-IN-GAME-AND-VOICE.md](phases/P18-IN-GAME-AND-VOICE.md)
- [phases/P16-HOST-HANDOFF.md](phases/P16-HOST-HANDOFF.md)
- [phases/P11-RECOVERY-AND-NETWORKS.md](phases/P11-RECOVERY-AND-NETWORKS.md)
- [phases/P14-ADDITIONAL-LOADERS.md](phases/P14-ADDITIONAL-LOADERS.md)
- [phases/P10-COMPLETE-ROOM-UX.md](phases/P10-COMPLETE-ROOM-UX.md)
- [phases/P02-ACCOUNTS-DEVICES-AND-DATA.md](phases/P02-ACCOUNTS-DEVICES-AND-DATA.md)
- [phases/P15-REMOTE-HOME-HOST.md](phases/P15-REMOTE-HOME-HOST.md)
- [phases/P05-RELAY-INFRASTRUCTURE.md](phases/P05-RELAY-INFRASTRUCTURE.md)
- [phases/P09-WORLD-SAFETY.md](phases/P09-WORLD-SAFETY.md)
- [phases/P12-CORE-RELEASE.md](phases/P12-CORE-RELEASE.md)
- [phases/P07-ROOMS-AND-ADMISSION.md](phases/P07-ROOMS-AND-ADMISSION.md)

Added: 18-AUTHORITY-AND-MAINTENANCE.md, 19-LAUNCHER-INTEGRATION.md and this audit report.
