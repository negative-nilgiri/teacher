## Recording HEAD only where Git already runs

"On top of commit X" is extra context, and reading `HEAD` needs Git. So
`record_heads` in `src/compiler/mod.rs` only asks for the owning repository's
`HEAD` when the lesson also reaches that repository through a Git blob or
Git diff source. Diffs against the worktree record their files' blob IDs and the
repository's `HEAD` directly, since they run Git anyway.
