import { languageDisplayName } from "../languages";

export function LanguageLabel({ language }: { language: string }) {
  const label = languageDisplayName(language);
  return (
    <span aria-label={`Language: ${label}`} className="language-label">
      {label}
    </span>
  );
}
