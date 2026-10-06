import type { ExternalArtifactNode } from "../types";
import { Markdown } from "./Markdown";

/**
 * A media block that only has its text to show: the caption, then the
 * fallback Markdown, labelled so the learner knows the media itself is not
 * available. The media element is not rendered here.
 */
export function ExternalArtifactBlock({ node }: { node: ExternalArtifactNode }) {
  return (
    <section className="external-block" aria-label={`Media: ${node.source_id}`}>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      <div className="external-fallback">
        <p className="external-notice">
          The {node.kind} <code>{node.file}</code> is not available here. This is its text
          description.
        </p>
        <p className="external-alt">{node.alt}</p>
        <Markdown>{node.fallback}</Markdown>
      </div>
    </section>
  );
}

/** What a block link to a media block previews: its alt text and fallback. */
export function ExternalArtifactPreview({ node }: { node: ExternalArtifactNode }) {
  return (
    <div className="external-fallback">
      <p className="external-alt">{node.alt}</p>
      <Markdown>{node.fallback}</Markdown>
    </div>
  );
}
