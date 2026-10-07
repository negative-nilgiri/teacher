export type NodeId = number;
export type ChoiceId = number;

/** Frozen provenance of a block's content, mirroring the artifact. */
export type ResourceReference =
  | { kind: "inline"; sha256: string }
  | { kind: "file"; path: string; sha256: string; blob_id?: string; head?: string }
  | {
      kind: "git_blob";
      repository: string;
      path: string;
      revision: string;
      revision_object_id: string;
      content_object_id: string;
      sha256: string;
    }
  | {
      kind: "git_diff";
      repository: string;
      base_revision: string;
      base_object_id: string;
      target: { kind: "revision"; revision: string; object_id: string } | { kind: "worktree" };
      files: string[];
      worktree_blob_ids?: Record<string, string>;
      head?: string;
      sha256: string;
    };

interface LessonNodeBase {
  node_id: NodeId;
  source_id: string;
  /** Absent for quizzes and external artifacts, which have no source resource. */
  reference?: ResourceReference;
}

export interface MarkdownNode extends LessonNodeBase {
  type: "markdown";
  content: string;
}

export interface CodeNode extends LessonNodeBase {
  type: "code";
  content: string;
  language: string;
  caption?: string;
  filename?: string;
  /** Source-file line of the first displayed line; absent for inline code. */
  first_line?: number;
  highlights?: CodeHighlight[];
}

export type HighlightColor = "yellow" | "green" | "red" | "blue";

export interface CodeHighlight {
  lines: CodeHighlightRange[];
  color: HighlightColor;
  annotation?: string;
}

export interface CodeHighlightRange {
  start: number;
  end: number;
}

export type DiffLineKind = "context" | "addition" | "deletion";

export interface DiffLine {
  kind: DiffLineKind;
  content: string;
  old_line: number | null;
  new_line: number | null;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

export type RenderedSegment =
  | { kind: "unchanged" | "removed" | "added"; markdown: string }
  | { kind: "gap"; blocks: number };

/** Complete Markdown blocks of a displayed change, prepared by the compiler. */
export interface RenderedMarkdownDiff {
  segments: RenderedSegment[];
}

export interface DiffFile {
  old_path: string | null;
  new_path: string | null;
  language: string;
  hunks: DiffHunk[];
  rendered?: RenderedMarkdownDiff;
}

export interface DiffNode extends LessonNodeBase {
  type: "diff";
  files: DiffFile[];
  caption?: string;
}

export interface MultipleChoice {
  choice_id: ChoiceId;
  content: string;
}

export interface MultipleChoiceNode extends LessonNodeBase {
  type: "multiple_choice";
  prompt: string;
  choices: MultipleChoice[];
  hints: string[];
}

/**
 * Code the learner can run. It carries its own `content`, or `of`, the node
 * of the code block it runs, whose code the block does not repeat.
 */
export interface RunCodeNode extends LessonNodeBase {
  type: "run_code";
  content?: string;
  of?: NodeId;
  language: string;
  caption?: string;
  filename?: string;
  /** Source-file line of the first displayed line; absent for inline code. */
  first_line?: number;
  timeout_secs: number;
  /** Output frozen when the lesson was built. */
  expected_output?: string;
}

export type ExternalArtifactKind = "image" | "audio" | "video";

/**
 * A media file produced outside the compiler. The file is shown when it is
 * `available`; otherwise only its text is: `alt` describes the media and
 * `fallback` is Markdown shown in its place. It has no `reference`, like a
 * quiz.
 */
export interface ExternalArtifactNode extends LessonNodeBase {
  type: "external_artifact";
  kind: ExternalArtifactKind;
  /** A bare file name. */
  file: string;
  alt: string;
  fallback: string;
  caption?: string;
  /** Whether the file is in the sidecar directory right now. */
  available: boolean;
  /** Changes when the file does; sent only when `available`. */
  version?: string;
}

export type LessonNode =
  | MarkdownNode
  | CodeNode
  | DiffNode
  | MultipleChoiceNode
  | RunCodeNode
  | ExternalArtifactNode;

/** Where a block link points: a node and, optionally, displayed lines. */
export interface BlockLink {
  target: NodeId;
  lines?: { start: number; end: number };
}

/** One shown definition of a name; its kind decides which usages link. */
export interface DefinitionSite {
  kind: "function" | "macro" | "type" | "value" | "command";
  target: NodeId;
  lines?: { start: number; end: number };
}

export interface PublicLesson {
  title: string;
  /** The lesson source relative to the filesystem root, when recorded. */
  lesson_path?: string;
  /** The `.learn` file as given to `learn serve`. */
  artifact_path?: string;
  nodes: LessonNode[];
  /** Block links by destination without the `#`, such as `queue-def:12-18`. */
  links?: Record<string, BlockLink>;
  /** Shown definitions by name, for go-to-definition. */
  definitions?: Record<string, DefinitionSite[]>;
}

export interface Attempt {
  choice_id: ChoiceId;
  correct: boolean;
}

export interface ChoiceExplanation {
  choice_id: ChoiceId;
  explanation: string;
}

export interface RevealedAnswer {
  choice_id: ChoiceId;
  explanation: string;
  /** Why individual distractors are wrong; only sent once resolved. */
  choice_explanations?: ChoiceExplanation[];
}

export interface QuestionState {
  attempts: Attempt[];
  completed: boolean;
  revealed: boolean;
  answer?: RevealedAnswer;
}

export interface LessonProgress {
  completed_questions: number;
  total_questions: number;
  questions: Record<string, QuestionState>;
}

/** Whether this launch lets the learner run code (`learn serve --allow-run`). */
export interface RunStatus {
  enabled: boolean;
  /** Sent with every run request; `null` unless running is enabled. */
  token: string | null;
}

/** The outcome of one run. A failing or killed program is a normal result. */
export interface RunResult {
  stdout: string;
  stderr: string;
  /** `null` when the program was killed or never started. */
  exit_code: number | null;
  timed_out: boolean;
  /** Whether either stream was cut at the server's output cap. */
  truncated: boolean;
  duration_ms: number;
  /** Why the program could not run, such as an interpreter that is missing. */
  error?: string;
}

export interface RunResponse {
  run: RunResult;
}

export interface StateResponse {
  lesson: PublicLesson;
  progress: LessonProgress;
  run: RunStatus;
  /** The last result of each run block that has run, by node ID. */
  runs: Record<string, RunResult>;
}

export interface QuestionMutationResponse {
  progress: LessonProgress;
  question: QuestionState;
}

export type MutationResponse = StateResponse | QuestionMutationResponse;
