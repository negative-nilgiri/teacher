import { useEffect, useId, useMemo, useState } from "react";

let initialized = false;
let nextDiagramId = 0;
let renderQueue = Promise.resolve();

interface LegendEntry {
  className: string;
  fill?: string;
  stroke?: string;
  color?: string;
}

function splitStyleDeclarations(source: string): string[] {
  const declarations: string[] = [];
  let current = "";
  let depth = 0;
  let quote: '"' | "'" | null = null;
  let escaped = false;

  for (const character of source) {
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }
    if (character === "\\") {
      current += character;
      escaped = true;
      continue;
    }
    if (quote) {
      current += character;
      if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'") {
      current += character;
      quote = character;
      continue;
    }
    if (character === "(") depth += 1;
    if (character === ")" && depth > 0) depth -= 1;
    if (character === "," && depth === 0) {
      declarations.push(current);
      current = "";
      continue;
    }
    current += character;
  }
  if (current) declarations.push(current);
  return declarations;
}

function parseLegend(source: string): LegendEntry[] {
  const entries = new Map<string, LegendEntry>();
  const definitions =
    /(?:^|[;\r\n])\s*classDef\s+([A-Za-z0-9_-]+(?:\s*,\s*[A-Za-z0-9_-]+)*)\s+([^;\r\n]+)/g;

  for (const match of source.matchAll(definitions)) {
    const style: Pick<LegendEntry, "fill" | "stroke" | "color"> = {};
    for (const declaration of splitStyleDeclarations(match[2])) {
      const separator = declaration.indexOf(":");
      if (separator < 0) continue;
      const property = declaration.slice(0, separator).trim().toLocaleLowerCase();
      if (property !== "fill" && property !== "stroke" && property !== "color") continue;
      const value = declaration
        .slice(separator + 1)
        .trim()
        .replace(/\s*!important\s*$/i, "");
      if (value) style[property] = value;
    }

    if (!style.fill && !style.stroke && !style.color) continue;
    for (const rawClassName of match[1].split(",")) {
      const className = rawClassName.trim();
      if (!className || className === "default") continue;
      entries.set(className, { ...entries.get(className), ...style, className });
    }
  }

  return [...entries.values()];
}

function legendLabel(className: string): string {
  return className
    .replace(/[_-]+/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .trim();
}

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
  const legend = useMemo(() => parseLegend(source), [source]);

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
    <div className="mermaid-diagram">
      <div
        aria-label={label}
        className="mermaid-canvas"
        role="img"
        // Mermaid produced this SVG under its strict security policy; source is never injected directly.
        dangerouslySetInnerHTML={{ __html: svg }}
      />
      {legend.length > 0 && (
        <aside aria-label="Diagram legend" className="mermaid-legend">
          <strong>Legend</strong>
          <ul>
            {legend.map((entry) => (
              <li key={entry.className}>
                <span
                  className="mermaid-legend-item"
                  style={{
                    backgroundColor: entry.fill,
                    borderColor: entry.stroke,
                    color: entry.color,
                  }}
                  title={`Mermaid class: ${entry.className}`}
                >
                  {legendLabel(entry.className)}
                </span>
              </li>
            ))}
          </ul>
        </aside>
      )}
    </div>
  );
}
