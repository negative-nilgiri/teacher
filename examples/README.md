# Runnable examples

## Inline lesson

The inline example has no repository dependencies and can be compiled directly:

```sh
learnc check examples/inline-lesson.json
learnc build examples/inline-lesson.json
learn serve examples/inline-lesson.learn
```

## Repository-backed lesson

[`repository-lesson.json`](repository-lesson.json) demonstrates highlighted
worktree and Git-blob code, patch files, a file-backed multiple-choice prompt,
and a generated worktree diff. Those sources require a specific commit and a
deliberate uncommitted change, so the companion script creates that state
instead of assuming it already exists in this checkout.

From a source checkout, run the whole example with:

```sh
just repository-example
```

This creates a temporary Git repository, checks and builds the lesson, and
serves it. The repository is removed when the server exits.

To inspect or operate on the fixture manually:

```sh
repository=$(examples/create-repository-lesson.sh)
learnc check --root "$repository" "$repository/lesson.json"
learnc build --root "$repository" "$repository/lesson.json"
learn serve "$repository/lesson.learn"
```

The setup script prints the generated repository path. Here that repository is
also the selected filesystem root; a larger lesson may instead select a parent
directory containing several repositories. The script leaves its temporary
directory in place. Remove it after inspection.
