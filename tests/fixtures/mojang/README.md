# Mojang metadata fixtures

- `1.21.1.json` and `26.3.json` are the unmodified version descriptors served by
  `piston-meta.mojang.com` (fetched 2026-10-17). Their SHA-1 values
  (`499680d659b7a0ccb2fa9a809c345c5baf2ace7d`, `11f1aefe473eefba0af62e3286c33b692807b2eb`)
  match the live catalog and the specification's source register. They contain only
  metadata: URLs, sizes and hashes. No game files are included in this repository.
- `asset-index-small.json` is a synthetic asset index used to test deduplication and
  rejection rules; it is not Mojang data.

These files are excluded from Prettier so their bytes (and hashes) never change.
