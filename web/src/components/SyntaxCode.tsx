import hljs from "highlight.js/lib/core";
import { highlightedLanguages } from "../languages";

for (const [name, definition] of highlightedLanguages) {
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
