# 04 — Game installation, runtime management and download sources

## Source registry

No game file repository needs to be operated by the project. Mojang metadata provides exact version-specific URLs, file sizes and hashes. Adoptium provides runtime package metadata. Query metadata and follow its validated links; do not scrape download pages or build a guessed list of Minecraft JARs.

| Resource | Authoritative source / resolution rule |
| --- | --- |
| Version catalog | `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` |
| Per-version descriptor | Catalog `versions[].url`; validate raw response bytes against `versions[].sha1` |
| Client JAR | Descriptor `downloads.client.url`, `.sha1`, `.size` |
| Java libraries and native JARs | Descriptor `libraries[].downloads.artifact` and, for older descriptors if supported, `downloads.classifiers` |
| Library host | Normally `https://libraries.minecraft.net/`; descriptor URLs remain authoritative |
| Asset index | Descriptor `assetIndex.url`, `.sha1`, `.size` |
| Asset object | `https://resources.download.minecraft.net/<hash[0:2]>/<hash>` from index objects |
| Logging configuration | Descriptor `logging.client.file.url`, `.sha1`, `.size` |
| Java runtime metadata | `https://api.adoptium.net/v3/assets/latest/<major>/hotspot?architecture=<arch>&image_type=jre&os=<os>&vendor=eclipse` |
| Java archive | Adoptium entry `binary.package.link`, `.checksum` (SHA-256), `.size`; not `binary.installer` |

Client and logging downloads commonly use `piston-data.mojang.com`. Content hashes and version IDs in these URLs must come from metadata. The source register contains exact checked examples; they are evidence, not hard-coded latest-version constants.

## Explicit install-operation state machine

`not_installed → resolving → checking_files → downloading → extracting_runtime → finalizing → ready`

Any active phase may become `cancel_requested → cancelled` or `failed`; a backend crash is reconciled as `interrupted`. Operation state is distinct from persistent InstallationHealth (document 06). Repair re-enters `resolving/checking_files` using the exact locked metadata. `ready` is only emitted after all required work succeeds and the lock/receipt is committed. A job can be retried after restart by reconstructing its plan and checking completed files.

At most one installation/repair job is active globally, and none may run while launch/game/cleanup holds the activity permit. Instance version/settings affecting the plan cannot be mutated during an active job. UI cancellation requests are acknowledged separately from completed cancellation.

## Metadata resolution

1. Fetch catalog with a descriptive application User-Agent and reasonable timeouts. Cache ETag/Last-Modified when returned; use bounded cache size and show fetch age. Use catalog cache for browsing during an outage, but never fabricate a release.
2. Resolve the selected exact version ID from the supported compatibility set. Download the descriptor and verify the catalog SHA-1 over the downloaded raw bytes before parsing.
3. Require modern `arguments.jvm` and `arguments.game`, a supported `mainClass`, `downloads.client`, `javaVersion.majorVersion`, `assetIndex` and complete artifact metadata. Missing data is `UNSUPPORTED_VERSION_METADATA`, not an invitation to guess.
4. Preserve the verified descriptor bytes under its hash and freeze them into the installation lock. Tolerate unknown optional fields; reject unknown required execution features or unsafe paths.
5. Evaluate library and argument rules for the target OS and the actual JVM architecture, not the webview user agent.
6. Resolve and validate the asset index and logging configuration. P05 emits a `GamePlan` plus `RuntimeRequirement { major, os, arch }`; P07 resolves that requirement into a pinned `RuntimePlan`. Small bounded metadata requests use the source client and do not depend on the later bulk downloader.
7. P08 combines both into `FinalInstallPlan` before bulk execution. Plans are deterministic for fixed verified inputs and the target; they include relative paths, hashes/sizes and URLs. Until runtime resolution finishes, show runtime transfer/storage totals as unknown instead of calling the P05 estimate complete. The pure planner consumes fetched metadata; it performs no network I/O or process spawning itself.

Only the pinned modern vanilla metadata shape is mandatory. Version inheritance (`inheritsFrom`), legacy `minecraftArguments`, missing-download Maven fallback, legacy virtual assets and `map_to_resources` must be rejected clearly until deliberately implemented and tested.

## Rules engine

Rules are ordered. If there is no `rules` property, the item is allowed. If rules exist, start disallowed and evaluate in order; each matching rule sets the current result to its `action`. The last matching action wins. Match all predicates in a rule conjunctively.

- Map Windows to `windows`, macOS to `osx`, Linux to `linux`.
- Architecture values follow the metadata/JVM convention, not arbitrary Rust names. Normalize host `x86_64`, provider `x64` and JVM `amd64` deliberately in the platform adapter; preserve tested semantics for regex/architecture predicates.
- OS version patterns must be evaluated against the correctly obtained native OS version. Invalid patterns or unknown rule actions are resolver errors.
- Feature booleans: `is_demo_user=false`; `has_custom_resolution=true` when dimensions are supplied; quick-play flags false for MVP. A missing feature is false, so a predicate explicitly requiring false can match.
- Conditional `value` may be a string or an array of strings. Preserve each argument as one argument; never tokenize it through a shell.
- A rule with an unsupported predicate cannot silently broaden applicability. Fail resolution for the supported target until the semantics are implemented.

Test precedence, no-match cases, false features, string/array values and every target OS. Do not replace ordered rules with “any allow rule wins.”

## Libraries and native loading

Modern descriptors can list native JARs as normal `downloads.artifact` entries. Both x64 and arm64 macOS native JARs may be allowed by the same OS rule; their resource paths can be architecture-specific and loaded by the library. **Do not flatten every native-looking JAR into one directory or indiscriminately discard OS-allowed variants.** Preserve official ordered classpath entries and allow the selected libraries' normal resource extraction where that is how the pinned release works.

For descriptors using the legacy `natives`/`classifiers` extraction mechanism, add a separate tested path only if needed by the support set: choose the classifier using the actual JVM architecture, download its declared artifact, and extract to the per-run directory while honoring extraction exclusions. If not implemented, reject that format explicitly. Never mix architectures into a flat extraction directory.

Create writable isolated native work directories for each run. Follow metadata-provided native-directory suffixes: observed newer metadata uses separate subdirectories for Java, JNA, LWJGL and Netty. Include only the exact verified paths in the classpath. Do not glob all cached JARs.

## Asset installation

Validate the index hash and shape. Each logical name resolves to an object with a 40-hex-character SHA-1 and nonnegative size. Download unique hashes once. Store object `abcdef...` at `shared/assets/objects/ab/abcdef...`; preserve the full index JSON in the expected indexes projection.

Assets are addressed by their hashes; logical names must not become arbitrary writable filesystem paths. The actual aggregate transfer size is the deduplicated size of missing objects, not necessarily `assetIndex.totalSize`. Show separate metadata/preparation progress when the total is not yet known.

## Managed Java

Read `javaVersion.majorVersion` from the selected locked descriptor. Research observed Java 21 for 1.21.1 and Java 25 for 26.3. Do not impose Java 21 on every release or choose the newest installed Java just because it is newer.

| Product target | Adoptium `os` | Adoptium `architecture` |
| --- | --- | --- |
| Windows x64 | `windows` | `x64` |
| Linux x64 | `linux` | `x64` |
| macOS Intel | `mac` | `x64` |
| macOS Apple Silicon | `mac` | `aarch64` |

Ask for a GA HotSpot JRE package of the required major and target platform. Validate response filters, release type where supplied, package type, SHA-256 and size. If no JRE exists, a GA JDK archive of the same major is an explicit fallback; record the selection. Never silently switch major or CPU architecture. Resolve once, persist the exact build/package receipt and reuse it. Java patch updates are deferred maintenance, not part of MVP Repair. Reinstalling/repairing an existing instance uses its pinned runtime receipt and archive URL, not a fresh `latest` query. An unavailable pinned package is an actionable failure, never an implicit runtime upgrade.

Use the portable ZIP/tar.gz package, not an MSI/PKG system installer. Download to staging, verify SHA-256, safely extract, retain distribution license notices, then identify `bin/java` (or macOS `Contents/Home/bin/java`, Windows `bin/java.exe`). After archive and extracted-tree verification, execute `-XshowSettings:properties -version` with the sanitized child environment from document 05 and bounded capture/timeouts; check version/architecture. Preserve Unix execute permissions. Use Windows process creation flags to avoid a console popup while still capturing logs.

Generate an extracted-tree receipt from the newly verified archive: normalized relative paths, file types, SHA-256 and sizes for regular files, executable mode where relevant, and validated internal link targets. Exclude the receipt itself from its file list. Store receipt identity in the lock. Repair checks this tree, including Java executable and libraries; an archive checksum alone does not verify extracted files. On corruption, rebuild the pinned runtime in fresh staging and verify it before any Java probe.

Promote the completed runtime within the same data filesystem using a recoverable commit protocol. Replacement of a nonempty directory is not assumed atomic on every OS: retain a last-good directory until the new receipt is committed, reconcile interrupted swaps at startup, and exclude readers with the global activity permit. Never modify global Java, registry Java associations, PATH or JAVA_HOME. A malformed runtime is never marked ready. A custom runtime picker is outside MVP.

Mojang runtime manifests are an alternative future provider, but their discovery/versioning must be verified before use. Do not hard-code a stale hash-bearing `java-runtime/.../all.json` URL copied from a gist. Adoptium is the chosen MVP provider and was checked for both required Java majors across all four target combinations.

## Download executor

- Stream bytes to disk with bounded buffers; never load large JARs/archives into memory.
- Default concurrency eight; use a bounded queue and per-content deduplication.
- Validate HTTPS, expected host/path policy and every redirect. Credentialed requests and public artifact clients are separate so tokens cannot leak to CDNs.
- Use connection timeout around 15 seconds, a no-progress read timeout around 60 seconds, and cancellation. Large archive downloads must not share a tiny absolute request timeout.
- Retry transient network failures, selected `5xx` and `429` up to three times with jittered backoff and `Retry-After`; do not retry malformed metadata or permanent `404` indefinitely.
- Required MVP recovery is file-level: preserve verified completed files; incomplete `.part` files may restart from zero. Byte-range resume is optional and must correctly validate status `206`, Content-Range and ETag/If-Range before appending.
- Compute expected digest while streaming and compare final size/hash before commit. For a mismatch delete/quarantine the partial file, retry once from scratch, then report `HASH_MISMATCH`.
- SHA-1 is required to match Mojang's provided content identifiers; use SHA-256 for runtime packages and launcher release artifacts. A hash received from the same upstream is integrity metadata, not an independent publisher signature.
- Commit verified content through atomic rename within the same filesystem. Another job must never see a half-written final file.
- Inspect available disk before downloading; include missing compressed data, extraction staging and a reasonable reserve. On disk full, preserve existing valid content and report the required location/space without corrupting the lock.
- Aggregate progress at no more than 10 updates/second. Distinguish downloading bytes from extraction/finalization; do not show 100% installation before the commit.

## Safe archive and path handling

Reject absolute paths, `..` traversal, drive prefixes, alternate data-stream syntax on Windows, NUL bytes, unsafe device paths and paths outside the intended staging root. Bound entry count, individual size and total expanded size to prevent archive bombs. Check duplicate normalized paths and case-folding collisions on case-insensitive filesystems.

Java archives may legitimately use internal symlinks. Either support them with normalized target validation strictly inside the runtime root, or reject the package with a clear error; do not follow escaping symlinks. Resolve hard links only to validated internal entries. Never execute scripts from an archive. Filesystem operations must not follow a pre-existing link outside the owned staging area.

## Verify and repair

Every downloaded artifact is fully hashed before commit; immutable cache entries are reused under the verification policy below. Before ordinary launch, check the lock and required file existence/size; rehash files whose local verification record or modification metadata changed. This is a performance optimization against accidental corruption, not a defense against a hostile same-user filesystem. Explicit Repair fully checks locked artifacts, original metadata bytes and runtime-tree entries. It replaces invalid artifacts only; an invalid extracted runtime is rebuilt as a unit from the pinned archive. Worlds and game-owned data are excluded. Missing metadata is fetched by its locked URL/hash, not silently substituted from the current catalog.

Cache-only browsing is allowed; downloading or launching still reports unavailable service dependencies. Cancelled installs remain resumable and never appear ready. Do not redownload entire assets or runtimes on every launch.

Primary references: [live Mojang catalog](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json), [Adoptium API implementation](https://github.com/adoptium/api.adoptium.net), [Adoptium API reference](https://api.adoptium.net/q/swagger-ui/), and exact checked data in [09](09-SOURCES-AND-VERIFICATION.md).

## Cancellation and resource bounds

Cancellation is cooperative through download, hash and extraction workers. Stop scheduling new work, wait for existing writers to stop, remove only uncommitted staging and record a terminal result before releasing the permit. Commit is a short non-cancellable boundary: a cancellation racing a completed commit reports its actual success, never a contradictory cancelled-but-ready state. A cancelled Repair cannot certify a damaged installation as ready; a previous receipt is historical evidence only and launch must revalidate it.

Bound metadata response sizes, JSON nesting/item counts, redirect count and archive expansion. Suggested initial limits: catalog 8 MiB, descriptor 8 MiB, asset index 32 MiB, five redirects; archive entry/expanded-byte limits are separate per approved runtime package and must accommodate measured baseline packages. Treat these as configurable backend policy with tests, never arbitrary frontend knobs. Disk estimates include staging and a reserve; unknown expanded size must remain visible, and extraction enforces its cap even when Content-Length is absent.

Archive extraction must support the safe internal links present in the selected Temurin archives. Rejecting all links is only acceptable if another qualified package for that same major/target is available; it cannot be used to declare a required platform complete without a working runtime.

## Reuse and stale shared health

Resolve required game artifacts to their canonical content paths from document 02; classpaths refer to those exact files. The original Maven coordinate/path remains provenance, not the cache key. Descriptor-provided Maven paths are still validated even when not used as destinations. Preserve official classpath order and native-JAR behavior. Do not deduplicate by basename or replace all versions of a library with the newest one. The two initial descriptors use explicit classpaths; a future format requiring a conventional flat Maven root through `library_directory` is unsupported until a tested adapter exists.

Before computing missing bytes, validate reusable content and deduplicate by algorithm/digest. Reject conflicting sizes for the same digest as `METADATA_CONFLICT`. A different download URL for identical verified bytes does not force a new transfer. Retry/download counters distinguish network-attempt bytes from unique committed bytes; retries cannot increase the installation percentage above 100%.

On discovering shared corruption, invalidate the shared verification receipt and derived health for every referencing instance. Do not rewrite each instance's immutable install lock: those locks still describe the correct expected content. Repair can restore that shared content once for all instances while the global permit excludes game readers. Every launch revalidates shared health even if its own instance summary says ready. See document 10 for exact test cases.

Repair's completed install lock is immutable input. A cancelled or failed repair may leave some artifacts fixed and others unverified; this never upgrades installation health to ready. A future retry checks remaining content against the same lock. Full-cache garbage collection and removing supposedly unused shared files are deferred.

The game-facing asset-index projection is derived mutable state, not a shared immutable artifact health check. Preflight may rebuild it from the selected verified index without Repair; a different prior projection is normal. See document 10.
