export type NodeId = number;
export type ChoiceId = number;

interface LessonNodeBase {
  node_id: NodeId;
  source_id: string;
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

export interface DiffFile {
  old_path: string | null;
  new_path: string | null;
  language: string;
  hunks: DiffHunk[];
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

export type LessonNode =
  | MarkdownNode
  | CodeNode
  | DiffNode
  | MultipleChoiceNode;

export interface PublicLesson {
  title: string;
  nodes: LessonNode[];
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

export interface StateResponse {
  lesson: PublicLesson;
  progress: LessonProgress;
}

export interface QuestionMutationResponse {
  progress: LessonProgress;
  question: QuestionState;
}

export type MutationResponse = StateResponse | QuestionMutationResponse;
