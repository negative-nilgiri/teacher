import type { DefinitionSite } from "./types";

/** Where a piece of highlighted code sits: its block and first line number. */
export interface CodeOrigin {
  nodeId: number;
  /** The gutter number of the text's first line. */
  firstLine: number;
}

/** Token classes whose text is not code: names there never link. */
const NOT_CODE = /hljs-(comment|string|meta|doctag|regexp|char|quote)/;
const TOKEN = /&[#a-zA-Z0-9]+;|[A-Za-z_][A-Za-z0-9_]*/g;

function plainLength(escaped: string): number {
  return escaped.replace(/&[#a-zA-Z0-9]+;/g, "_").length;
}

/**
 * Wrap the names in highlight.js output that have a shown definition, as
 * `<span class="definition-ref" data-definition="name" data-sites="0,2">`.
 * A function only links a call (`name(`), a macro only `name!`, types,
 * values, and commands any use; the definition's own first line never links.
 */
export function markDefinitions(
  html: string,
  definitions: Record<string, DefinitionSite[]>,
  origin: CodeOrigin,
): string {
  const parts = html.split(/(<[^>]+>)/);
  const plain = parts
    .filter((part) => !part.startsWith("<"))
    .join("")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&");
  const stack: boolean[] = [];
  let offset = 0;
  let line = origin.firstLine;
  return parts
    .map((part) => {
      if (part.startsWith("</span")) {
        stack.pop();
        return part;
      }
      if (part.startsWith("<span")) {
        stack.push(NOT_CODE.test(part));
        return part;
      }
      if (part.startsWith("<")) return part;
      const excluded = stack.some(Boolean);
      const segmentStart = offset;
      const segmentLine = line;
      offset += plainLength(part);
      line += (part.match(/\n/g) ?? []).length;
      if (excluded) return part;
      return part.replace(TOKEN, (token, index: number) => {
        if (token.startsWith("&")) return token;
        const sites = definitions[token];
        if (!sites || !Object.hasOwn(definitions, token)) return token;
        const before = part.slice(0, index);
        const at = segmentStart + plainLength(before);
        const tokenLine = segmentLine + (before.match(/\n/g) ?? []).length;
        const next = plain.slice(at + token.length).trimStart();
        const call = next.startsWith("(") || next.startsWith("::<");
        const macro = next.startsWith("!") && !next.startsWith("!=");
        const allowed = sites
          .map((site, siteIndex) => ({ site, siteIndex }))
          .filter(({ site }) => {
            if (site.target === origin.nodeId && site.lines?.start === tokenLine) return false;
            if (macro) return site.kind === "macro";
            if (call) return site.kind === "function" || site.kind === "type";
            return site.kind === "type" || site.kind === "value" || site.kind === "command";
          })
          .map(({ siteIndex }) => siteIndex);
        if (allowed.length === 0) return token;
        return `<span class="definition-ref" data-definition="${token}" data-sites="${allowed.join(",")}">${token}</span>`;
      });
    })
    .join("");
}
