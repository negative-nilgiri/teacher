## Where to look when reviewing

- **Criteria wording** in `src/learnverify/checks.rs`: it is what the model judges
  against, and the part most likely to need tuning after real runs.
- **`verify.unavailable` rules** in `grade.rs` and `config.rs`: unfilterable,
  never fatal, rejected by `--ignore-code`.
- **Cache safety** in `cache.rs`: directory checks, `0600` entries, TTL.
- **`SKILL.md` step 5**: agents follow it literally.

Tests: 23 unit tests under `src/learnverify/`, and `tests/verify_contract.rs`,
which runs the binary against a local fake TypeSafe server. The 20-second
run budget is tested against a local server that never answers. Deliberately
left out: a cap on the number of requests (calls are cheap) and model pinning
(each finding records the resolved `model`).
