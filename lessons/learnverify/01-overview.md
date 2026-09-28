# What `learnverify` adds

`learnc lint` judges the *shape* of a lesson: sizes, languages, how much is
highlighted. It cannot tell whether a hint gives the answer away or an
annotation says something false about its code. `learnverify` is a separate,
optional binary that asks the TypeSafe Jev model those questions and reports
the answers in lint's diagnostic format.

It replaces `learnpick`, which is gone in package 1.13.0. The whole change
keeps one boundary: the checker depends on the core library, and nothing in
the core depends on the checker. An API that is down, slow, or rejecting the
key can never change what `learnc check`, `learnc build`, `learnc lint`, or
`learn` do.

The diagram below is the path of one run. Read the rest of this lesson in the
same order: shared loading, planning, question wording, highlight state,
requests, cache, grading, and finally the migration.
