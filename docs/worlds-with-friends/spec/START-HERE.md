# Start here

## Reading order

Read README, this document, PHASES, PROGRESS, 01, 02, 03, 04, 05, 07, 09, 13, 17, 18, 19, TRACEABILITY and ACCEPTANCE before implementation. Read every phase's referenced documents before starting that phase. The document map in [README](README.md) supplies links.

## Working agreement

- Communicate with the user in Russian. Source code, identifiers, comments, repository documentation, tests, commits, PR titles and descriptions stay in English. Localized product strings may be Russian and English.
- Inspect the actual repository, its instructions, existing architecture, tests and CI first. Protect unrelated changes. No repository is included in this archive.
- Work on one phase at a time. For the initial request, complete P01 and stop with a report; continue later phases when the user requests continuation. A phase may have several small PRs.
- Create a feature branch, implement the current phase, run relevant checks, commit, push, create a PR and merge that bot-owned PR automatically when required CI and repository protections permit. This workflow is already authorized by the project owner.
- Never bypass required reviews, branch protections or CI. Do not force-push the default branch or merge unrelated PRs. If a remote, credentials or required reviewer is unavailable, finish local work and report the concrete blocker.
- Production deployment, purchasing infrastructure, publishing public releases and destructive data migrations are separate actions. Prepare reviewable deployment plans before requesting approval where necessary.
- Do not request credentials in chat. Use the repository's secret mechanism and documented environment contracts.
- Keep the source of truth in PROGRESS and linked evidence. Mark a blocked gate honestly; fixtures do not prove production integration.

## Completion of a phase

Deliver implementation, generated contract definitions where relevant, migrations and rollback/forward-recovery guidance, tests against the phase's concrete risks, and operator/user documentation. Record exact commands, tool versions, OS/architecture, build commit, observations and unresolved limitations. Identify simulated versus live evidence.

Update traceability, accepted decisions, test evidence and PROGRESS in the same phase branch. CI success alone is insufficient when the phase requires real two-device networking or a playable session.

## First implementation task

P01 inventories the base launcher, own OAuth provider, room backend, game identity bridge and deployment targets. It freezes an initial support tuple, architecture contracts and test plan. It does not invent unavailable provider behavior. Continue useful independent work when an input is unavailable, but do not mark P01 complete until its exit criteria are met.

## Change discipline

Keep the document phase IDs local to this feature, under `docs/worlds-with-friends/`; they do not replace phase IDs of the base launcher. Any changed normative default must update its authoritative document, generated configuration, tests and change log. Never quietly expand the release scope to avoid a failing requirement.

## Staged gates

P01 freezes contracts and candidate choices, not completed future spikes. P02 validates the account libraries/provider in a headless test harness; P03 makes the agent their sole production owner. P04 freezes transport versions after its spike. P06 runs the real server in an isolated qualification purpose without public room publication; P07 supplies production admission and verified player binding. These intermediate modes cannot be enabled as a production join bypass.

P12 completion requires the full agreed core platform matrix. Missing hardware may justify a labelled preview, but narrowing the promised release scope requires an explicit owner decision and does not silently pass the original gate.

Use the current 1.0.2 contracts and audit report. In particular, P07 owns the first-world authority transaction needed for its real session; P09 expands it to data operations. Historical audit reports explain previous defects and cannot override current normative documents or acceptance classification.
