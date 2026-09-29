The worktree part of a Git diff is recorded by the diff resolver itself, in
`src/repository/git.rs`, under the same snapshot guard as the diff: nothing can
change between reading the diff and hashing the files.
