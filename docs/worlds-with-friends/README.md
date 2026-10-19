# Worlds with Friends

Private worlds hosted by a player's own launcher: the launcher installs and runs a Minecraft
server for the world, friends join from an invitation through an encrypted tunnel, directly
or through our relay, with no router setup. Decision: [D-068](../DECISIONS.md#d-068--worlds-with-friends).

| Document                         | What it holds                                                                     |
| -------------------------------- | --------------------------------------------------------------------------------- |
| [ADAPTATION.md](ADAPTATION.md)   | How this launcher implements the feature; **wins over `spec/` where they differ** |
| [INTEGRATION.md](INTEGRATION.md) | Every adapter mapped to existing code, with the change and the phase              |
| [PLAN.md](PLAN.md)               | Phases W1–W12 with dependencies and exit criteria                                 |
| [PROGRESS.md](PROGRESS.md)       | Current state, evidence and outside inputs                                        |
| [spec/](spec/README.md)          | The specification 1.0.2 as delivered by the owner, unchanged                      |

The phase IDs W1–W12 belong to this feature only. P01–P14 in `docs/spec/` are the base
launcher's phases; P01–P18 inside `spec/` are the delivered specification's own numbering and
are cited only to trace where a requirement came from.
