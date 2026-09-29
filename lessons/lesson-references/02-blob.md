## Finding content that is committed later

A worktree file has no commit when the lesson is built. Its **Git blob ID** is
the hash Git will give that exact content whenever it is committed, so the
agent can find it later with `git log --all --find-object=<blob>`, even after a
move or rename. `git_blob_id` in `src/repository/git.rs` computes it in Rust,
over the whole file rather than the displayed lines, so lessons made only of
plain files still never run Git.
