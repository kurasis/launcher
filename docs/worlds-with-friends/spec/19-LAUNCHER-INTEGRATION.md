# 19 — Contract with the existing launcher

This feature specification is self-contained. The previous launcher specification is not required and is not included. The actual repository and its working account/game runtime integrations are the implementation inputs. Do not rebuild unrelated launcher functionality just to match an assumed old architecture.

## Required capability adapters

| Adapter | Required input/output | Boundary |
| --- | --- | --- |
| AccountSession | Current own account, valid scoped API credentials, refresh/revoke events | Secrets stay in native code/vault |
| GameIdentityAdapter | Stable profile mapping plus trusted pre-play verification tied to a real connection | No client-asserted nickname trust |
| ClientCatalog | Qualified game versions and exact metadata references | A displayed version is not automatically a supported room tuple |
| ClientInstall | Immutable platform-neutral pack reference plus target -> verified target-specific install lock | Preserve user instances/settings |
| RuntimeProvider | Exact Java requirement/target -> verified runtime receipt | Do not silently install latest on retry |
| ClientLaunch | Prepared profile, opaque session handle, local listener address -> supervised run receipt | Native argument compiler, cancellation boundary and no secret webview DTO |
| ArtifactStore | Verified objects, pin/unpin, staged publication and integrity checks | No live in-place repair or unsafe cross-process cache writes |
| ProcessSupervisor | Spawn intent, actual process identity, stop and exit outcome | Independent of webview lifetime |
| UI shell | Typed commands/events, localization, single UI instance and open/deep-link routing | UI is disposable and cannot grant permissions |
| Package/update system | Agent packaging, protocol compatibility, supported OS dependencies and drain/update flow | Do not replace a live incompatible agent |

## P01 mapping deliverable

For every adapter record existing implementation path, owner, behavior, gap, minimal change, tests and whether the gap blocks a later phase. Reuse qualified facilities. If the launcher already has a suitable daemon, extend it instead of adding a second storage writer. If it has no global lock of the kind discussed elsewhere, do not introduce or remove one merely because a historical design used it.

Support one host server and one local game client with consistent resource accounting. Migrate only storage/process ownership that is actually shared. Different roots may remain separate with explicit object-copy or read-only sharing contracts; no dual writers to one mutable root.

## First core tuple

P01 selects a candidate from versions the launcher can actually prepare. P06 records exact server/client/runtime/platform pins. P07 proves room admission and the trusted pre-play identity mapping. If a minimal identity bridge/bootstrap is required, implement that dependency in core and label the runtime flavor. General modpack distribution and additional loader coverage stay outside core.

A missing existing capability is a named integration task, not a reason to import an earlier unrelated specification or invent successful tests. Missing real provider/configuration inputs block only dependent live acceptance; continue useful adapter and fixture work honestly.

Additional adapter obligations: GameIdentityAdapter exposes trusted release completion and a stable persistent profile namespace; ServerAdapter supplies isolated first-world generation and direct-file sink policy; ProcessSupervisor fences cancellation/spawn and recovery workers; ArtifactStore uses the reviewed native outbound-fetch policy. Map these to actual code rather than assuming they exist.
