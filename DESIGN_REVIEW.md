# V1 design specification

This is the authoritative implementation specification for v1 of the interactive
lesson compiler and runtime. It records the accepted design; superseded options
and exploratory alternatives have been removed.

## Accepted foundation

- The product is an agent-independent runtime for declarative interactive lesson
  documents.
- Agents own understanding, pedagogy, lesson sequencing, and question writing.
- The system owns validation, safe repository access, rendering, and interaction
  state. `learnc` owns source and repository access; `learn` owns serving and
  learner state. After v1, `learn` may also own agent/LLM access.
- An authored lesson is immutable during a session. If its content needs to
  change, the agent generates a new document.
- Submitted answers, attempts, explicit answer reveals, and results are mutable
  session state rather than lesson mutations. Expanded hints remain local React
  state in v1.

## Accepted deployment architecture

- The standard local workflow uses two Rust binaries from one Cargo package:
  `learnc` compiles lessons and `learn` serves compiled artifacts. `learn` embeds
  the compiled React frontend. Docker, bind mounts, and a separately installed
  JavaScript runtime are not part of the workflow.
- Installation is initially through `cargo install`; the sole initial user is the
  project author. The prebuilt frontend bundle must therefore be included in the
  crate package so installation does not require Node.js.
- The installed `git` executable is an acceptable dependency.
- `learnc` is the only component that reads source repository files and invokes
  Git. `learn` and the browser receive resolved artifact data.
- Repository references are resolved and frozen during `learnc build`.
- Credentials are not used in v1. Future credential lookup belongs behind a
  `learn` runtime boundary so environment variables can later be replaced by a
  daemon/binary protocol without changing lesson documents.

## V1 scope

V1 tests whether an agent can generate a useful interactive explanation and
quiz about its code changes without generating UI code.

V1 block types:

- `markdown`
- `code`
- `diff`
- `multiple_choice`

There is no separate `callout` block in v1; Markdown is sufficient initially.

V1 includes deterministic multiple-choice quizzes. It does not include free
response, self-review, LLM grading, chat, or tutor interaction. LLM-backed
interaction is a possible follow-up after v1.

V1 session state is in memory and is lost when `learn` exits. The compiled
`.learn` artifact is persistent, but learner-session export is not part of v1.

## V1 document envelope

- JSON is the only source format in v1. A custom textual DSL and parser are not
  justified unless actual use exposes a need for them.
- The top-level document contains only `schema_version`, `title`, and the ordered
  `blocks` array.
- V1 has no top-level summary, repository declaration, capability list, author,
  timestamps, tags, theme, or arbitrary metadata. New fields should be added only
  in response to demonstrated use.

```json
{
  "schema_version": "1.1.0",
  "title": "Understanding the queue changes",
  "blocks": []
}
```

## Source IDs and compiled node IDs

- Every authored block has a required, document-unique, human-readable source
  ID. IDs are not limited to interactive blocks because future references turn
  the ordered document into a graph.
- Source IDs are the names agents use for references and the names validation
  errors report.
- The preprocessing/compilation phase builds a symbol table from source IDs to
  generated runtime `NodeId` values, then resolves graph edges to those runtime
  IDs. Runtime code does not need to use authored strings as database, DOM, or
  routing identifiers.
- The compiled representation may retain the source ID for diagnostics and
  exports while using `NodeId` for internal identity.
- Missing and duplicate source IDs remain validation errors; generated IDs
  cannot make an ambiguous source document unambiguous.

The accepted v1 model has two identity layers:

```text
agent-authored SourceId -> compiler symbol table -> dense NodeId
```

- `SourceId` is used while parsing, resolving references, and reporting errors.
- `NodeId` is a generated dense integer used by the compiled graph, runtime, and
  session-scoped browser API.
- The resolved IR may retain `SourceId` as diagnostic metadata.
- The symbol table is conceptually `HashMap<SourceId, NodeId>`; a separately
  materialized hashed identifier adds no safety or function in v1.
- Context-specific encoding and validation—not hashing—make identifiers safe in
  JSON, URLs, HTML, or other sinks.
- If persistence or export later needs an opaque identifier stable across
  compilations, a third `StableNodeKey` may be introduced without changing the
  source DSL. It is deliberately not part of v1.

## Candidate v1.1: explicit interaction context

V1.1 should consider lesson-level, non-rendered context resources containing code
fragments and diffs. An agent could prepare all relevant context when generating
the lesson, and a future grader or tutor interaction could reference selected
resource IDs without rereading the repository or receiving the entire lesson.

These should be modeled as resources rather than hidden display blocks:

```json
{
  "context_resources": [
    {
      "id": "queue-implementation",
      "type": "code",
      "source": {
        "kind": "file",
        "path": "src/queue.rs",
        "lines": { "start": 70, "end": 118 }
      }
    },
    {
      "id": "ordering-change",
      "type": "diff",
      "source": { "kind": "inline", "content": "..." }
    }
  ]
}
```

Future LLM-backed interactions would explicitly list the resource IDs they may
receive. Context resources would:

- use the same resolution, freezing, digest, path, and size rules as visible code
  and diff blocks;
- not appear in the main lesson flow;
- be inspectable by the user through a context/disclosure view rather than being
  secret;
- never grant implicit access to other files or the repository;
- be sent only when an interaction explicitly references them.

This gives an LLM immediate, agent-curated context while keeping data disclosure
bounded and visible to the user.

## V1 content blocks and sources

The accepted non-interactive blocks are `markdown`, `code`, and `diff`. All have
required authored source IDs. Markdown handles rendered prose; ordinary prose
must not be placed in code blocks, where Markdown remains literal. A code block
may use language `markdown` when the syntax itself is the subject, but it should
contain only the relevant fragment. `code` and `diff` remain distinct semantic
blocks.

Presentation controls such as captions, colors, layout, line highlights,
collapsing, and copy-button configuration are excluded from v1. Adjacent Markdown
can introduce or explain another block. Lessons should form local narrative
units by interleaving an explanation, its relevant code or diff, and any
follow-up instead of collecting unrelated diffs at the end.

### Compiled lesson output

The generic `learnc` compiler produces data for the separate `learn` runtime.
Compiling a lesson does not create a new native executable:

```text
lesson JSON + referenced resources + repository state
    -> preprocessing/compilation
    -> resolved CompiledLesson
```

V1 has an explicit compiled artifact and two binaries with distinct trust
boundaries:

```text
learnc check [args] lesson.json
learnc build [args] lesson.json -> lesson.learn
learn serve [args] lesson.learn -> local lesson server
```

- `check` runs parsing, semantic validation, name resolution, source resolution,
  and repository checks, then discards the compiled result. It does not write an
  artifact or start a server.
- `build` performs the same pipeline and atomically writes a versioned,
  self-contained `.learn` data artifact.
- `learnc` owns source parsing, repository/file/Git access, validation, and
  artifact generation. It does not run the lesson application.
- `learn` accepts the artifact, validates its format/integrity, and serves it. It
  does not parse lesson source or reread source files and repositories.
- `learn` contains the local server and compiled React frontend. Those are not
  duplicated inside each `.learn` artifact.
- Both binaries may be installed from one Cargo package/workspace while sharing
  protocol and artifact libraries.
- The artifact contains normalized lesson IR, resolved Markdown/code/diffs,
  private quiz material, source-ID/debug metadata, repository provenance, and
  compiler-computed resource hashes. V1 serializes this data as readable JSON
  with the `.learn` extension.

### Block-specific source unions

The DSL uses block-specific tagged source unions rather than one universal source
type. This prevents nonsensical combinations while allowing internal resolver
types to be shared.

Conceptually:

```rust
enum MarkdownSource {
    Inline { content: String },
    File { path: RepoPath },
}

enum CodeSource {
    Inline { content: String },
    File { path: RepoPath, lines: Option<LineRange> },
    GitBlob {
        revision: GitRevision,
        path: RepoPath,
        lines: Option<LineRange>,
    },
}

enum DiffSource {
    Inline { content: String },
    File { path: RepoPath },
    Git {
        base: GitRevision,
        target: GitDiffTarget,
        files: Vec<GitDiffFile>,
        context_lines: u16,
    },
}
```

Code blocks also have an optional presentation `language`, independent of the
source variant. Canonical values are `rust`, `python`, `javascript`,
`typescript`, `c`, `cpp`, `go`, `java`, `shell`, `json`, `yaml`, `toml`,
`html`, `css`, `xml`, `sql`, `markdown`, `mermaid`, and `text`. Accepted aliases
normalize as follows: `rs` to `rust`; `py` to `python`;
`js`/`jsx`/`mjs`/`cjs` to `javascript`; `ts`/`tsx`/`mts`/`cts` to
`typescript`; `h` to `c`; `c++`/`cxx`/`cc`/`hpp`/`hxx`/`hh` to `cpp`; `golang` to `go`;
`sh`/`bash`/`zsh`/`fish` to `shell`; `jsonc` to `json`; `yml` to `yaml`; `htm`
to `html`; `xsl`/`xslt`/`svg` to `xml`; `scss`/`sass` to `css`; `md`/`mdx` to
`markdown`; `mmd` to `mermaid`; and `txt`/`plain`/`plaintext` to `text`. For
file and Git-blob sources, an omitted language is inferred from a recognized
path extension. Extensionless names such as `Makefile` and `Dockerfile`, inline
sources, and unknown paths fall back to plain text, as do unknown explicit
language values.

This field is introduced by source schema `1.1.0`. The compiler continues to
decode closed `1.0.0` documents without the field and can emit either version's
exact JSON Schema; new language-aware documents use `1.1.0`.

`mermaid` is rendered as a diagram rather than highlighted source. Mermaid uses
the ordinary code block with an inline source and `language: "mermaid"`; fenced
Mermaid inside a Markdown block is not part of the protocol. A lesson uses
`language: "text"` when it intends to display Mermaid syntax literally. The
runtime uses Mermaid's strict security mode and falls back to escaped source
when a diagram cannot be rendered.

- Source paths use a validated, selected-root-relative `RepoPath`; the resolver
  converts them to platform-specific `PathBuf` values.
- Source line ranges are explicitly one-based and inclusive. They are not exposed
  as Rust's half-open, platform-sized `Range<usize>`.
- The compiler resolves file sources and includes their contents in
  `CompiledLesson`; the browser never reads source files.
- The precise Markdown parser crate is an implementation decision. The protocol
  defines the supported Markdown semantics, with raw HTML disabled.

### Git-generated diffs

Agents may invoke Git while understanding a change, and may reference an existing
patch through `DiffSource::File`. The normal repository-backed form is instead a
declarative Git comparison. The agent specifies the base, target, files, optional
before/after line ranges, and context-line count; the compiler translates that
specification into controlled Git operations. Lesson documents never contain
arbitrary commands or Git argument arrays.

Per-file ranges select intersecting changed regions using `before` line numbers
for deleted content or `after` line numbers for additions/modifications. The
compiler includes the selected changes plus the requested context and reports an
error when a range intersects no change.

### Structured compiled diffs

All inline, file, and Git-generated diff sources compile into structured data:

```text
ResolvedDiff
  -> files
     -> hunks
        -> context/addition/deletion lines with old/new line numbers
```

The React frontend renders this representation and does not parse unified diffs.
The compiler validates and parses raw patch sources once. Selected compiled diffs
carry a normalized language inferred from each displayed file path so their
lines receive the same syntax presentation as code blocks. Selected compiled
diffs are teaching/rendering data and are not required to remain applicable by
`git apply`; the original raw patch may be retained internally as provenance.

## Repository resolution and freezing

- Every source path is relative to one runtime-selected filesystem root. The
  root defaults to the compiler's current working directory and can be
  overridden with `--root`; lesson content cannot declare an absolute root.
  The selected root does not need to be a Git repository.
- Symbolic Git revision expressions such as `HEAD`, `main`, and `HEAD~2` are
  allowed in lesson source. The compiler resolves them to concrete object IDs and
  records those IDs in the artifact.
- Source JSON is a build recipe and may resolve differently on a later build.
  The `.learn` artifact is the frozen build result.
- For file sources, the compiler resolves the path, reads and selects content,
  computes a resource hash, and embeds the selected content in the artifact.
  Agents do not provide content hashes in v1.
- Explicitly selected untracked worktree files compile as complete additions.
  Ignored files remain excluded.
- A build either produces one internally consistent snapshot or fails if input
  bytes, referenced revisions, path ownership, or selected repository state
  change during resolution. It never knowingly emits a mixture of repository
  states.
- One Git diff source may cover files from only one owning Git repository. The
  compiler discovers each path's nearest owning repository and runs Git from
  that repository root. One lesson may use unrelated sibling repositories,
  nested repositories, or submodules beneath its selected root, but paths with
  different owners require separate diff blocks. `learnc check` reports the
  repository groups and tells the agent how to split an invalid selection.

Content hashes are provenance/integrity information, not node identity. Authored
graph references always use `SourceId`; compilation resolves those references to
artifact-local `NodeId` values. Source documents never refer to block numbers or
compiled node numbers.

## V1 quiz design

V1 has only one interactive block: `multiple_choice`. The earlier
`free_response` proposal is postponed rather than shipping a weak self-review
interaction. V1 therefore has exactly four block types:

- `markdown`
- `code`
- `diff`
- `multiple_choice`

Multiple-choice prompts, choices, explanations, and hints use Markdown. There
must be at least two choices and exactly one choice marked `correct: true`;
omitting `correct` means false. Choices are not graph nodes and do not have
agent-authored IDs. The compiler generates artifact-local `ChoiceId` values,
removes the source `correct` markers from presentation data, and records the
correct generated ID in the artifact's private answer table.

Choice order is fixed in v1. An incorrect attempt does not reveal the answer;
the learner may request hints or retry. The correct answer and authored
explanation appear only after a correct attempt or an explicit “Reveal answer”
action. `learn` performs grading rather than the browser.

V1 reports factual state—questions completed, deterministic results, and revealed
answers—but computes no aggregate lesson score.

## Future interaction seam

Free response, LLM grading, and agent discussion are not v1 requirements. If
interaction is later justified, the existing architecture supports it as session
events rather than lesson mutation:

```text
browser message -> learn runtime -> configured agent/LLM -> streamed response
```

The React renderer may display a streamed response by adding a generated block to
the session projection. This does not modify the authored or compiled lesson; it
is a transient or persisted session event attached to an authored interaction.
Future interactions can explicitly reference the non-rendered context resources
proposed for v1.1. Narrow operations such as one-shot feedback or a generated
follow-up should be considered before committing to unrestricted chat.

## V1 compiled artifact

`lesson.learn` is a versioned, self-contained compiled data artifact. Its v1
encoding is readable JSON despite the custom extension. This keeps development,
diagnostics, Serde integration, and compatibility testing simple; compression or
a container format may be introduced later if actual artifact sizes justify it.

The artifact contains:

- an `artifact_version` independent of the source `schema_version`;
- the lesson title and ordered compiled nodes;
- Markdown retained as resolved Markdown text;
- code retained as resolved text plus its normalized or inferred language;
- diffs lowered into structured file/hunk/line data;
- a presentation section, including authored hints, separated from server-owned
  quiz answers and explanations;
- artifact-local `NodeId`/`ChoiceId` values plus retained source IDs for
  diagnostics;
- compiler version, selected-root-relative paths, owning-repository provenance,
  resolved Git object IDs, and compiler-generated resource hashes;
- no absolute source paths and no copy of the server or frontend assets.

The public/private split is a runtime presentation boundary, not an anti-cheating
claim: local artifacts are inspectable. A later artifact version may extend the
server-owned section with explicit agent context resources without changing this
principle.

`learnc check` only accepts lesson source such as `lesson.json`. Like
`cargo check`, it runs the source frontend, validation, name resolution, file/Git
resolution, and consistency checks, then stops without serializing or reading a
`.learn` artifact. There is no separate artifact-check command. Compiler/runtime
artifact compatibility is covered by the test suite; `learn` still performs
structural deserialization and version gating when loading an artifact.

## V1 runtime and browser state

`learn serve lesson.learn` loads one artifact and creates one in-memory learner
session. Multiple tabs may observe that same session. Exiting `learn` destroys
the session; multi-user sessions, accounts, and persistence are outside v1.

The governing state rule is:

> Whenever truth must be shared between components or survive a browser refresh,
> `learn` owns it. React owns only unsubmitted drafts and presentational state.

Accordingly, `learn` owns submitted attempts, deterministic results, explicit
answer reveals, and completion state. React owns the currently selected but
unsubmitted choice, expanded hints, focus/scroll state, and loading indicators.

The v1 HTTP API is deliberately small:

```text
GET  /api/v1/state
POST /api/v1/questions/{node_id}/submit
POST /api/v1/questions/{node_id}/reveal
```

- `state` returns the public lesson projection and current recorded progress in
  one response; there are no separate lesson/session bootstrap endpoints.
- `submit` accepts a generated choice ID, records the attempt, and reveals the
  answer/explanation only on success.
- `reveal` records the explicit reveal and returns the answer/explanation.
- Hints are included in public presentation data and expanded locally by React;
  their visibility does not need to survive refresh.
- There is no generic node-action RPC. New shared operations receive explicit,
  typed endpoints when they are introduced.

The browser never receives the private answer table as part of initial state.
This is a clean behavioral boundary, not protection from a user inspecting their
local artifact.

## V1 command-line interface

```text
learnc check [OPTIONS] lesson.json
learnc build [OPTIONS] lesson.json [-o|--output lesson.learn]
learnc schema [--version 1.1.0]
learn serve [OPTIONS] lesson.learn
```

- `learnc schema` emits the exact source JSON Schema for agent discovery.
- If no artifact output is specified, `name.json` builds to `name.learn`.
- Builds write artifacts atomically.
- `learn serve` binds to a random loopback port, prints its startup result, and
  opens a browser only when `--open` is explicitly passed.
- Stdin, automatic repair, example, watch, and combined build-and-serve commands
  are excluded until actual use demonstrates a need.

The CLI is agent-centric: structured JSON is the default for success and failure
output. `--text`/`-t` switches to human-readable output. The name `--output`
remains reserved for the artifact path. JSON output goes to stdout, operational
logs go to stderr, and process exit status independently reports success or
failure. Diagnostics contain stable codes, JSON Pointers, messages, related
locations, and safe suggestions; independent errors are collected when possible.
Malformed source is never silently repaired.

## V1 trust model

V1 is a trusted local prototype used by one user, their coding agent, and their
own repository. `.learn` artifacts are compiler outputs, not an untrusted exchange
format. Security machinery must remain proportional to that scope.

The v1 baseline is therefore limited to:

- `learn` binds to loopback only;
- `learn` checks the artifact version and structurally deserializes the artifact;
- source paths remain selected-root-relative as a language/consistency invariant;
- declarative Git sources are executed through direct process arguments, never an
  authored shell command;
- code and diff content are rendered without executing authored code; known code
  languages receive syntax presentation and `mermaid` is rendered as a diagram;
- raw HTML in Markdown is disabled, primarily to keep rendering deterministic and
  prevent the DSL from acquiring an escape hatch into arbitrary UI.

V1 does not require launch tokens, CSRF machinery, adversarial artifact resource
limits, hostile symlink protection, encrypted private sections, or a comprehensive
web threat model. These must be revisited before `learn` holds credentials,
connects to an agent/LLM daemon, accepts third-party artifacts, listens beyond
loopback, or serves multiple users.

## Versioning and compatibility

Three versions evolve independently and use full SemVer `major.minor.patch`:

- the Cargo package release containing `learnc` and `learn`;
- `schema_version` in authored lesson JSON;
- `artifact_version` in compiled `.learn` JSON.

For example, package `0.4.0` may accept source schema `1.1.0` and emit artifact
schema `1.2.0`. New compilers retain explicit decoders for supported older source
versions; unsupported newer versions are rejected rather than guessed. Artifacts
are disposable build products, so an incompatible artifact is rebuilt from its
source instead of migrated in place.

Both binaries ship together from one Cargo package/workspace and share source and
artifact schema libraries. Compatibility is enforced with valid/invalid source
fixtures, JSON Schema tests, compiler-to-runtime loading tests, important golden
artifact projections, Git integration tests (including worktrees, untracked
files, revisions, submodules, and unrelated sibling repositories), and
public/private quiz projection tests.
