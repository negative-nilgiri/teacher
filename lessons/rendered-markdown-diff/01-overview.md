# Rendered Markdown diffs

Agents teaching changes to real docs diff Markdown files, and the line view
shows them as raw source. Diff blocks now render a Markdown file from a Git
source as formatted text, with added blocks in green and removed ones struck
through in red. The source schema is unchanged; artifacts move to 1.4.0 and
the package to 1.14.0.

The block below is a real example: the paragraph this change added to
`docs/AUTHORING.md`, diffed from the last commit before the change to the
worktree. Use the `Rendered | Source` switch in its header to compare the two
views.
