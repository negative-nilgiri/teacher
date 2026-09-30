# Block links

A later block often relies on something shown much earlier, such as a struct
definition. Repeating it clutters the lesson; writing "see above" makes the
learner scroll away and lose their place. Since source schema 2.3.0, any
Markdown can link to another block by its ID, whole (`#queue-def`) or by the
lines its gutter shows (`#queue-def:12-18`). Hovering or focusing a link
previews the target in place; clicking jumps to it, and Back returns.

This lesson uses block links itself: from the third section on, links point
back at the code explained earlier. Try hovering them.
