//! Definitions shown by a lesson, for go-to-definition in the browser.
//!
//! This is not a parser: each supported language has a small table of the
//! keywords that start a definition (`fn`, `def`, `func`, `class`, ...). The
//! compiler scans the code blocks and the new-side lines of diffs, records
//! each definition's name and extent, and freezes a table of name → sites.
//! A language without a table simply gets no definitions, never wrong ones.

use std::collections::BTreeMap;

use crate::artifact::{
    CompiledNode, CompiledNodeContent, DefinitionKind, DefinitionSite, LinkedLines,
};
use crate::language::Language;
use crate::repository::DiffLineKind;

/// Longest extent recorded for one definition, so previews stay readable.
const MAX_EXTENT_LINES: usize = 40;

/// A definition found in a run of lines: its name and the zero-based index of
/// its first and last line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Found {
    pub(crate) name: String,
    pub(crate) kind: DefinitionKind,
    pub(crate) first: usize,
    pub(crate) last: usize,
}

/// How a language's statements end, which decides extents.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Style {
    /// Braces and semicolons, and a signature may continue on later lines.
    Braces,
    /// Braces for bodies, but a line without `{` is a whole definition.
    LineBraces,
    /// Python: the body is the following, more indented lines.
    Indent,
    /// SQL: a statement ends at `;`.
    Semicolon,
}

fn style(language: Language) -> Option<Style> {
    Some(match language {
        Language::Rust
        | Language::JavaScript
        | Language::TypeScript
        | Language::C
        | Language::Cpp
        | Language::Java => Style::Braces,
        Language::Go | Language::Shell => Style::LineBraces,
        Language::Python => Style::Indent,
        Language::Sql => Style::Semicolon,
        _ => return None,
    })
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// The identifier at the start of `text`, and the rest.
fn identifier(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    let mut characters = text.char_indices();
    let (_, first) = characters.next()?;
    if !is_identifier_start(first) {
        return None;
    }
    let end = characters
        .find(|(_, character)| !is_identifier_char(*character))
        .map_or(text.len(), |(index, _)| index);
    Some((&text[..end], &text[end..]))
}

/// `text` without a leading `word` followed by a non-identifier character.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix(word)?;
    match rest.chars().next() {
        Some(character) if is_identifier_char(character) => None,
        _ => Some(rest),
    }
}

/// Strip any number of leading words from `words`.
fn strip_all<'a>(mut text: &'a str, words: &[&str]) -> &'a str {
    loop {
        match words.iter().find_map(|word| strip_word(text, word)) {
            Some(rest) => text = rest,
            None => return text.trim_start(),
        }
    }
}

/// The name after one of `keywords` at the start of `text`.
fn after_keyword<'a>(text: &'a str, keywords: &[&str]) -> Option<&'a str> {
    keywords
        .iter()
        .find_map(|keyword| strip_word(text, keyword))
        .and_then(|rest| identifier(rest).map(|(name, _)| name))
}

const CONTROL: &[&str] = &[
    "if", "for", "while", "switch", "catch", "return", "else", "do", "try", "new", "throw", "case",
    "sizeof", "function", "await", "yield", "typeof", "delete", "match", "loop",
];

/// `name(...)` followed only by an optional `: Type` and `{`: a method in a
/// class body. Calls with callbacks (`describe("x", () => {`) are rejected.
fn method_name(text: &str) -> Option<&str> {
    let (name, rest) = identifier(text)?;
    let rest = rest.strip_prefix('(')?;
    let close = rest.rfind(')')?;
    let (arguments, tail) = (&rest[..close], rest[close + 1..].trim());
    let tail_ok = tail == "{" || (tail.starts_with(':') && tail.ends_with('{'));
    let arguments_ok = !arguments.contains("=>")
        && !arguments.contains('"')
        && !arguments.contains('\'')
        && !arguments.contains("function");
    (tail_ok && arguments_ok && !CONTROL.contains(&name)).then_some(name)
}

/// The name before the first `(` of a C, C++, or Java signature, when at least
/// one type token precedes it: `static int pop_front(struct queue *q)`.
fn signature_name(text: &str) -> Option<&str> {
    let open = text.find('(')?;
    let head = text[..open].trim_end();
    let name_start = head
        .rfind(|character: char| !is_identifier_char(character))
        .map_or(0, |index| index + 1);
    let name = &head[name_start..];
    let before = head[..name_start].trim_end();
    let before = before.strip_suffix("::").map_or(before, |text| {
        // `Queue::pop` names the method; drop the qualifier.
        let start = text
            .rfind(|character: char| !is_identifier_char(character))
            .map_or(0, |index| index + 1);
        text[..start].trim_end()
    });
    let first = before.split_whitespace().next()?;
    let valid = !name.is_empty()
        && name.starts_with(is_identifier_start)
        && !CONTROL.contains(&name)
        && !CONTROL.contains(&first)
        && !before.ends_with(['=', ',', '(', '.', '!', '&', '|', '+', '-', '?', ':'])
        && !text[open..].contains('"');
    valid.then_some(name)
}

/// The name defined on `line`, and its kind, for `language`.
fn defined_name(language: Language, line: &str) -> Option<(String, DefinitionKind)> {
    use DefinitionKind::{Command, Function, Macro, Type, Value};
    let top_level = !line.starts_with([' ', '\t']);
    let text = line.trim();
    let found: Option<(&str, DefinitionKind)> = match language {
        Language::Rust => {
            let text = match strip_word(text, "pub") {
                Some(rest) => rest
                    .trim_start()
                    .strip_prefix('(')
                    .and_then(|inner| inner.find(')').map(|end| &inner[end + 1..]))
                    .unwrap_or(rest),
                None => text,
            };
            let text = strip_all(text, &["async", "unsafe", "default", "extern", "\"C\""]);
            let text = match strip_word(text, "const") {
                Some(rest)
                    if strip_word(rest, "fn").is_some() || strip_word(rest, "unsafe").is_some() =>
                {
                    strip_all(rest, &["unsafe", "async"])
                }
                _ => text,
            };
            if let Some(rest) = text.strip_prefix("macro_rules!") {
                identifier(rest).map(|(name, _)| (name, Macro))
            } else if let Some(rest) = strip_word(text, "static") {
                identifier(strip_all(rest, &["mut"])).map(|(name, _)| (name, Value))
            } else if let Some(name) = after_keyword(text, &["fn"]) {
                Some((name, Function))
            } else if let Some(name) = after_keyword(text, &["const"]) {
                Some((name, Value))
            } else {
                after_keyword(text, &["struct", "enum", "trait", "type", "union", "mod"])
                    .map(|name| (name, Type))
            }
        }
        Language::Python => {
            let text = strip_all(text, &["async"]);
            after_keyword(text, &["def"])
                .map(|name| (name, Function))
                .or_else(|| after_keyword(text, &["class"]).map(|name| (name, Type)))
        }
        Language::JavaScript | Language::TypeScript => {
            let text = strip_all(
                text,
                &[
                    "export",
                    "default",
                    "declare",
                    "async",
                    "abstract",
                    "public",
                    "private",
                    "protected",
                    "static",
                    "readonly",
                    "override",
                ],
            );
            if let Some(rest) = strip_word(text, "function") {
                identifier(rest.trim_start().trim_start_matches('*'))
                    .map(|(name, _)| (name, Function))
            } else if let Some(name) =
                after_keyword(text, &["class", "interface", "type", "enum", "namespace"])
            {
                Some((name, Type))
            } else if let Some(rest) = ["const", "let", "var"]
                .iter()
                .find_map(|keyword| strip_word(text, keyword))
            {
                // Only module-level bindings: locals would shadow everywhere.
                identifier(rest)
                    .filter(|_| top_level)
                    .and_then(|(name, rest)| {
                        let rest = rest.trim_start();
                        let rest = match rest.strip_prefix(':') {
                            Some(typed) => typed.find('=').map_or("", |index| &typed[index..]),
                            None => rest,
                        };
                        if !rest.starts_with('=') || rest.starts_with("==") {
                            return None;
                        }
                        let value = rest[1..].trim_start();
                        let callable = value.contains("=>")
                            || strip_word(value, "function").is_some()
                            || strip_word(value, "async").is_some();
                        Some((name, if callable { Function } else { Value }))
                    })
            } else {
                // `get name() {` is an accessor; a method may also be named `get`.
                method_name(text)
                    .or_else(|| {
                        ["get", "set"]
                            .iter()
                            .find_map(|word| strip_word(text, word))
                            .and_then(method_name)
                    })
                    .map(|name| (name, Function))
            }
        }
        Language::Go => {
            if let Some(rest) = strip_word(text, "func") {
                let rest = rest.trim_start();
                let rest = match rest.strip_prefix('(') {
                    Some(receiver) => receiver.find(')').map_or("", |end| &receiver[end + 1..]),
                    None => rest,
                };
                identifier(rest)
                    .filter(|(_, after)| after.trim_start().starts_with(['(', '[']))
                    .map(|(name, _)| (name, Function))
            } else if let Some(name) = after_keyword(text, &["type"]) {
                Some((name, Type))
            } else {
                after_keyword(text, &["const", "var"])
                    .filter(|_| top_level)
                    .map(|name| (name, Value))
            }
        }
        Language::Java => {
            if text.starts_with('@') && !text.starts_with("@interface") {
                return None;
            }
            let text = strip_all(
                text,
                &[
                    "public",
                    "private",
                    "protected",
                    "static",
                    "final",
                    "abstract",
                    "sealed",
                    "synchronized",
                    "native",
                    "default",
                    "strictfp",
                    "transient",
                    "volatile",
                ],
            );
            let text = text.strip_prefix("non-sealed").unwrap_or(text);
            if let Some(rest) = text.strip_prefix("@interface") {
                identifier(rest).map(|(name, _)| (name, Type))
            } else if let Some(name) =
                after_keyword(text, &["class", "interface", "enum", "record"])
            {
                Some((name, Type))
            } else if text.trim_end().ends_with(';') {
                None
            } else if method_name(text).is_some_and(|name| {
                name.starts_with(|character: char| character.is_ascii_uppercase())
            }) {
                // A constructor: the class definition already covers the name.
                None
            } else {
                signature_name(text).map(|name| (name, Function))
            }
        }
        Language::C | Language::Cpp => {
            let text = strip_all(
                text,
                &[
                    "static",
                    "inline",
                    "extern",
                    "virtual",
                    "constexpr",
                    "explicit",
                    "friend",
                ],
            );
            if text.trim_end().ends_with(';') {
                // `typedef ... Name;` defines Name; other `;` lines declare.
                strip_word(text, "typedef").and_then(|rest| {
                    let rest = rest.trim_end().trim_end_matches(';');
                    let start = rest
                        .rfind(|character: char| !is_identifier_char(character))
                        .map_or(0, |index| index + 1);
                    let name = &rest[start..];
                    (!name.is_empty() && name.starts_with(is_identifier_start))
                        .then_some((name, Type))
                })
            } else if let Some(rest) = ["struct", "enum", "union", "class", "namespace"]
                .iter()
                .find_map(|keyword| strip_word(strip_all(text, &["typedef"]), keyword))
            {
                let rest = strip_all(rest, &["class"]);
                identifier(rest)
                    .filter(|(_, after)| {
                        let after = after.trim_start();
                        after.is_empty() || after.starts_with(['{', ':'])
                    })
                    .map(|(name, _)| (name, Type))
            } else {
                signature_name(text).map(|name| (name, Function))
            }
        }
        Language::Shell => {
            if let Some(rest) = strip_word(text, "function") {
                identifier(rest).map(|(name, _)| (name, Command))
            } else {
                identifier(text)
                    .filter(|(_, rest)| rest.trim_start().starts_with("()"))
                    .map(|(name, _)| (name, Command))
            }
        }
        Language::Sql => {
            let upper = text.to_ascii_uppercase();
            let mut rest = upper.strip_prefix("CREATE")?.trim_start().to_owned();
            for optional in ["OR REPLACE", "TEMPORARY", "TEMP", "UNIQUE"] {
                if let Some(after) = rest.strip_prefix(optional) {
                    rest = after.trim_start().to_owned();
                }
            }
            let kind = [
                "TABLE",
                "VIEW",
                "FUNCTION",
                "PROCEDURE",
                "INDEX",
                "TYPE",
                "TRIGGER",
                "SEQUENCE",
            ]
            .iter()
            .find(|kind| rest.starts_with(**kind))?;
            let mut rest = rest[kind.len()..].trim_start().to_owned();
            if let Some(after) = rest.strip_prefix("IF NOT EXISTS") {
                rest = after.trim_start().to_owned();
            }
            // Take the name from the original line to keep its case.
            let offset = upper.len() - rest.len();
            let original = &text[offset..];
            let raw = original
                .split(|character: char| character.is_whitespace() || character == '(')
                .next()?
                .trim_matches(['"', '`', '[', ']']);
            let name = raw.rsplit('.').next()?.trim_matches(['"', '`', '[', ']']);
            let valid = !name.is_empty()
                && name.starts_with(is_identifier_start)
                && name.chars().all(is_identifier_char);
            return valid.then(|| (name.to_owned(), Type));
        }
        _ => None,
    };
    found.map(|(name, kind)| (name.to_owned(), kind))
}

/// `line` without string literals and line comments, for brace counting.
fn code_only(line: &str, style: Style) -> String {
    let mut output = String::new();
    let mut characters = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(character) = characters.next() {
        match quote {
            Some(open) => {
                if character == '\\' {
                    characters.next();
                } else if character == open {
                    quote = None;
                }
            }
            None => {
                if character == '"' || (character == '\'' && style != Style::Braces) {
                    quote = Some(character);
                } else if (character == '/' && characters.peek() == Some(&'/'))
                    || (character == '#' && matches!(style, Style::LineBraces | Style::Indent))
                {
                    // The rest of the line is a comment.
                    break;
                } else if character == '\'' {
                    // A char literal such as '{' in C-like languages.
                    let literal = characters.clone().take(3).collect::<String>();
                    if literal.len() >= 2 && literal[1..].starts_with('\'') {
                        characters.next();
                        characters.next();
                    }
                } else {
                    output.push(character);
                }
            }
        }
    }
    output
}

/// The last line of the definition that starts at `first`.
fn extent(lines: &[&str], first: usize, style: Style) -> usize {
    let limit = (first + MAX_EXTENT_LINES).min(lines.len()) - 1;
    match style {
        Style::Indent => {
            let indent = |line: &str| line.len() - line.trim_start().len();
            let base = indent(lines[first]);
            let mut last = first;
            for (index, line) in lines.iter().enumerate().take(limit + 1).skip(first + 1) {
                if line.trim().is_empty() {
                    continue;
                }
                if indent(line) <= base {
                    break;
                }
                last = index;
            }
            last
        }
        Style::Semicolon => (first..=limit)
            .find(|index| lines[*index].trim_end().ends_with(';'))
            .unwrap_or(first),
        Style::Braces | Style::LineBraces => {
            let mut depth = 0i32;
            // Parentheses let a signature span several lines before its `{`.
            let mut parens = 0i32;
            let mut opened = false;
            for (index, line) in lines.iter().enumerate().take(limit + 1).skip(first) {
                for character in code_only(line, style).chars() {
                    match character {
                        '(' => parens += 1,
                        ')' => parens -= 1,
                        '{' => {
                            depth += 1;
                            opened = true;
                        }
                        '}' => depth -= 1,
                        ';' if !opened && depth == 0 && parens <= 0 && style == Style::Braces => {
                            return index;
                        }
                        _ => {}
                    }
                }
                if opened && depth <= 0 {
                    return index;
                }
                if !opened && parens <= 0 && style == Style::LineBraces {
                    return first;
                }
            }
            if opened { limit } else { first }
        }
    }
}

/// Every definition in `lines` for `language`.
pub(crate) fn find_definitions(language: Language, lines: &[&str]) -> Vec<Found> {
    let Some(style) = style(language) else {
        return Vec::new();
    };
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let (name, kind) = defined_name(language, line)?;
            Some(Found {
                name,
                kind,
                first: index,
                last: extent(lines, index, style),
            })
        })
        .collect()
}

/// The lesson's definition table: every shown definition by name, with the
/// block and the lines it spans, in the numbers the block's gutter shows.
pub(crate) fn index(nodes: &[CompiledNode]) -> BTreeMap<String, Vec<DefinitionSite>> {
    let mut table = BTreeMap::<String, Vec<DefinitionSite>>::new();
    let mut add = |name: String, link: DefinitionSite| {
        let sites = table.entry(name).or_default();
        if !sites.contains(&link) {
            sites.push(link);
        }
    };
    for node in nodes {
        match &node.content {
            CompiledNodeContent::Code {
                content,
                language,
                first_line,
                ..
            } => {
                let lines = content.lines().collect::<Vec<_>>();
                let first = first_line.unwrap_or(1);
                for found in find_definitions(*language, &lines) {
                    add(
                        found.name,
                        DefinitionSite {
                            kind: found.kind,
                            target: node.node_id,
                            lines: Some(LinkedLines {
                                start: first + found.first as u32,
                                end: first + found.last as u32,
                            }),
                        },
                    );
                }
            }
            CompiledNodeContent::Diff { diff, .. } => {
                for file in &diff.files {
                    for hunk in &file.hunks {
                        let shown = hunk
                            .lines
                            .iter()
                            .filter(|line| line.kind != DiffLineKind::Deletion)
                            .filter_map(|line| Some((line.new_line?, line.content.as_str())))
                            .collect::<Vec<_>>();
                        let texts = shown.iter().map(|(_, text)| *text).collect::<Vec<_>>();
                        for found in find_definitions(file.language, &texts) {
                            add(
                                found.name,
                                DefinitionSite {
                                    kind: found.kind,
                                    target: node.node_id,
                                    lines: Some(LinkedLines {
                                        start: shown[found.first].0,
                                        end: shown[found.last].0,
                                    }),
                                },
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(language: Language, source: &str) -> Vec<(String, usize, usize)> {
        let lines = source.lines().collect::<Vec<_>>();
        find_definitions(language, &lines)
            .into_iter()
            .map(|found| (found.name, found.first, found.last))
            .collect()
    }

    fn only_names(language: Language, source: &str) -> Vec<String> {
        names(language, source)
            .into_iter()
            .map(|(name, ..)| name)
            .collect()
    }

    #[test]
    fn rust_definitions_and_extents() {
        let source = "pub(crate) struct Queue {\n    items: Vec<u32>,\n}\n\nimpl Queue {\n    pub async fn pop_front(&mut self) -> u32 {\n        let brace = '{';\n        helper(\"}\")\n    }\n}\nconst MAX: usize = 4;\npub type Id = u32;\nmacro_rules! square {\n    ($x:expr) => { $x * $x };\n}\nstatic mut COUNT: u32 = 0;\npub const fn zero() -> u32 { 0 }";
        assert_eq!(
            names(Language::Rust, source),
            [
                ("Queue".into(), 0, 2),
                ("pop_front".into(), 5, 8),
                ("MAX".into(), 10, 10),
                ("Id".into(), 11, 11),
                ("square".into(), 12, 14),
                ("COUNT".into(), 15, 15),
                ("zero".into(), 16, 16),
            ]
        );
    }

    #[test]
    fn python_uses_indentation() {
        let source = "class Queue:\n    def pop(self):\n        return 1\n\n    async def peek(self):\n        pass\n\ndef helper():\n    return 2\nx = helper()";
        assert_eq!(
            names(Language::Python, source),
            [
                ("Queue".into(), 0, 5),
                ("pop".into(), 1, 2),
                ("peek".into(), 4, 5),
                ("helper".into(), 7, 8),
            ]
        );
    }

    #[test]
    fn javascript_and_typescript_forms() {
        let source = "export interface Queue {\n  items: number[];\n}\nexport async function popFront(q: Queue): number {\n  return q.items.shift();\n}\nconst LIMIT: number = 4;\nconst double = (x) => x * 2;\nclass Store {\n  get(key: string) {\n    return 1;\n  }\n}\ndescribe(\"queue\", () => {\n});\nif (x) {\n}\nexport type Id = string;";
        assert_eq!(
            only_names(Language::TypeScript, source),
            ["Queue", "popFront", "LIMIT", "double", "Store", "get", "Id"]
        );
    }

    #[test]
    fn go_forms() {
        let source = "type Queue struct {\n\titems []int\n}\n\nfunc (q *Queue) PopFront() int {\n\treturn q.items[0]\n}\nfunc helper[T any](x T) T { return x }\ntype ID int\nconst Limit = 4";
        assert_eq!(
            names(Language::Go, source),
            [
                ("Queue".into(), 0, 2),
                ("PopFront".into(), 4, 6),
                ("helper".into(), 7, 7),
                ("ID".into(), 8, 8),
                ("Limit".into(), 9, 9),
            ]
        );
    }

    #[test]
    fn java_forms_skip_calls_and_declarations() {
        let source = "@Override\npublic final class Queue<T> {\n    private int size;\n    public Queue(int size) {\n        this.size = size;\n    }\n    public static <T> List<T> popFront(int n) {\n        run(() -> {\n        });\n        return list;\n    }\n    abstract void peek();\n}\nrecord Point(int x, int y) {}";
        assert_eq!(
            only_names(Language::Java, source),
            ["Queue", "popFront", "Point"]
        );
    }

    #[test]
    fn c_and_cpp_forms() {
        let source = "struct queue {\n    int items;\n};\nstruct queue;\ntypedef unsigned int item_t;\nstatic int pop_front(struct queue *q)\n{\n    if (q) {\n    }\n    return helper(q);\n}\nint Queue::size() const {\n    return 0;\n}\nint x = compute(1);";
        assert_eq!(
            names(Language::Cpp, source),
            [
                ("queue".into(), 0, 2),
                ("item_t".into(), 4, 4),
                ("pop_front".into(), 5, 10),
                ("size".into(), 11, 13),
            ]
        );
    }

    #[test]
    fn shell_and_sql_forms() {
        let shell = "build() {\n  make\n}\nfunction deploy {\n  echo hi # }\n}\nclean () {\n  rm -rf out\n}\nname {";
        assert_eq!(
            names(Language::Shell, shell),
            [
                ("build".into(), 0, 2),
                ("deploy".into(), 3, 5),
                ("clean".into(), 6, 8)
            ]
        );
        let sql = "CREATE TABLE IF NOT EXISTS app.queue_items (\n  id int\n);\ncreate or replace view \"Recent\" as\nselect 1;\nSELECT * FROM queue_items;";
        assert_eq!(
            names(Language::Sql, sql),
            [("queue_items".into(), 0, 2), ("Recent".into(), 3, 4)]
        );
    }

    #[test]
    fn the_index_uses_gutter_numbers_and_keeps_ambiguous_names() {
        let input = serde_json::json!({"schema_version":"2.3.0","title":"Defs","blocks":[
            {"type":"code","id":"first","language":"rust","source":{"kind":"inline","content":"fn helper() {}\nstruct Queue;"}},
            {"type":"diff","id":"change","source":{"kind":"inline","content":"diff --git a/q.rs b/q.rs\n--- a/q.rs\n+++ b/q.rs\n@@ -10,2 +10,4 @@\n context\n-old\n+fn helper() {\n+}\n+fn added() {}\n"}}
        ]})
        .to_string();
        let artifact =
            crate::compiler::compile(&input, &crate::compiler::CompileOptions::new(".")).unwrap();
        let table = &artifact.presentation.definitions;
        assert_eq!(
            table["Queue"],
            [DefinitionSite {
                kind: DefinitionKind::Type,
                target: crate::source::NodeId::new(0),
                lines: Some(LinkedLines { start: 2, end: 2 })
            }]
        );
        // `helper` is shown twice, so the browser offers a chooser.
        assert_eq!(table["helper"].len(), 2);
        assert_eq!(
            table["helper"][1],
            DefinitionSite {
                kind: DefinitionKind::Function,
                target: crate::source::NodeId::new(1),
                lines: Some(LinkedLines { start: 11, end: 12 })
            }
        );
        assert_eq!(
            table["added"][0].lines,
            Some(LinkedLines { start: 13, end: 13 })
        );
        assert!(!table.contains_key("old"));
    }

    #[test]
    fn kinds_decide_which_usages_link_and_locals_are_skipped() {
        let lines = [
            "const LIMIT = 4;",
            "export const double = (x) => x * 2;",
            "function outer() {",
            "  const local = 1;",
            "  let other = () => 2;",
            "}",
            "class Store {}",
        ];
        let found = find_definitions(Language::TypeScript, &lines)
            .into_iter()
            .map(|found| (found.name, found.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("LIMIT".to_owned(), DefinitionKind::Value),
                ("double".to_owned(), DefinitionKind::Function),
                ("outer".to_owned(), DefinitionKind::Function),
                ("Store".to_owned(), DefinitionKind::Type),
            ]
        );
        let rust = find_definitions(
            Language::Rust,
            &[
                "macro_rules! square {",
                "}",
                "fn answer(",
                "    jobs: &[Job],",
                "    cache: bool,",
                ") -> u32 {",
                "    0",
                "}",
            ],
        );
        assert_eq!(rust[0].kind, DefinitionKind::Macro);
        // A signature over several lines keeps its whole body.
        assert_eq!(
            (rust[1].name.as_str(), rust[1].first, rust[1].last),
            ("answer", 2, 7)
        );
        assert_eq!(rust[1].kind, DefinitionKind::Function);
    }

    #[test]
    fn unsupported_languages_define_nothing() {
        assert!(find_definitions(Language::Json, &["{\"fn\": 1}"]).is_empty());
        assert!(find_definitions(Language::Mermaid, &["flowchart LR"]).is_empty());
    }
}
