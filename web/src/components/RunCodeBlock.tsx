import { useLinks } from "../links";
import type { CodeNode, RunCodeNode, RunResult } from "../types";
import { CodeBlock } from "./CodeBlock";
import { Markdown } from "./Markdown";
import { ReferenceLink } from "./ReferenceLink";

/** What the app gives a run block: whether it can run, and its last result. */
export interface RunControls {
  /** Whether this launch lets the learner run code. */
  enabled: boolean;
  /** A run of this block is under way. */
  busy: boolean;
  /** The last run of this block, from the server. */
  result?: RunResult;
  onRun: () => void;
}

/**
 * Code that can be run, with the output of its last run and the output it
 * produced when the lesson was built. A block of its own code shows that code;
 * a block of `of` points at the code block it runs instead of repeating the
 * code. Where the output comes from is the server's; this only shows it.
 */
export function RunCodeBlock({ node, run }: { node: RunCodeNode; run?: RunControls }) {
  const { nodes } = useLinks();
  const target = node.of === undefined ? undefined : nodes[node.of];

  return (
    <section className="run-block" aria-label={`Run: ${node.source_id}`}>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {node.content !== undefined ? (
        <CodeBlock node={ownCode(node, node.content)} />
      ) : target ? (
        <p className="run-target">
          Runs{" "}
          <ReferenceLink destination={target.source_id}>
            <code>{target.source_id}</code>
          </ReferenceLink>
        </p>
      ) : null}
      {run?.enabled ? (
        <div className="run-controls">
          <button className="primary" disabled={run.busy} onClick={run.onRun} type="button">
            {run.busy ? "Running…" : run.result ? "Run again" : "Run"}
          </button>
        </div>
      ) : (
        <p className="run-hint">
          Start <code>learn serve --allow-run</code> to run this code.
        </p>
      )}
      {run?.result ? <RunOutput result={run.result} timeoutSecs={node.timeout_secs} /> : null}
      {node.expected_output !== undefined ? (
        <section className="run-output" aria-label="Expected output">
          <header className="source-block-header run-output-header">
            <strong className="run-output-title">Expected output</strong>
            <span className="run-output-note">frozen when the lesson was built</span>
          </header>
          <pre className="run-output-text">
            <code>{node.expected_output}</code>
          </pre>
        </section>
      ) : null}
    </section>
  );
}

function RunOutput({ result, timeoutSecs }: { result: RunResult; timeoutSecs: number }) {
  const silent = result.stdout === "" && result.stderr === "" && !result.error;
  return (
    <section aria-label="Run output" aria-live="polite" className="run-output">
      <header className="source-block-header run-output-header">
        <strong className="run-output-title">Output</strong>
        <span className="run-output-note">
          {result.exit_code === null ? "no exit code" : `exit code ${result.exit_code}`}
          {" · "}
          {formatDuration(result.duration_ms)}
        </span>
      </header>
      {result.error ? (
        <p className="run-notice run-notice-error" role="alert">
          {result.error}
        </p>
      ) : null}
      {result.timed_out ? (
        <p className="run-notice run-notice-error">
          Timed out after {timeoutSecs} s and was stopped.
        </p>
      ) : null}
      {result.truncated ? (
        <p className="run-notice">Output was cut at 64 KiB per stream.</p>
      ) : null}
      {result.stdout !== "" ? <RunStream label="stdout" text={result.stdout} /> : null}
      {result.stderr !== "" ? <RunStream label="stderr" text={result.stderr} /> : null}
      {silent ? <p className="run-notice">The program printed nothing.</p> : null}
    </section>
  );
}

function RunStream({ label, text }: { label: "stdout" | "stderr"; text: string }) {
  return (
    <div className={`run-stream run-stream-${label}`}>
      <span className="run-stream-label">{label}</span>
      <pre aria-label={label} className="run-output-text">
        <code>{text}</code>
      </pre>
    </div>
  );
}

function formatDuration(milliseconds: number): string {
  return milliseconds < 1000 ? `${milliseconds} ms` : `${(milliseconds / 1000).toFixed(1)} s`;
}

/** The block's own code shown as an ordinary code block, without its caption. */
function ownCode(node: RunCodeNode, content: string): CodeNode {
  return {
    content,
    filename: node.filename,
    first_line: node.first_line,
    language: node.language,
    node_id: node.node_id,
    reference: node.reference,
    source_id: node.source_id,
    type: "code",
  };
}
