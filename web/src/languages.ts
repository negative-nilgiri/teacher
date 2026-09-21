import type { LanguageFn } from "highlight.js";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

interface LanguageDefinition {
  displayName: string;
  highlighter?: LanguageFn;
}

// Keep imports static so Vite can include only the Highlight.js grammars that
// the compiled artifact contract can emit. Mermaid has a semantic renderer and
// plain text deliberately has no grammar.
export const languageRegistry = {
  c: { displayName: "C", highlighter: c },
  cpp: { displayName: "C++", highlighter: cpp },
  css: { displayName: "CSS", highlighter: css },
  go: { displayName: "Go", highlighter: go },
  html: { displayName: "HTML", highlighter: xml },
  java: { displayName: "Java", highlighter: java },
  javascript: { displayName: "JavaScript", highlighter: javascript },
  json: { displayName: "JSON", highlighter: json },
  markdown: { displayName: "Markdown", highlighter: markdown },
  mermaid: { displayName: "Mermaid" },
  python: { displayName: "Python", highlighter: python },
  rust: { displayName: "Rust", highlighter: rust },
  shell: { displayName: "Shell", highlighter: bash },
  sql: { displayName: "SQL", highlighter: sql },
  text: { displayName: "Plain text" },
  toml: { displayName: "TOML", highlighter: ini },
  typescript: { displayName: "TypeScript", highlighter: typescript },
  xml: { displayName: "XML", highlighter: xml },
  yaml: { displayName: "YAML", highlighter: yaml },
} as const satisfies Record<string, LanguageDefinition>;

const languageLookup: Record<string, LanguageDefinition> = languageRegistry;

export const highlightedLanguages: ReadonlyArray<readonly [string, LanguageFn]> =
  Object.entries(languageRegistry).flatMap(([name, definition]) =>
    "highlighter" in definition ? [[name, definition.highlighter] as const] : [],
  );

export function languageDisplayName(language: string): string {
  return languageLookup[language]?.displayName ?? language;
}

export function specificLanguageDisplayName(language: string): string | null {
  if (language === "text") return null;
  return languageLookup[language]?.displayName ?? null;
}
