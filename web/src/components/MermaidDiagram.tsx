import { useEffect, useId, useState } from "react";

let initialized = false;
let nextDiagramId = 0;
let renderQueue = Promise.resolve();

async function renderMermaid(id: string, source: string): Promise<string> {
  const { default: mermaid } = await import("mermaid");
  if (!initialized) {
    mermaid.initialize({
      flowchart: { htmlLabels: false },
      securityLevel: "strict",
      startOnLoad: false,
      theme: "base",
    });
    initialized = true;
  }
  const render = renderQueue.then(() => mermaid.render(id, source));
  renderQueue = render.then(
    () => undefined,
    () => undefined,
  );
  const { svg } = await render;
  return svg;
}

interface MermaidDiagramProps {
  label: string;
  source: string;
}

export function MermaidDiagram({ label, source }: MermaidDiagramProps) {
  const reactId = useId();
  const [svg, setSvg] = useState<string>();
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    setSvg(undefined);
    setFailed(false);

    const diagramId = `mermaid-${reactId.replace(/[^a-zA-Z0-9_-]/g, "")}-${nextDiagramId++}`;
    void renderMermaid(diagramId, source)
      .then((rendered) => {
        if (active) setSvg(rendered);
      })
      .catch(() => {
        if (active) setFailed(true);
      });

    return () => {
      active = false;
    };
  }, [reactId, source]);

  if (failed) {
    return (
      <div className="mermaid-error" role="status">
        <p>This diagram could not be rendered. Its source is shown below.</p>
        <pre>
          <code>{source}</code>
        </pre>
      </div>
    );
  }

  if (!svg) {
    return <p className="mermaid-loading">Rendering diagram…</p>;
  }

  return (
    <div
      aria-label={label}
      className="mermaid-diagram"
      role="img"
      // Mermaid produced this SVG under its strict security policy; source is never injected directly.
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
