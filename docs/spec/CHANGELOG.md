# Specification changelog

## 1.2.1 — 2026-10-17

Second review focused on actual multi-version/cache and interruption scenarios. Changes: canonical client/library content paths; runtime keys including image type and package digest; DATA_ROOT lifetime lock; authoritative install-lock commit and retry-plan pinning; separate installation health; complete background close/return behavior; classpath-root validation; log-sink failure isolation; explicit IPC mutation envelope and cancellation boundaries. Added document 10 with MV01–MV12 checks and SECOND-AUDIT.md with evidence limits.

These changes refine the MVP without adding arbitrary legacy-version support, mods, concurrent games or a new backend service. An implementation using the old layout must import verified data safely rather than discard it.

## 1.2.0 — 2026-10-17

This is a corrected specification release based on a full review of the 27 Markdown files in version 1.1. It contains no launcher implementation and does not mark any application acceptance check as passed.

- Removed the conflicting P04/G0/P09 exception. Positive own-client authentication gates integration; the complete live account matrix gates P13/G5.
- Separated GamePlan, RuntimeRequirement, RuntimePlan and FinalInstallPlan across P05/P07/P08. Estimates no longer pretend to include an unresolved Java package.
- Distinguished app-owned JSON schemas from immutable upstream bytes. Added draft-instance hash nullability and recoverable Windows-safe commit requirements.
- Serialized shared installation/repair and game activity globally, including orphan recovery. Added same-ID/different-hash asset-index projection rules.
- Specified extracted-runtime integrity receipts, repair before execution, and exact pinned metadata/runtime restoration without an implicit upgrade.
- Clarified OAuth callback, redirect, account-generation and vault-deletion behavior. Unknown 403 responses no longer automatically mean an unapproved app.
- Removed inherited JVM option injection, specified non-secret argfiles and actual Java/locale encoding tests, and resolved premature XUID compatibility claims.
- Defined bounded log retention and post-exit pipe drain; synchronized explicit Quit, keep-open and automatic return behavior.
- Replaced an impossible event-delivery guarantee with durable terminal state, request deduplication and epoch/revision reconciliation.
- Made UI mode, error states and RAM/resolution input bounds explicit.
- Separated candidate qualification packages from final artifacts, narrowed untested Linux claims and preserved all mandatory real-machine/account checks.
- Kept one progress tracker with stable relative links; synchronized every phase and its chatbot prompt.
- Added [AUDIT-REPORT.md](AUDIT-REPORT.md) with findings, review coverage and evidence limits, and extended the source register with primary audit references.

The source URLs and dated public metadata observations from 1.1 remain reference inputs. They are not evidence of a working authenticated launcher. Start new development at P01 using [START-HERE.md](START-HERE.md); when continuing existing code, review the audit findings against that code before resuming the next eligible phase.

## 1.1 — Previous delivered archive

Introduced the P01–P14 phase breakdown, per-phase prompts, traceability and initial implementation tracker. Version 1.2.0 supersedes its specification requirements while preserving the chosen stack and vanilla MVP scope.
