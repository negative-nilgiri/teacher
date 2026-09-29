# Diffs that are mostly additions

A diff hunk with one deleted line and thirty added ones is really new code.
Shown as a diff, it reads as a wall of `+` lines and loses what a code block
offers: highlights with annotations and real source line numbers. The new lint
rule `lint.diff.mostly_additions` reports such hunks as a warning, so the
agent keeps the diff for the lines that change and shows the rest as code.
