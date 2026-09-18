use super::LessonSource;

/// Exact JSON Schema for source documents accepted by this decoder.
pub fn source_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(LessonSource))
        .expect("generated lesson source schema must serialize")
}

pub fn source_json_schema_pretty() -> String {
    serde_json::to_string_pretty(&source_json_schema())
        .expect("generated lesson source schema must serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn definition<'a>(schema: &'a Value, name: &str) -> &'a Value {
        &schema["$defs"][name]
    }

    fn variant<'a>(schema: &'a Value, definition_name: &str, tag: &str) -> &'a Value {
        definition(schema, definition_name)["oneOf"]
            .as_array()
            .expect("tagged union has oneOf variants")
            .iter()
            .find(|candidate| {
                candidate["properties"]["kind"]["const"] == tag
                    || candidate["properties"]["type"]["const"] == tag
            })
            .unwrap_or_else(|| panic!("{definition_name} omitted `{tag}` variant"))
    }

    #[test]
    fn schema_names_all_four_block_types_and_current_version() {
        let schema = source_json_schema_pretty();
        for expected in ["markdown", "code", "diff", "multiple_choice", "1.0.0"] {
            assert!(schema.contains(expected), "schema omitted {expected}");
        }
    }

    #[test]
    fn every_tagged_variant_is_a_closed_object() {
        let schema = source_json_schema();
        for definition_name in [
            "Block",
            "MarkdownSource",
            "CodeSource",
            "DiffSource",
            "GitDiffTarget",
        ] {
            let variants = definition(&schema, definition_name)["oneOf"]
                .as_array()
                .expect("tagged union has oneOf variants");
            assert!(!variants.is_empty(), "{definition_name} has no variants");
            for candidate in variants {
                assert_eq!(
                    candidate["additionalProperties"],
                    Value::Bool(false),
                    "{definition_name} variant is not closed: {candidate}"
                );
            }
        }
    }

    #[test]
    fn schema_exposes_source_validation_constraints() {
        let schema = source_json_schema();

        assert_eq!(schema["properties"]["title"]["minLength"], 1);
        assert_eq!(schema["properties"]["title"]["pattern"], r"\S");

        for definition_name in ["SourceId", "RepoPath", "GitRevision"] {
            let definition = definition(&schema, definition_name);
            assert_eq!(
                definition["minLength"], 1,
                "{definition_name} permits empty strings"
            );
            assert!(
                definition["pattern"].is_string(),
                "{definition_name} omitted its semantic pattern"
            );
        }

        let markdown_inline = variant(&schema, "MarkdownSource", "inline");
        assert_eq!(markdown_inline["properties"]["content"]["minLength"], 1);
        assert_eq!(markdown_inline["properties"]["content"]["pattern"], r"\S");

        let question = variant(&schema, "Block", "multiple_choice");
        assert_eq!(question["properties"]["choices"]["minItems"], 2);
        assert_eq!(question["properties"]["hints"]["items"]["minLength"], 1);
        assert!(
            question["properties"]["choices"]["description"]
                .as_str()
                .expect("choices have a description")
                .contains("exactly one")
        );

        let git_diff = variant(&schema, "DiffSource", "git");
        assert_eq!(git_diff["properties"]["files"]["minItems"], 1);
        assert!(
            git_diff["properties"]["files"]["description"]
                .as_str()
                .expect("file selections have a description")
                .contains("only once")
        );

        let line_range = definition(&schema, "LineRange");
        assert_eq!(line_range["properties"]["start"]["minimum"], 1);
        assert_eq!(line_range["properties"]["end"]["minimum"], 1);
        assert!(
            line_range["properties"]["end"]["description"]
                .as_str()
                .expect("range end has a description")
                .contains("must not precede")
        );
    }
}
