## Migration

The learnpick module, client, and binary are deleted; the client lives on in
`client.rs` with the same environment handling. The justfile, the install and
help tests, the isolation test, and `scripts/package-smoke.sh` now cover
`learnverify`. The smoke probe below is the one behavior that flipped: learnpick
failed without an API key, while learnverify succeeds and reports the skip.
