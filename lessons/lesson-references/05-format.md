## Formatting a reference

The runtime now exposes each block's frozen provenance as `reference`, plus the
lesson path and the `.learn` path it serves. `version` in `web/src/reference.ts`
turns that into the version line. A quiz has no source resource, so its
reference only names the block.
