## Thresholds

Both thresholds are lint configuration, in `config.example.toml` and as
`learnc lint` flags (`--diff-addition-heavy-min-lines`,
`--diff-max-deletion-ratio`). The defaults report a hunk that adds at least 20
lines while deleting at most one line for every ten added.
