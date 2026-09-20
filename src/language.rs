//! Canonical language identifiers shared by compiled code and diff files.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A renderer-safe language identifier frozen into `.learn` artifacts.
///
/// Authored aliases and file extensions are normalized to this closed set.
/// Unknown explicit values deliberately fall back to [`Language::Text`]
/// instead of making a lesson fail to compile.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    #[serde(rename = "javascript")]
    JavaScript,
    #[serde(rename = "typescript")]
    TypeScript,
    C,
    Cpp,
    Go,
    Java,
    Shell,
    Json,
    Yaml,
    Toml,
    Html,
    Xml,
    Css,
    Sql,
    Markdown,
    Mermaid,
    #[default]
    Text,
}

impl Language {
    pub const ALL: [Self; 19] = [
        Self::Rust,
        Self::Python,
        Self::JavaScript,
        Self::TypeScript,
        Self::C,
        Self::Cpp,
        Self::Go,
        Self::Java,
        Self::Shell,
        Self::Json,
        Self::Yaml,
        Self::Toml,
        Self::Html,
        Self::Xml,
        Self::Css,
        Self::Sql,
        Self::Markdown,
        Self::Mermaid,
        Self::Text,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::Java => "java",
            Self::Shell => "shell",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Html => "html",
            Self::Xml => "xml",
            Self::Css => "css",
            Self::Sql => "sql",
            Self::Markdown => "markdown",
            Self::Mermaid => "mermaid",
            Self::Text => "text",
        }
    }

    /// Normalize an authored language name or alias.
    pub fn from_authored(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Self::Rust,
            "python" | "py" => Self::Python,
            "javascript" | "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "typescript" | "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "c" | "h" => Self::C,
            "c++" | "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Self::Cpp,
            "go" | "golang" => Self::Go,
            "java" => Self::Java,
            "shell" | "sh" | "bash" | "zsh" | "fish" => Self::Shell,
            "json" | "jsonc" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "html" | "htm" => Self::Html,
            "xml" | "xsl" | "xslt" | "svg" => Self::Xml,
            "css" | "scss" | "sass" => Self::Css,
            "sql" => Self::Sql,
            "markdown" | "md" | "mdx" => Self::Markdown,
            "mermaid" | "mmd" => Self::Mermaid,
            "text" | "txt" | "plain" | "plaintext" => Self::Text,
            _ => Self::Text,
        }
    }

    /// Infer a language from a displayed root-relative path.
    pub fn from_path(path: &str) -> Self {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let lower_name = file_name.to_ascii_lowercase();
        let extension = Path::new(&lower_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        Self::from_authored(extension)
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_authored_aliases() {
        for (value, expected) in [
            ("RS", Language::Rust),
            ("py", Language::Python),
            ("js", Language::JavaScript),
            ("tsx", Language::TypeScript),
            ("c++", Language::Cpp),
            ("golang", Language::Go),
            ("bash", Language::Shell),
            ("yml", Language::Yaml),
            ("md", Language::Markdown),
            ("mmd", Language::Mermaid),
        ] {
            assert_eq!(Language::from_authored(value), expected, "{value}");
        }
    }

    #[test]
    fn serialized_names_match_the_canonical_renderer_contract() {
        for language in Language::ALL {
            let encoded = serde_json::to_value(language).unwrap();
            assert_eq!(encoded, language.as_str(), "{language:?}");
            assert_eq!(
                serde_json::from_value::<Language>(encoded).unwrap(),
                language
            );
        }
    }

    #[test]
    fn infers_paths_and_safely_falls_back_to_text() {
        for (path, expected) in [
            ("src/main.rs", Language::Rust),
            ("tool.py", Language::Python),
            ("web/app.js", Language::JavaScript),
            ("web/component.tsx", Language::TypeScript),
            ("native/main.c", Language::C),
            ("native/main.h", Language::C),
            ("native/main.cpp", Language::Cpp),
            ("native/main.hpp", Language::Cpp),
            ("cmd/server.go", Language::Go),
            ("Main.java", Language::Java),
            ("scripts/run.sh", Language::Shell),
            ("data.json", Language::Json),
            ("config.yml", Language::Yaml),
            ("Cargo.toml", Language::Toml),
            ("index.html", Language::Html),
            ("feed.xml", Language::Xml),
            ("style.css", Language::Css),
            ("query.sql", Language::Sql),
            ("README.md", Language::Markdown),
            ("diagram.mmd", Language::Mermaid),
            ("notes.txt", Language::Text),
        ] {
            assert_eq!(Language::from_path(path), expected, "{path}");
        }
        assert_eq!(Language::from_path("Dockerfile"), Language::Text);
        assert_eq!(Language::from_path("Makefile"), Language::Text);
        assert_eq!(Language::from_path("LICENSE"), Language::Text);
        assert_eq!(Language::from_authored("totally-unknown"), Language::Text);
    }
}
