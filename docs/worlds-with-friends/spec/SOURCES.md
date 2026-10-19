# Sources and verification ledger

Reviewed on 2026-10-18. These are primary technical references and proposed artifact origins, not a list of binaries already installed or gameplay-tested. Documentation inspection does not qualify a library release or a game tuple. Store exact dependency pins, artifact URLs, hashes, licenses and retrieval timestamps in implementation evidence.

## Standards and tools

| ID | Primary source | Use and inspection status |
| --- | --- | --- |
| S01 | [OAuth security BCP, RFC 9700](https://www.rfc-editor.org/rfc/rfc9700.html) | Inspected; security baseline for the own-provider integration |
| S02 | [OAuth for native apps, RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html) | Inspected; system-browser and native callback design |
| S03 | [PKCE, RFC 7636](https://www.rfc-editor.org/rfc/rfc7636.html) | Normative reference; exact provider interoperability must be tested |
| S10 | [Sender-constrained OAuth requests, RFC 9449](https://www.rfc-editor.org/rfc/rfc9449.html) | Inspected; possible standard device-control proof mechanism, subject to actual backend support |
| S04 | [iroh API](https://docs.rs/iroh/latest/iroh/) | Inspected; endpoint identity, streams, direct connectivity and relay fallback; moving latest URL must not become a dependency pin |
| S05 | [iroh relay configuration](https://docs.rs/iroh-relay/latest/iroh_relay/server/struct.RelayConfig.html) | Inspected; TLS, access and limits; constructor defaults need explicit production configuration |
| S06 | [iroh repository](https://github.com/n0-computer/iroh) | Upstream implementation; pin a reviewed release/commit and inspect its relay server APIs |
| S07 | [Tauri capabilities](https://v2.tauri.app/security/capabilities/) | Inspected; narrow frontend permissions |
| S08 | [Tauri external binaries](https://v2.tauri.app/develop/sidecar/) | Inspected; packaging reference, not proof of independently surviving process lifetime |
| S09 | [Modrinth API](https://docs.modrinth.com/api/) | Inspected; mod/project/version metadata and source references for P13 |

## Game and loader artifact origins

| Family | Source | Required resolver behavior | Preparation evidence |
| --- | --- | --- | --- |
| Vanilla distribution | [Version catalog](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json) | Select exact version descriptor, then its server/client references and expected hashes; never synthesize version download URLs | Catalog URL is a known source; current browser fetch did not produce inspectable contents in this review; fetch and validate during P01/P06 |
| Fabric | [Meta API](https://meta.fabricmc.net/) and [server installer](https://fabricmc.net/use/server/) | Select exact game, loader and installer versions; derive server launcher from documented metadata; record generated dependencies | Meta route listing inspected, including version-specific server endpoints; no installer executed |
| Forge | [Downloads](https://files.minecraftforge.net/net/minecraftforge/forge/) and [Maven](https://maven.minecraftforge.net/) | Resolve exact installer from official references, preserve installer processors and generated launch requirements | Download catalog inspected; binaries and installer outputs untested |
| NeoForge | [Maven releases](https://maven.neoforged.net/releases/) | Use exact reviewed installer coordinates and generated outputs for the selected game version | Repository URL identified; current browser retrieval incomplete, so P14 must verify actual metadata and artifacts |
| Quilt | [Meta API](https://meta.quiltmc.org/) and [installer repository](https://github.com/QuiltMC/quilt-installer) | Verify current supported game/loader/installer contract rather than copying another loader's URLs | Meta root returned no inspectable schema; P14 must retrieve documentation and test installation |
| Java runtime | [Adoptium API](https://api.adoptium.net/) | Reuse the base launcher's exact platform/runtime resolver and verified archive receipts | Existing integration assumed by contract; current API/runtime builds must be verified in P01/P06 |

Do not bundle executable distributions in this specification. Do not treat catalog availability as evidence that every version supports every OS or the project's identity integration. Loader families are not interchangeable.

## Verification tasks for implementation

For each shipped artifact record source URL, final redirect URL, release/version, size, upstream hash, internal SHA-256, license/distribution policy, executable behavior and supported tuples. Revalidate redirect destination against source policy. Preserve source metadata bytes for reproducibility. If upstream supplies no cryptographic authenticity, document the trust model instead of treating a self-computed hash as authentication.

The actual own OAuth and room service documentation must be added privately to the project integration map by the owner or repository configuration. No production endpoint, credential, account claim or deployment address is invented here.

## Revision 1.0.1 audit verification — 2026-10-19

Rechecked the primary [relay configuration API](https://docs.rs/iroh-relay/latest/iroh_relay/server/struct.RelayConfig.html) and [DPoP standard](https://www.rfc-editor.org/rfc/rfc9449.html). Relay access/limit hooks do not by themselves prove periodic active-session enforcement or product quotas. DPoP and mutually authenticated TLS have distinct validation mechanisms; neither supplies room authorization by itself.

Other entries retain their original evidence and unresolved retrieval tasks. No game launch, binary installation or live account-provider test is claimed. New maintenance, privacy and commit rules are product requirements, not attributed upstream capabilities.

## Revision 1.0.2 supporting references — 2026-10-19

| ID | Primary source | Inspection and use |
| --- | --- | --- |
| S11 | [HTTP semantics, RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html) | Inspected HTTP method, origin, redirect and authentication boundaries. Business-operation deduplication and authorized result replay are explicit product contracts, not a claim of exactly-once HTTP delivery. |
| S12 | [OWASP SSRF prevention guidance](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html) | Inspected destination/redirect validation guidance. The product additionally applies reviewed origin and credential separation to native desktop content fetches. |

These references support boundary design, not a particular installed HTTP library configuration. Current source-artifact pins and platform behavior still require implementation qualification. No new upstream game/loader compatibility claim is made by this audit.
