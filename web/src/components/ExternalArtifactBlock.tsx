import { useState } from "react";
import type { ExternalArtifactNode } from "../types";
import { Markdown } from "./Markdown";

/**
 * A media block: the caption, then the media when its file is available, with
 * the fallback Markdown folded away beneath it. Without the file, or when the
 * browser cannot load it, the fallback is shown in its place, labelled so the
 * learner knows the media itself is not available.
 */
export function ExternalArtifactBlock({ node }: { node: ExternalArtifactNode }) {
  const [failed, setFailed] = useState(false);
  const showMedia = node.available && !failed;
  return (
    <section className="external-block" aria-label={`Media: ${node.source_id}`}>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {showMedia ? (
        <>
          <div className="external-media">
            <Media node={node} onError={() => setFailed(true)} />
          </div>
          <details className="external-text">
            <summary>Text version</summary>
            <Markdown>{node.fallback}</Markdown>
          </details>
        </>
      ) : (
        <div className="external-fallback">
          <p className="external-notice">
            The {node.kind} <code>{node.file}</code> is not available here. This is its text
            description.
          </p>
          <p className="external-alt">{node.alt}</p>
          <Markdown>{node.fallback}</Markdown>
        </div>
      )}
    </section>
  );
}

/** The element for the node's kind. The alt text labels audio and video. */
function Media({ node, onError }: { node: ExternalArtifactNode; onError: () => void }) {
  // The version changes with the file, so a replaced file is not read from cache.
  const version = node.version ? `?v=${encodeURIComponent(node.version)}` : "";
  const src = `/api/v1/artifacts/${node.node_id}/file${version}`;
  switch (node.kind) {
    case "image":
      return <img alt={node.alt} onError={onError} src={src} />;
    case "audio":
      return <audio aria-label={node.alt} controls onError={onError} preload="metadata" src={src} />;
    case "video":
      return <video aria-label={node.alt} controls onError={onError} preload="metadata" src={src} />;
  }
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
