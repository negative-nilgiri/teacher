## Where to look when reviewing

- **Criteria wording** in `src/learnverify/checks.rs`: it is what the model judges
  against, and the part most likely to need tuning after real runs.
- **`verify.unavailable` rules** in `grade.rs` and `config.rs`: unfilterable,
  never fatal, rejected by `--ignore-code`.
- **Cache safety** in `cache.rs`: directory checks, `0600` entries, TTL.
- **`SKILL.md` step 5**: agents follow it literally.

Tests: 23 unit tests under `src/learnverify/`, and `tests/verify_contract.rs`,
which runs the binary against a local fake TypeSafe server. Not covered: real
API latency (the 10-second timeout is untested against the live service) and
an overall time budget, which was deliberately left out.
