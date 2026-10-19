# 17 — Required inputs, decisions and integration gates

## Required real inputs

| Gate | Input | Evidence needed | Blocks |
| --- | --- | --- | --- |
| G01 | Existing repository, instructions and CI | Repository inventory, clean change boundary, working baseline checks | P01 completion |
| G02 | Own OAuth provider | Public-client registration, issuer/audience, supported endpoints and real test accounts | P02 live acceptance |
| G03 | Existing room backend | Actual API/schema or explicit approval of proposed replacement/addition | P02/P07 integration |
| G04 | Project game identity integration | P01 credible pre-play hook design; P06 minimal required bootstrap; P07 real profile/connection binding | P01 design and P07 playable acceptance, not a requirement to finish P07 during P01 |
| G05 | Baseline game/server/runtime tuple | Exact hashes/builds plus supported OS matrix | P06 installation evidence |
| G06 | Public relay environment | DNS/TLS, reachable test node, account-limited access | P05/P11 real network tests |
| G07 | Target devices and networks | Supported OS/CPU hosts and real residential/CGNAT test access | P12 qualification |
| G08 | Package signing/update mechanism | Existing release integration or explicitly limited private test distribution | Public release claim |

A missing gate is a blocker for its dependent evidence, not permission to invent credentials or claim completion. Finish contract design and independent implementation before asking the user for the smallest missing input. Record exact gaps in PROGRESS. Test providers, simulated networks and mock identity bridges remain explicitly labelled.

## Decisions to freeze in P01

Record ADRs for: extension versus new room API; process ownership/migration from the base supervisor; IPC transport; own-provider account validation; trusted game identity binding; candidate support tuple/runtime flavor; transport/relay candidate versions with final pins owned by P04; invite and host-consent policy; authorization expiry behavior; storage/custody policy; packaging/update strategy and initial capacity budget.

Do not freeze an exact dependency version solely because documentation says latest. Compile a spike, inspect licenses, record source commit/version and test the operations the feature actually requires. Failed source retrieval during specification preparation remains a verification task, not a verified download recipe.

## Product defaults

Core: private rooms; 8-player qualification target; one hosted room and one local game per desktop agent; stopped-world snapshots; local world custody; automatic route policy with an explicit relay-only option; no public TCP ingress; no voice dependency. Account, endpoint and player identities are distinct. All changes to these defaults require consistent contract and test updates.

## Completion honesty

An executable local demo with mock accounts is not a production login integration. Encrypted connectivity is not game identity binding. A successful server launch is not a verified world restore. Two relay nodes are not proven failover. A content hash is not publisher trust. A written spec is not implementation evidence.

## Gate ownership clarification

Earlier specification archives are not integration inputs. P01 maps [19](19-LAUNCHER-INTEGRATION.md) to the actual repository and selects a candidate. Real account library behavior is demonstrated in P02; independent production agent ownership arrives in P03. Transport pins follow the P04 spike. P06 proves exact local server execution in non-public qualification mode; P07 supplies production session/identity admission. A minimal loader needed by the selected identity bridge is explicitly pulled into P06, without claiming general loader-pack support.

All required P12 targets and budgets stay fixed unless the owner explicitly changes scope. A preview with blocked targets remains a preview. The non-rollback authority and maintenance contracts must be included in backend integration; an existing room API lacking them requires a mapped extension, not fake client-side enforcement.

Revision 1.0.2 additionally requires the real adapter to prove game-side release acknowledgment, stable persistent player mapping and direct-file logging policy. Backend mapping must include start cancellation, initial world publication, result transactions, recovery claims, world-reference uniqueness and stopped-room route-policy transitions. These are core integration requirements, not assumed capabilities of a raw tunnel.
