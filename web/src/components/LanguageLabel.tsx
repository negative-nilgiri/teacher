const displayNames: Record<string, string> = {
  c: "C",
  cpp: "C++",
  css: "CSS",
  go: "Go",
  html: "HTML",
  java: "Java",
  javascript: "JavaScript",
  json: "JSON",
  markdown: "Markdown",
  mermaid: "Mermaid",
  python: "Python",
  rust: "Rust",
  shell: "Shell",
  sql: "SQL",
  text: "Plain text",
  toml: "TOML",
  typescript: "TypeScript",
  xml: "XML",
  yaml: "YAML",
};

export function specificLanguageDisplayName(language: string): string | null {
  if (language === "text") return null;
  return displayNames[language] ?? null;
}

export function LanguageLabel({ language }: { language: string }) {
  const label = displayNames[language] ?? language;
  return (
    <span aria-label={`Language: ${label}`} className="language-label">
      {label}
    </span>
  );
}
