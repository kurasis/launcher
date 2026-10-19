# Initial instruction for the development chatbot

You are implementing Worlds with Friends for an existing cross-platform Minecraft Java launcher using Tauri 2, React, TypeScript, Vite and Rust.

Speak with me in Russian. Keep the repository in English, including code, comments, technical documents, tests, commit messages and pull requests. Product localization may include Russian and English.

Treat the attached specification as the feature contract. Read README.md and START-HERE.md, then follow their reading order. Inspect the actual repository and its instructions before changing anything. Use our own OAuth accounts and existing room services through explicit adapters. Never invent provider endpoints, account claims, game identity integration or completed tests.

Start with P01 only. Produce its concrete integration map, decisions, qualification tuple, repository plan and implementation contracts. Complete all authorized work that does not depend on missing input. Report a precise blocker if required information is unavailable; do not replace real integration with a fake success path.

You are authorized to create branches, commit, push, open PRs and merge your own PRs automatically after required tests and CI pass and repository protections permit. Do not bypass reviews, force-push the default branch or merge unrelated work. This authorization does not itself authorize production deployment, paid infrastructure or public release publication.

Keep PROGRESS.md accurate. For every claim of completion, provide relevant evidence and distinguish fixtures, local tests, live tests and untested behavior. At the end of P01, report in Russian: completed work, changed files, checks and results, commit/PR/merge status, blockers and the next phase. Then stop and wait for my continuation instruction.

For a later continuation request, reload PROGRESS.md and the actual repository state, select the next dependency-satisfied phase and follow the same workflow. Do not restart completed phases without a concrete reason.

Use specification revision 1.0.2, including AUDIT-REPORT-v1.0.2.md, 18-AUTHORITY-AND-MAINTENANCE.md and 19-LAUNCHER-INTEGRATION.md. Do not request an earlier launcher specification archive. Map capabilities to the actual repository and keep changes limited to this feature and its necessary integration dependencies.
