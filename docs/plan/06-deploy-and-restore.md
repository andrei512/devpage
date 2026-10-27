---
id: "06"
blocked-by: ["01", "03"]
boxes: [viewer/server, viewer/deploying, showcase]
---

# Deploy examples, the restore test, and the docs around them

## Build

- `deploy/server/`: the Dockerfile from
  [running a server](../boxes/viewer/server/running-a-server/README.md#the-image),
  `litestream.yml`, and a `compose.yaml` with Azurite and an S3 emulator whose
  image actually pulls anonymously.
- `deploy/bundled/`: the bundled image (`devpage /srv/docs`, comments on a
  volume or `--no-comments`) and its compose file.
- The restore test from
  [the PVC is only a cache](../boxes/viewer/server/constraints/the-pvc-is-only-a-cache.md#check),
  `--ignored`, against Azurite and the S3 emulator: publish and comment, delete
  the data folder, restore with Litestream, find everything.
- Docs: the bundled deploy page and configuration reference; a showcase deck
  slide for server mode.

## Acceptance

- Both images build; both compose files come up; the restore test passes
  against both backends.
