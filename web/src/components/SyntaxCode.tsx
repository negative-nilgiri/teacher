import hljs from "highlight.js/lib/core";
import { markDefinitions, type CodeOrigin } from "../definitions";
import { highlightedLanguages } from "../languages";
import { useLinks } from "../links";

for (const [name, definition] of highlightedLanguages) {
  hljs.registerLanguage(name, definition);
}

interface SyntaxCodeProps {
  children: string;
  className?: string;
  language?: string;
  /** Where this code sits, so names with a shown definition can link. */
  origin?: CodeOrigin;
}

export function SyntaxCode({ children, className, language, origin }: SyntaxCodeProps) {
  const { definitions, inPreview } = useLinks();
  const supportedLanguage = language && hljs.getLanguage(language) ? language : null;
  const classes = [className, supportedLanguage && `language-${supportedLanguage}`]
    .filter(Boolean)
    .join(" ");

  if (!supportedLanguage) {
    return <code className={classes || undefined}>{children}</code>;
  }

  let highlighted = hljs.highlight(children, {
    language: supportedLanguage,
    ignoreIllegals: true,
  }).value;
  // Previews never link again, so hovering inside one cannot nest.
  if (origin && definitions && !inPreview) {
    highlighted = markDefinitions(highlighted, definitions, origin);
  }

  return (
    <code
      className={classes || undefined}
      // highlight.js escapes source text before adding its own markup.
      dangerouslySetInnerHTML={{ __html: highlighted }}
    />
  );
}
