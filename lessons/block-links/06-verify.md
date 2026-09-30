## What learnverify adds

`learnc check` knows a range exists, not that it still shows the right code.
Each block with links gets its own request asking, per link, whether the
target fails to show what the link text says (`verify.reference_mismatch`).
Link targets also join quiz and highlight requests through
`add_linked_targets` below, so a question is judged with the definitions it
relies on in view.
