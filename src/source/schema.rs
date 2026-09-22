use super::{
    SchemaVersion,
    model::{
        LessonSourceV1_0_0, LessonSourceV1_1_0, LessonSourceV1_2_0, LessonSourceV1_3_0,
        LessonSourceV2_0_0, LessonSourceV2_1_0,
    },
};

/// Exact JSON Schema for the current authored-document version.
pub fn source_json_schema() -> serde_json::Value {
    source_json_schema_for(SchemaVersion::CURRENT)
}

/// Exact JSON Schema for one supported authored-document version.
pub fn source_json_schema_for(version: SchemaVersion) -> serde_json::Value {
    match version {
        SchemaVersion::V1_0_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV1_0_0)),
        SchemaVersion::V1_1_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV1_1_0)),
        SchemaVersion::V1_2_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV1_2_0)),
        SchemaVersion::V1_3_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV1_3_0)),
        SchemaVersion::V2_0_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV2_0_0)),
        SchemaVersion::V2_1_0 => serde_json::to_value(schemars::schema_for!(LessonSourceV2_1_0)),
    }
    .expect("generated lesson source schema must serialize")
}

pub fn source_json_schema_pretty() -> String {
    source_json_schema_pretty_for(SchemaVersion::CURRENT)
}

pub fn source_json_schema_pretty_for(version: SchemaVersion) -> String {
    serde_json::to_string_pretty(&source_json_schema_for(version))
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
        for expected in ["markdown", "code", "diff", "multiple_choice", "2.1.0"] {
            assert!(schema.contains(expected), "schema omitted {expected}");
        }
    }

    #[test]
    fn schemas_are_exact_for_each_supported_version() {
        let v1_0 = source_json_schema_for(SchemaVersion::V1_0_0);
        let v1_1 = source_json_schema_for(SchemaVersion::V1_1_0);
        let v1_2 = source_json_schema_for(SchemaVersion::V1_2_0);
        let v1_3 = source_json_schema_for(SchemaVersion::V1_3_0);
        let v2_0 = source_json_schema_for(SchemaVersion::V2_0_0);
        let v2_1 = source_json_schema_for(SchemaVersion::V2_1_0);
        let v1_0_code = variant(&v1_0, "BlockV1_0_0", "code");
        let v1_1_code = variant(&v1_1, "BlockV1_1_0", "code");
        let v1_2_code = variant(&v1_2, "BlockV1_2_0", "code");
        let v1_2_diff = variant(&v1_2, "BlockV1_2_0", "diff");
        let v1_3_code = variant(&v1_3, "BlockV1_3_0", "code");
        let v1_3_question = variant(&v1_3, "BlockV1_3_0", "multiple_choice");
        let v2_0_question = variant(&v2_0, "Block", "multiple_choice");
        let v2_0_code = variant(&v2_0, "Block", "code");
        let v2_1_code = variant(&v2_1, "Block", "code");
        let v2_1_question = variant(&v2_1, "Block", "multiple_choice");
        assert!(v1_0_code["properties"].get("language").is_none());
        assert!(v1_1_code["properties"].get("language").is_some());
        assert!(v1_1_code["properties"].get("caption").is_none());
        assert!(v1_2_code["properties"].get("caption").is_some());
        assert!(v1_2_diff["properties"].get("caption").is_some());
        assert!(v1_2_code["properties"].get("highlights").is_none());
        assert!(v1_3_code["properties"].get("highlights").is_some());
        assert_eq!(v1_3_question["properties"]["prompt"]["type"], "string");
        assert_eq!(
            v2_0_question["properties"]["prompt"]["$ref"],
            "#/$defs/MarkdownSource"
        );
        assert!(
            definition(&v2_0, "CodeHighlight")["properties"]
                .get("annotation")
                .is_none()
        );
        assert!(
            definition(&v2_1, "CodeHighlight")["properties"]
                .get("annotation")
                .is_some()
        );
        assert_eq!(
            v2_1_question["properties"]["prompt"]["$ref"],
            "#/$defs/MarkdownSource"
        );
        assert!(v2_0_code["properties"].get("highlights").is_some());
        assert!(v2_1_code["properties"].get("highlights").is_some());
        assert_eq!(
            v1_0["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV1_0_0"
        );
        assert_eq!(
            v1_1["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV1_1_0"
        );
        assert_eq!(
            v1_2["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV1_2_0"
        );
        assert_eq!(
            v1_3["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV1_3_0"
        );
        assert_eq!(
            v2_0["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV2_0_0"
        );
        assert_eq!(
            v2_1["properties"]["schema_version"]["$ref"],
            "#/$defs/SchemaVersionV2_1_0"
        );
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

        let code_block = variant(&schema, "Block", "code");
        assert_eq!(code_block["properties"]["language"]["minLength"], 1);
        assert_eq!(code_block["properties"]["language"]["pattern"], r"\S");
        assert!(
            !code_block["required"]
                .as_array()
                .expect("code block has required fields")
                .iter()
                .any(|field| field == "language")
        );

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
