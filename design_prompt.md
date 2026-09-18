We want to design a small, agent-agnostic system for generating interactive explanations and quizzes about code changes.

Do not implement anything yet.

Your job in this phase is to explore the design, challenge assumptions, identify the right abstractions, and produce a concrete architecture we can review before implementation.

## Problem

Coding agents such as Codex, Claude Code, Gemini CLI, etc. are already good at understanding code and diffs.

Instead of encoding complicated teaching logic into a skill such as `/explain`, we want to give agents a generic capability for generating interactive teaching material.

An agent should be able to inspect a repository or diff, decide how best to explain it, and emit a declarative document describing an interactive lesson.

The lesson may interleave:

- Markdown explanations
- Code snippets
- References to repository files
- Diff excerpts
- Multiple-choice questions
- Free-response questions
- Hints
- Callouts
- Possibly diagrams
- LLM-backed evaluation
- Embedded tutor/chat interactions

The system should then render that document as a local interactive web application.

The agent should be responsible for the intelligence and pedagogy.

The runtime should be responsible for:

- Parsing
- Validation
- Rendering
- State
- LLM interaction
- Secret management
- Persistence where useful

The agent should NOT need to generate HTML, CSS, React, or JavaScript.

## Core mental model

Think of the system as something close to:

```text
agent
  |
  | lesson DSL
  v
lesson runtime / compiler
  |
  | validated internal representation
  v
React renderer
  |
  v
interactive local lesson
```

Potentially:

```text
Codex / Claude / other agent
          |
          | JSON DSL
          v
    lesson runtime
       /       \
      /         \
 React UI      LLM provider
      \         /
       \       /
         user
```

The DSL should be declarative.

The agent describes *what the lesson contains*, not how the UI is implemented.

## Important design goal

Do not design this as a Codex-specific feature.

The protocol should be usable by any process capable of emitting the DSL.

For example:

```text
Codex --------\
Claude --------\
Gemini ---------> lesson CLI/runtime
custom agent ---/
```

The interface to the agent should ideally be something as simple as:

```bash
learn serve lesson.json
```

or:

```bash
learn serve --stdin
```

Potentially with commands such as:

```bash
learn validate lesson.json
learn render lesson.json
learn serve lesson.json
```

Do not assume these exact commands are correct. Evaluate the CLI design.

---

# DSL

We are considering a structured JSON DSL with tagged-union blocks.

Conceptually:

```json
{
  "version": 1,
  "title": "Understanding the graph changes",
  "blocks": [
    {
      "type": "markdown",
      "content": "..."
    },
    {
      "type": "code",
      "language": "rust",
      "code": "..."
    },
    {
      "type": "multiple_choice",
      "id": "q1",
      "prompt": "...",
      "choices": [
        {
          "id": "a",
          "content": "..."
        }
      ],
      "answer": "a",
      "explanation": "..."
    }
  ]
}
```

The actual schema should be designed rather than blindly copying this example.

Things worth considering:

### Static blocks

Potential examples:

- `markdown`
- `code`
- `diff`
- `callout`
- `heading`
- `diagram`

Avoid introducing separate block types when Markdown can already represent something cleanly.

### Interactive blocks

Potential examples:

- `multiple_choice`
- `free_response`
- `ordering`
- `predict_output`
- `reveal`
- `checkpoint`

Again, do not assume all of these belong in v1.

### Repository-aware blocks

It may be better for an agent to reference code rather than copying it.

For example:

```json
{
  "type": "code_ref",
  "path": "src/graph.rs",
  "lines": {
    "start": 81,
    "end": 103
  }
}
```

Potentially also references to:

- Git diff hunks
- A commit
- A branch comparison
- Symbols
- Repository files

Think carefully about whether references should be resolved when the lesson is generated or when it is rendered.

Consider reproducibility: what happens if the repository changes after the lesson is generated?

---

# LLM-backed blocks

Some blocks may require semantic interpretation rather than deterministic grading.

Example:

```json
{
  "type": "free_response",
  "id": "q3",
  "prompt": "Why is Acquire needed here?",
  "evaluation": {
    "type": "llm",
    "rubric": [
      "Explains synchronization with previous Release operations",
      "Connects that synchronization to observing earlier writes"
    ]
  }
}
```

There may also be a chat/tutor block:

```json
{
  "type": "chat",
  "id": "memory-ordering-tutor",
  "instructions": "Help the learner reason about this code without immediately giving away the answer.",
  "context": {
    "refs": ["..."]
  }
}
```

The React component should NOT directly own or receive an API key.

The intended architecture is closer to:

```text
browser
   |
   v
local runtime
   |
   v
LLM adapter
   |
   +-- OpenAI
   +-- Anthropic
   +-- local model
   +-- other provider
```

Credentials should be runtime configuration, not part of the lesson DSL.

For example, conceptually:

```bash
learn serve lesson.json --llm some-provider:some-model
```

or via configuration/environment variables.

The lesson might simply declare:

```json
{
  "evaluation": {
    "type": "llm",
    "role": "grader"
  }
}
```

while runtime configuration maps `grader` to a provider/model.

Explore whether named model roles are worthwhile.

For example:

```toml
[roles.tutor]
provider = "..."
model = "..."

[roles.grader]
provider = "..."
model = "..."
```

---

# Dynamic lessons

Consider whether v1 should support a lesson changing dynamically in response to the learner.

Potential model:

```text
lesson
  |
  v
question
  |
  v
user answer
  |
  v
LLM evaluation
  |
  v
new blocks appended to lesson
```

For example, the runtime could ask an LLM to generate:

```json
{
  "append": [
    {
      "type": "callout",
      "kind": "correction",
      "content": "..."
    },
    {
      "type": "free_response",
      "id": "follow-up",
      "prompt": "..."
    }
  ]
}
```

This is appealing, but it may introduce significantly more complexity.

Evaluate whether dynamic document mutation belongs in the initial architecture or should be deliberately postponed.

---

# Compiler/runtime idea

We have been informally calling the lesson builder a "compiler".

Explore whether this is a useful model.

Potential pipeline:

```text
lesson source
    |
    v
parse
    |
    v
validate
    |
    v
normalize
    |
    v
resolve references
    |
    v
internal representation
    |
    v
React renderer
```

Potentially different backends could exist eventually:

```text
lesson DSL
   |
   +--> React SPA
   |
   +--> static HTML
   |
   +--> terminal UI
   |
   +--> some future format
```

Do not optimize prematurely for hypothetical backends, but avoid choices that unnecessarily make the DSL equivalent to React component props.

The DSL should describe semantic concepts, not presentation implementation details.

---

# Technical direction

A likely implementation stack is:

- Rust for CLI/runtime/server
- Serde for schema parsing
- Axum or similar for local HTTP server
- React + TypeScript for the frontend
- Frontend assets embedded into the Rust executable
- JSON as the initial transport/document format

This is not fixed.

Challenge these choices if there is a materially better design.

We care about:

- Small deployment footprint
- One binary if practical
- macOS first, but ideally portable
- Good agent ergonomics
- Easy schema generation
- Easy validation
- Stable versioned protocol
- Minimal dependencies for the user

A desirable eventual UX could be:

```bash
codex ...
# agent produces lesson.json

learn serve lesson.json
```

or the agent might directly run:

```bash
learn serve --stdin <<'EOF'
...
EOF
```

and return a localhost URL.

---

# Security model

Think about this carefully.

Potential concerns include:

- API keys
- LLM requests
- Markdown rendering
- HTML injection
- Arbitrary file references
- Path traversal
- Reading files outside the repository
- Malicious or accidentally dangerous lesson documents
- Arbitrary URLs
- Code snippets
- Dynamic LLM-generated content
- Browser-to-runtime communication
- Running a localhost server
- Whether the server should bind only to loopback
- CSRF / cross-origin behavior if relevant

The DSL should NOT become arbitrary React or arbitrary JavaScript execution.

---

# Agent ergonomics

This is particularly important.

The system exists primarily as a tool that coding agents invoke.

Think about what makes the protocol easy and reliable for an LLM to produce.

Questions to consider:

- JSON vs another format
- Whether IDs should be required
- How verbose schemas should be
- How validation errors should be reported to the agent
- Whether `learn` should automatically repair/normalize minor schema mistakes
- Whether a JSON Schema should be exposed
- Whether we should expose examples to agents
- Whether the agent should query runtime capabilities/version before generating a lesson
- Protocol version negotiation
- How an agent discovers supported block types
- Whether something like this is useful:

```bash
learn schema
learn capabilities
learn example multiple-choice
```

The agent should be able to recover easily from malformed output.

Validation errors should be extremely actionable.

For example:

```text
blocks[4].choices:
expected at least 2 choices, found 1
```

rather than:

```text
invalid document
```

---

# Scope control

Avoid turning this immediately into a giant general-purpose agent UI framework.

The motivating use case is:

> An agent changes some code and then generates an interactive lesson that teaches the human what changed and verifies their understanding.

However, the underlying abstractions should not artificially prevent broader usage later.

We suspect the system could eventually become a generic agent-to-human interactive document protocol, with blocks such as:

```text
markdown
code
diff
question
chat
approval
form
table
diagram
```

But do NOT let that hypothetical future destroy the simplicity of v1.

Identify the smallest coherent system that proves the architecture.

---

# Questions I want you to answer

Produce a design document covering at least the following.

## 1. Reframe the system

Explain what we are actually building in one or two precise paragraphs.

Identify the core abstraction.

## 2. Architecture

Propose the high-level architecture and component boundaries.

Show data flow.

Include a diagram if useful.

## 3. DSL design

Propose a concrete v1 schema.

Do not merely discuss it abstractly.

Show representative JSON examples.

Identify which blocks belong in v1 and which should explicitly wait.

## 4. Type model

Show how you would represent the core schema in Rust.

Also show the corresponding TypeScript type model where relevant.

Prefer tagged/discriminated unions.

## 5. Repository references

Design how code and diff references should work.

Address reproducibility and repository mutation.

## 6. LLM integration

Design the boundary between:

- lesson
- browser
- runtime
- LLM provider

Address credentials.

Explain how deterministic grading and LLM grading differ.

Discuss chat/tutor blocks.

## 7. Runtime/compiler pipeline

Explain parsing, validation, normalization, reference resolution, rendering, and runtime state.

Decide whether "compiler" is an appropriate abstraction.

## 8. CLI

Design the initial CLI.

Show example commands.

Optimize for agents as well as humans.

## 9. Frontend architecture

Explain how DSL blocks map to React components.

Do not implement the frontend.

Identify what state belongs in React versus the runtime.

## 10. Persistence

Decide what needs to persist, if anything.

Possible examples:

- Generated lesson
- Answers
- Conversation history
- Scores
- LLM evaluations

Avoid adding a database unless it buys us something meaningful.

## 11. Security

Threat-model the local system.

Be concrete about what should and should not be allowed.

## 12. Versioning

Propose how the DSL should evolve without breaking existing lessons or agents.

## 13. Dynamic lessons

Evaluate adaptive/dynamic lesson generation.

Decide whether it belongs in v1.

If not, design v1 so adding it later does not require starting over.

## 14. Alternatives and tradeoffs

Identify important architectural alternatives.

For each major decision, explain what we gain and what we give up.

I particularly want you to challenge the current assumptions rather than simply endorse them.

## 15. MVP

Define a very small first implementation.

Specify exactly what is in scope and out of scope.

The MVP should be enough to test the thesis:

> Can an arbitrary coding agent generate a genuinely useful interactive explanation of its own code changes through a stable declarative UI protocol?

## 16. Evolution path

Show what likely comes after the MVP if the experiment works.

Keep this grounded rather than producing a fantasy roadmap.

---

# Design principles

Favor:

- Simple protocols
- Explicit schemas
- Composability
- Agent independence
- Provider independence
- Strong validation
- Stable semantic abstractions
- Local-first operation
- Good CLI ergonomics
- Small trusted runtime
- Declarative documents

Avoid:

- Arbitrary JavaScript
- Agent-generated HTML
- Agent-generated React
- DSL nodes that are merely raw component props
- Provider credentials embedded in documents
- Premature plugin systems
- Premature distributed architecture
- Giant framework abstractions
- Solving every future use case in v1

Be willing to say that an idea should wait.

---

# Output format

Produce a design proposal, not code.

Use concrete schemas, interfaces, diagrams, and CLI examples where they improve precision.

Separate:

1. Decisions you recommend strongly
2. Decisions that remain genuinely open
3. Features that should explicitly be deferred

You should also produce a proposed MVP specification in `SPEC.md`, detailed enough that, after review, we could hand
it back to you and say:

> Implement exactly this.

Do not begin implementation until explicitly asked.

You may use mermaid diagrams with clear colors when it is sound
