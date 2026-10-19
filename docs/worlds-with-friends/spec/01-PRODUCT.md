# 01 — Product, scope and requirements

## Primary outcome

Two people on different residential networks can enter the same persistent world from a room invitation without editing router settings, exchanging IP addresses or manually assembling matching installations. Leaving the UI must not terminate friends' server access. A later session resumes the same authoritative world.

## Core requirements

| ID | Requirement |
| --- | --- |
| FR01 | Authenticate product users through the existing own OAuth account service; protect credentials outside the webview. |
| FR02 | Persist private rooms independently of live server sessions; list rooms and their observed host state. |
| FR03 | Enforce owner, manager and member permissions on the backend and privileged host commands. |
| FR04 | Issue, preview, redeem, expire and revoke invitations without exposing room secrets. |
| FR05 | Register and revoke cryptographically identified devices bound to product accounts. |
| FR06 | Resolve, install, verify, start and gracefully stop an exact dedicated server build. |
| FR07 | Forward client TCP connections over authenticated encrypted agent streams with bounded resources. |
| FR08 | Run an access-controlled relay service, with direct-path optimization and enforced relay-only privacy policy. |
| FR09 | Admit only permitted room members and bind room sessions to the project's game identity integration. |
| FR10 | Create isolated, reproducible room client profiles and launch into the correct ready session. |
| FR11 | Keep host and tunnel processes alive independently of GUI lifecycle; recover actual state after reopening. |
| FR12 | Import, snapshot, verify, restore and export worlds without concurrent writers or silent data loss. |
| FR13 | Reuse immutable artifacts safely across room versions; repair only managed content. |
| FR14 | Restrict local IPC, remote commands, artifact origins and archive extraction; redact secrets. |
| FR15 | Recover jobs, crashes and network outages without fabricating continuity or duplicating active worlds. |
| FR16 | Package and qualify Windows x64, macOS x64/arm64 and Linux x64 targets. |
| FR17 | Measure connection outcomes, resource consumption and relay traffic without collecting game contents. |
| FR18 | Enforce membership/device/session revocation with bounded stale authorization. |
| FR19 | Provide accessible Russian/English room, join, hosting, backup and recovery interfaces. |
| FR20 | Run one local room server and one local game client concurrently with explicit resource reservations. |

## Expansion requirements

| ID | Requirement | Earliest delivery |
| --- | --- | --- |
| FR21 | Distribute immutable client/server pack generations and perform reversible tested updates. | P13 |
| FR22 | Qualify loader-specific installers and game identity integration for selected Fabric, NeoForge, Forge and Quilt tuples. | P13–P14 |
| FR23 | Enroll a separate home host and authorize remote start/stop within owner-defined limits. | P15 |
| FR24 | Transfer hosting using a verified stopped-world snapshot and a fenced authority transition. | P16 |
| FR25 | Run the same room on a cloud host with explicit cost ceilings, storage policy and deletion controls. | P17 |
| FR26 | Offer optional integrated-world sharing and voice with capability-specific clients. | P18 |

## Explicit boundaries

Core: private invitations, one authoritative world per room, up to eight connected players including the host as the initial qualification target, one hosted room per desktop agent, one game client per desktop agent. Limits are configurable server-side, but increasing them requires capacity tests. A host may remain outside the game while friends play.

Core does not promise public server discovery, public TCP ingress, unrestricted arbitrary proxies, every historical game version, zero downtime on relay failure, automatic cloud failover, portable merging of divergent worlds, or unattended execution of content supplied by strangers. Voice and integrated-world controls are separately gated capabilities.

Core room metadata is persistent in the backend; world bytes remain on the designated host. The product must not imply that a room listing is a cloud backup. An unavailable or sleeping host cannot serve its world merely because the room still exists.

## Success measures

Track first successful join, join failure category, time from prepared invitation to connection, selected route, disconnect frequency, restore success, session corruption reports and relay bytes. Use the qualification targets in [14](14-TESTING.md), not invented guaranteed latency or cost claims. Tests must include five-person play and an eight-player controlled capacity case on recorded hardware.
