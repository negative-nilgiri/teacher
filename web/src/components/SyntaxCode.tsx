import hljs from "highlight.js/lib/core";
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

const languages = {
  bash,
  c,
  cpp,
  css,
  go,
  html: xml,
  java,
  javascript,
  json,
  markdown,
  python,
  rust,
  shell: bash,
  sql,
  toml: ini,
  typescript,
  xml,
  yaml,
} as const;

for (const [name, definition] of Object.entries(languages)) {
  hljs.registerLanguage(name, definition);
}

interface SyntaxCodeProps {
  children: string;
  className?: string;
  language?: string;
}

export function SyntaxCode({ children, className, language }: SyntaxCodeProps) {
  const supportedLanguage = language && hljs.getLanguage(language) ? language : null;
  const classes = [className, supportedLanguage && `language-${supportedLanguage}`]
    .filter(Boolean)
    .join(" ");

  if (!supportedLanguage) {
    return <code className={classes || undefined}>{children}</code>;
  }

  const highlighted = hljs.highlight(children, {
    language: supportedLanguage,
    ignoreIllegals: true,
  }).value;

  return (
    <code
      className={classes || undefined}
      // highlight.js escapes source text before adding its own markup.
      dangerouslySetInnerHTML={{ __html: highlighted }}
    />
  );
}
