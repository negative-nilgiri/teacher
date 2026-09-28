## Grading and `verify.unavailable`

`severity` in `src/learnverify/grade.rs` maps the model's probability of a
problem to a lint severity. `verify.annotation_contradicts_code` reaches
`warning` earlier, because a wrong annotation teaches something false.

When blocks could not be checked, one `verify.unavailable` finding names the
reasons and lists each unchecked block as a related location. It is a status,
not a check: `from_findings` never filters it and never makes it fatal, and the
config rejects it in `--ignore-code` and the ignore list. Silence must never look like "no problems".
