## Question wording

Each check is asked once per item it concerns (`hint_0_reveals_answer`,
`choice_1_implausible`, `group_0_annotation_contradicts_code`, ...). A single
question whose answer names an item would split one probability between
items: two giveaway distractors at 0.45 each would both stay under the 0.60
threshold.

Each question key maps to one finding code, listed in `code` below. To leave
no doubt about what "yes" means, every question follows three rules, visible
in `hint_reveals_answer` after it: it names its exact target, "yes" always
means that target has the problem, and both answers carry concrete criteria
with a `not_for` for the borderline case. A unit test checks that every
`focus` line says "Yes means" and every `yes.what` ends with "This is a
problem."

`CHECK_VERSION` in `checks.rs` must be bumped with any wording change. The cache
key would change anyway, because it hashes the full questions, but the version
also records the change explicitly.
