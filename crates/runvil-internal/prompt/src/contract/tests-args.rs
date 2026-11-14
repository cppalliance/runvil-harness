//! Tests for the `args` key: the keywords a declaration takes and the JSON
//! Schema it publishes, the default declaration, and each refused
//! declaration with its YAML path, line, and column.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use super::*;
use crate::ArgsSchema;
use crate::detail::TEXT_FIELD;

/// The keywords a node takes, as the refusal of any other keyword lists
/// them.
const KEYWORDS: &str = "the keywords are `type`, `properties`, `required`, \
                        `additionalProperties`, `items`, `enum`, `description`, and `default`";

/// The six type words, as the refusal of any other word lists them.
const TYPES: &str = "expected one of `string`, `boolean`, `integer`, `number`, `array`, `object`";

/// What the refusal of a node with no `type` says.
const NEEDS_TYPE: &str =
    "a node needs a `type`: `string`, `boolean`, `integer`, `number`, `array`, or `object`";

/// The JSON Schema the declaration in the YAML block `front` publishes.
#[track_caller]
fn published(front: &str) -> Json {
    parse_ok(&block_prompt(front))
        .frontmatter()
        .args()
        .as_json()
        .clone()
}

/// A declaration with one field, `field`, whose node is the flow mapping
/// `node`. The node opens on file line 8 at column 12, which the YAML
/// parser reports as line 7 column 12.
fn one_field(node: &str) -> String {
    format!("args:\n  type: object\n  properties:\n    field: {node}\n")
}

/// The JSON Schema published for a field whose node is the flow mapping
/// `node`.
#[track_caller]
fn published_field(node: &str) -> Json {
    published(&one_field(node))["properties"]["field"].clone()
}

/// Asserts that the declaration in the YAML block `front` fails the parse
/// with the message `invalid frontmatter: ` then `refusal`, located at the
/// file line and column `at`.
#[track_caller]
fn assert_refused(front: &str, refusal: &str, at: (u32, u32)) {
    let error = parse_err(&block_prompt(front));
    let (line, column) = at;
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error:?}");
    assert_eq!(
        error.message,
        format!("invalid frontmatter: {refusal}"),
        "{front}"
    );
    assert_eq!(
        location(&error),
        Some(("prompt", line, column, line, column)),
        "{refusal}"
    );
}

/// Asserts that a field whose node is the flow mapping `node` fails the
/// parse with `refusal` about that node, located where the node opens.
#[track_caller]
fn assert_field_refused(node: &str, refusal: &str) {
    assert_refused(
        &one_field(node),
        &format!("args.properties.field: {refusal} at line 7 column 12"),
        (8, 12),
    );
}

#[test]
fn each_of_the_six_types_parses_and_is_published_under_type() {
    let schema = published(
        "
        args:
          type: object
          properties:
            name: { type: string }
            flag: { type: boolean }
            count: { type: integer }
            ratio: { type: number }
            tags: { type: array, items: { type: string } }
            opts: { type: object }
        ",
    );
    assert_eq!(
        schema,
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "flag": { "type": "boolean" },
                "count": { "type": "integer" },
                "ratio": { "type": "number" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "opts": { "type": "object", "properties": {}, "additionalProperties": false },
            },
            "additionalProperties": false,
        })
    );
}

#[test]
fn properties_are_published_by_name_at_every_depth() {
    let schema = published(
        "
        args:
          type: object
          properties:
            page:
              type: object
              properties:
                size: { type: integer }
                sort:
                  type: object
                  properties:
                    by: { type: string }
        ",
    );
    assert_eq!(
        schema["properties"]["page"],
        json!({
            "type": "object",
            "properties": {
                "size": { "type": "integer" },
                "sort": {
                    "type": "object",
                    "properties": { "by": { "type": "string" } },
                    "additionalProperties": false,
                },
            },
            "additionalProperties": false,
        })
    );
}

#[test]
fn an_object_that_declares_no_field_publishes_empty_properties() {
    let empty = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    for front in [
        "args: { type: object }",
        "args: { type: object, properties: {} }",
    ] {
        assert_eq!(published(front), empty, "{front}");
    }
}

#[test]
fn required_is_published_in_name_order_and_only_when_it_names_a_field() {
    let schema = published(
        "
        args:
          type: object
          properties:
            query: { type: string }
            depth: { type: string }
            limit: { type: integer }
          required: [query, depth]
        ",
    );
    assert_eq!(schema["required"], json!(["depth", "query"]));

    for front in [
        "args: { type: object, properties: { query: { type: string } } }",
        "args: { type: object, properties: { query: { type: string } }, required: [] }",
    ] {
        assert_eq!(published(front).get("required"), None, "{front}");
    }
}

#[test]
fn additional_properties_is_published_as_written_and_as_false_when_left_out() {
    for (written, open) in [
        ("{ type: object, additionalProperties: true }", true),
        ("{ type: object, additionalProperties: false }", false),
        ("{ type: object }", false),
    ] {
        let root = published(&format!("args: {written}"));
        assert_eq!(
            root["additionalProperties"],
            json!(open),
            "the root: {written}"
        );
        let field = published_field(written);
        assert_eq!(
            field["additionalProperties"],
            json!(open),
            "a field: {written}"
        );
    }
}

#[test]
fn items_declares_the_node_of_each_list_item() {
    for (node, items) in [
        (
            "{ type: array, items: { type: string } }",
            json!({ "type": "string" }),
        ),
        (
            "{ type: array, items: { type: array, items: { type: integer } } }",
            json!({ "type": "array", "items": { "type": "integer" } }),
        ),
        (
            "{ type: array, items: { type: object, properties: { name: { type: string } }, \
             required: [name] } }",
            json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"],
                "additionalProperties": false,
            }),
        ),
    ] {
        assert_eq!(
            published_field(node),
            json!({ "type": "array", "items": items }),
            "{node}"
        );
    }
}

#[test]
fn an_enum_is_published_in_the_order_written_on_a_string_and_on_an_integer() {
    assert_eq!(
        published_field("{ type: string, enum: [quick, full] }"),
        json!({ "type": "string", "enum": ["quick", "full"] })
    );
    assert_eq!(
        published_field("{ type: integer, enum: [3, 1, -2] }"),
        json!({ "type": "integer", "enum": [3, 1, -2] })
    );
    // A quoted scalar is a string, whatever it looks like.
    assert_eq!(
        published_field("{ type: string, enum: ['1', 'true', 'null'] }"),
        json!({ "type": "string", "enum": ["1", "true", "null"] })
    );
}

#[test]
fn a_description_is_published_on_the_root_a_field_and_a_list_item() {
    let schema = published(
        "
        args:
          type: object
          description: What a search takes
          properties:
            focus:
              type: array
              description: The areas to cover
              items:
                type: string
                description: One area
        ",
    );
    assert_eq!(
        schema,
        json!({
            "type": "object",
            "description": "What a search takes",
            "properties": {
                "focus": {
                    "type": "array",
                    "description": "The areas to cover",
                    "items": { "type": "string", "description": "One area" },
                },
            },
            "additionalProperties": false,
        })
    );
}

#[test]
fn a_default_is_published_as_json_for_a_scalar_a_list_and_an_object() {
    for (node, default) in [
        ("{ type: string, default: quick }", json!("quick")),
        ("{ type: boolean, default: false }", json!(false)),
        ("{ type: integer, default: 3 }", json!(3)),
        ("{ type: number, default: 1.5 }", json!(1.5)),
        (
            "{ type: string, enum: [quick, full], default: full }",
            json!("full"),
        ),
        (
            "{ type: array, items: { type: string }, default: [a, b] }",
            json!(["a", "b"]),
        ),
        (
            "{ type: array, items: { type: string }, default: [] }",
            json!([]),
        ),
        ("{ type: object, default: {} }", json!({})),
        (
            "{ type: object, properties: { size: { type: integer }, tags: { type: array, \
             items: { type: string } } }, default: { size: 20, tags: [new] } }",
            json!({ "size": 20, "tags": ["new"] }),
        ),
        (
            "{ type: object, additionalProperties: true, default: { any: [1, { deep: true }] } }",
            json!({ "any": [1, { "deep": true }] }),
        ),
    ] {
        assert_eq!(published_field(node)["default"], default, "{node}");
    }
}

#[test]
fn a_default_holds_the_defaults_of_the_fields_it_leaves_out() {
    // A published default is the whole value a missing field takes, so
    // the fields the written default leaves out are filled at every depth.
    let schema = published(
        "
        args:
          type: object
          properties:
            page:
              type: object
              properties:
                size: { type: integer, default: 20 }
                sort:
                  type: object
                  properties:
                    by: { type: string, default: date }
                    desc: { type: boolean }
                tags:
                  type: array
                  items:
                    type: object
                    properties:
                      label: { type: string }
                      weight: { type: number, default: 1 }
                view:
                  type: object
                  properties:
                    mode: { type: string }
                  default: { mode: list }
              default: { sort: {}, tags: [{ label: new }] }
        ",
    );
    assert_eq!(
        schema["properties"]["page"]["default"],
        json!({
            "size": 20,
            "sort": { "by": "date" },
            "tags": [{ "label": "new", "weight": 1 }],
            "view": { "mode": "list" },
        })
    );
}

#[test]
fn a_whole_number_default_of_an_integer_field_is_published_as_an_integer() {
    for (node, text) in [
        (
            "{ type: integer, default: 5.0 }",
            r#"{"default":5,"type":"integer"}"#,
        ),
        (
            "{ type: integer, default: 1e3 }",
            r#"{"default":1000,"type":"integer"}"#,
        ),
        // A `number` field keeps its default as written.
        (
            "{ type: number, default: 5 }",
            r#"{"default":5,"type":"number"}"#,
        ),
        (
            "{ type: number, default: 5.0 }",
            r#"{"default":5.0,"type":"number"}"#,
        ),
    ] {
        assert_eq!(published_field(node).to_string(), text, "{node}");
    }
}

#[test]
fn a_null_default_declares_no_default() {
    for written in ["null", "~", ""] {
        let node = format!("{{ type: integer, default: {written} }}");
        assert_eq!(
            published_field(&node),
            json!({ "type": "integer" }),
            "{node}"
        );
    }
}

#[test]
fn the_published_json_is_sorted_and_says_whether_each_object_is_closed() {
    let prompt = parse_ok(&block_prompt(
        "
        args:
          type: object
          description: What a search takes
          required: [query]
          properties:
            query: { type: string, description: What to search for }
            limit: { type: integer, default: 3 }
            depth: { type: string, enum: [quick, full] }
            focus:
              type: array
              items: { type: string }
            page:
              type: object
              properties:
                size: { type: integer, default: 20 }
            filters:
              type: object
              additionalProperties: true
        ",
    ));
    let json =
        serde_json::to_string(prompt.frontmatter().args().as_json()).expect("a schema serializes");
    // Keys are in name order at every depth. A closed object says
    // `"additionalProperties":false` and an open one `true`, every object
    // lists its `properties`, and `required` is on the root alone, the one
    // object that requires a field.
    assert_eq!(
        json,
        concat!(
            r#"{"additionalProperties":false,"description":"What a search takes","properties":{"#,
            r#""depth":{"enum":["quick","full"],"type":"string"},"#,
            r#""filters":{"additionalProperties":true,"properties":{},"type":"object"},"#,
            r#""focus":{"items":{"type":"string"},"type":"array"},"#,
            r#""limit":{"default":3,"type":"integer"},"#,
            r#""page":{"additionalProperties":false,"properties":"#,
            r#"{"size":{"default":20,"type":"integer"}},"type":"object"},"#,
            r#""query":{"description":"What to search for","type":"string"}},"#,
            r#""required":["query"],"type":"object"}"#,
        )
    );
}

#[test]
fn a_prompt_with_no_args_key_declares_one_optional_string_named_text() {
    let prompt = parse_ok(&prompt("", &[]));
    let json =
        serde_json::to_string(prompt.frontmatter().args().as_json()).expect("a schema serializes");
    assert_eq!(
        json,
        concat!(
            r#"{"additionalProperties":false,"properties":{"text":"#,
            r#"{"description":"Freeform text for this prompt","type":"string"}},"type":"object"}"#,
        )
    );
    assert_eq!(TEXT_FIELD, "text");
}

#[test]
fn the_default_declaration_equals_a_written_copy_of_it() {
    let implied = parse_ok(&prompt("", &[]));
    let implied = implied.frontmatter().args();
    assert_eq!(*implied, ArgsSchema::default());

    for front in [
        "args: { type: object, properties: { text: { type: string, description: Freeform text \
         for this prompt } } }",
        // The same declaration with its keywords in another order and the
        // two keywords a closed object with no required field may leave
        // out.
        "args: { additionalProperties: false, required: [], properties: { text: { description: \
         Freeform text for this prompt, type: string } }, type: object }",
    ] {
        let written = parse_ok(&prompt(front, &[]));
        assert_eq!(written.frontmatter().args(), implied, "{front}");
    }

    let other = parse_ok(&prompt(
        "args: { type: object, properties: { text: { type: string } } }",
        &[],
    ));
    assert_ne!(
        other.frontmatter().args(),
        implied,
        "a copy with no description is another declaration"
    );
}

#[test]
fn an_unknown_keyword_is_refused_at_its_own_line_and_column() {
    let error = parse_err(&block_prompt(
        "
        args:
          type: object
          properties:
            query:
              tipe: string
        ",
    ));
    let message = "invalid frontmatter: args.properties.query: unknown keyword \"tipe\"; the \
                   keywords are `type`, `properties`, `required`, `additionalProperties`, \
                   `items`, `enum`, `description`, and `default` at line 8 column 7";
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    assert_eq!(error.message, message);
    // The YAML parser counts lines from the first line after the opening
    // `---`, so the line 8 it reports is file line 9.
    assert_eq!(location(&error), Some(("prompt", 9, 7, 9, 7)));
    assert_eq!(error.facts, BTreeMap::new());
    assert_eq!(
        error.prose,
        format!(
            "prompt:9: {message}. {}",
            ErrorCode::FrontmatterInvalid.prose()
        )
    );
}

#[test]
fn a_json_schema_keyword_outside_the_subset_is_refused_at_every_depth() {
    for keyword in [
        "title", "minimum", "pattern", "format", "oneOf", "$ref", "const", "nullable", "Type",
    ] {
        assert_refused(
            &format!("args:\n  type: object\n  {keyword}: 1\n"),
            &format!("args: unknown keyword \"{keyword}\"; {KEYWORDS} at line 6 column 3"),
            (7, 3),
        );
    }
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              type: string
              minLength: 1
        ",
        &format!(
            "args.properties.query: unknown keyword \"minLength\"; {KEYWORDS} at line 9 column 7"
        ),
        (10, 7),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              items:
                type: string
                pattern: ^a
        ",
        &format!(
            "args.properties.focus.items: unknown keyword \"pattern\"; {KEYWORDS} at line 11 \
             column 9"
        ),
        (12, 9),
    );
}

#[test]
fn an_unknown_type_word_is_refused_listing_the_six_types() {
    assert_refused(
        "
        args:
          type: object
          properties:
            flag:
              type: text
        ",
        &format!("args.properties.flag.type: unknown variant `text`, {TYPES} at line 8 column 13"),
        (9, 13),
    );
    // A type is one of the six words as written: another case, a short
    // form, and `null` are not among them.
    for word in ["String", "int", "float", "bool", "list", "any", "null"] {
        assert_refused(
            &one_field(&format!("{{ type: {word} }}")),
            &format!(
                "args.properties.field.type: unknown variant `{word}`, {TYPES} at line 7 column 20"
            ),
            (8, 20),
        );
    }
}

#[test]
fn a_type_that_is_not_one_word_is_refused_naming_what_a_type_is() {
    // A type is one word, so a list of words and a map are each refused.
    for (written, kind) in [("[string, integer]", "sequence"), ("{ a: 1 }", "map")] {
        assert_refused(
            &one_field(&format!("{{ type: {written} }}")),
            &format!(
                "args.properties.field.type: invalid type: {kind}, expected one type word: \
                 `string`, `boolean`, `integer`, `number`, `array`, or `object` at line 7 \
                 column 20"
            ),
            (8, 20),
        );
    }
}

#[test]
fn a_node_with_no_type_is_refused_where_the_node_opens() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              description: What to search for
        ",
        &format!("args.properties.query: {NEEDS_TYPE} at line 8 column 7"),
        (9, 7),
    );
    assert_refused(
        "
        args:
          properties:
            query: { type: string }
        ",
        &format!("args: {NEEDS_TYPE} at line 5 column 3"),
        (6, 3),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              items:
                description: One area
        ",
        &format!("args.properties.focus.items: {NEEDS_TYPE} at line 10 column 9"),
        (11, 9),
    );
    assert_field_refused("{}", NEEDS_TYPE);
}

#[test]
fn a_root_that_is_not_an_object_is_refused() {
    for (kind, rest) in [
        ("string", ""),
        ("boolean", ""),
        ("integer", ""),
        ("number", ""),
        ("array", "  items: { type: string }\n"),
    ] {
        assert_refused(
            &format!("args:\n  type: {kind}\n{rest}"),
            &format!(
                "args: the root of `args` must be `type: object`, and it is `{kind}` at line 5 \
                 column 3"
            ),
            (6, 3),
        );
    }
}

#[test]
fn the_root_takes_no_default_and_no_enum() {
    assert_refused(
        "
        args:
          type: object
          default: {}
        ",
        "args: the root of `args` takes no `default` at line 5 column 3",
        (6, 3),
    );
    assert_refused(
        "
        args:
          type: object
          enum: [a]
        ",
        "args: `enum` applies to a `string` or an `integer` node, and this node is `object` at \
         line 5 column 3",
        (6, 3),
    );
}

#[test]
fn a_list_item_takes_no_default() {
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              items:
                type: string
                default: all
        ",
        "args.properties.focus.items: a list item takes no `default` at line 10 column 9",
        (11, 9),
    );
}

#[test]
fn items_is_refused_on_a_node_that_is_not_an_array() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              type: string
              items:
                type: string
        ",
        "args.properties.query: `items` applies to an `array` node, and this node is `string` at \
         line 8 column 7",
        (9, 7),
    );
    for kind in ["string", "boolean", "integer", "number", "object"] {
        assert_field_refused(
            &format!("{{ type: {kind}, items: {{ type: string }} }}"),
            &format!("`items` applies to an `array` node, and this node is `{kind}`"),
        );
    }
}

#[test]
fn properties_required_and_additional_properties_are_refused_on_a_node_that_is_not_an_object() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              type: string
              properties:
                words: { type: integer }
        ",
        "args.properties.query: `properties` applies to an `object` node, and this node is \
         `string` at line 8 column 7",
        (9, 7),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              items: { type: string }
              required: [first]
        ",
        "args.properties.focus: `required` applies to an `object` node, and this node is `array` \
         at line 8 column 7",
        (9, 7),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            limit:
              type: integer
              additionalProperties: false
        ",
        "args.properties.limit: `additionalProperties` applies to an `object` node, and this \
         node is `integer` at line 8 column 7",
        (9, 7),
    );
    for (kind, word) in [
        ("string", "string"),
        ("boolean", "boolean"),
        ("integer", "integer"),
        ("number", "number"),
        ("array, items: { type: string }", "array"),
    ] {
        for (keyword, written) in [
            ("properties", "properties: {}"),
            ("required", "required: []"),
            ("additionalProperties", "additionalProperties: true"),
        ] {
            assert_field_refused(
                &format!("{{ type: {kind}, {written} }}"),
                &format!("`{keyword}` applies to an `object` node, and this node is `{word}`"),
            );
        }
    }
}

#[test]
fn an_enum_is_refused_on_a_number_a_boolean_an_array_and_an_object() {
    assert_refused(
        "
        args:
          type: object
          properties:
            ratio:
              type: number
              enum: [0.5, 1]
        ",
        "args.properties.ratio: `enum` applies to a `string` or an `integer` node, and this node \
         is `number` at line 8 column 7",
        (9, 7),
    );
    for (kind, word) in [
        ("number", "number"),
        ("boolean", "boolean"),
        ("array, items: { type: string }", "array"),
        ("object", "object"),
    ] {
        assert_field_refused(
            &format!("{{ type: {kind}, enum: [a] }}"),
            &format!(
                "`enum` applies to a `string` or an `integer` node, and this node is `{word}`"
            ),
        );
    }
}

#[test]
fn an_array_with_no_items_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              description: The areas to cover
        ",
        "args.properties.focus: an `array` node needs `items` at line 8 column 7",
        (9, 7),
    );
}

#[test]
fn a_keyword_written_twice_is_refused_where_its_node_opens() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              type: string
              description: What to search for
              type: integer
        ",
        "args.properties.query: keyword `type` is written twice at line 8 column 7",
        (9, 7),
    );
    for (keyword, node) in [
        ("type", "{ type: string, type: string }"),
        (
            "description",
            "{ type: string, description: a, description: b }",
        ),
        ("default", "{ type: string, default: a, default: b }"),
        ("enum", "{ type: string, enum: [a], enum: [b] }"),
        (
            "properties",
            "{ type: object, properties: {}, properties: {} }",
        ),
        ("required", "{ type: object, required: [], required: [] }"),
        (
            "additionalProperties",
            "{ type: object, additionalProperties: true, additionalProperties: true }",
        ),
        (
            "items",
            "{ type: array, items: { type: string }, items: { type: string } }",
        ),
    ] {
        assert_field_refused(node, &format!("keyword `{keyword}` is written twice"));
    }
}

#[test]
fn required_names_only_declared_fields_and_each_one_once() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
          required: [query, limit]
        ",
        "args: `required` names `limit`, which `properties` does not declare at line 5 column 3",
        (6, 3),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
          required: [query, query]
        ",
        "args: `required` names `query` twice at line 5 column 3",
        (6, 3),
    );
    assert_refused(
        "
        args:
          type: object
          required: [query]
        ",
        "args: `required` names `query`, which `properties` does not declare at line 5 column 3",
        (6, 3),
    );
    assert_field_refused(
        "{ type: object, properties: { size: { type: integer } }, required: [sort] }",
        "`required` names `sort`, which `properties` does not declare",
    );
}

#[test]
fn an_enum_lists_at_least_one_value_and_each_value_once() {
    assert_refused(
        "
        args:
          type: object
          properties:
            depth:
              type: string
              enum: [quick, full, quick]
        ",
        "args.properties.depth: `enum` lists \"quick\" twice at line 8 column 7",
        (9, 7),
    );
    assert_field_refused("{ type: integer, enum: [1, 2, 1] }", "`enum` lists 1 twice");
    assert_field_refused("{ type: string, enum: [] }", "`enum` lists no value");
    assert_field_refused("{ type: integer, enum: [] }", "`enum` lists no value");
}

#[test]
fn an_enum_value_of_another_kind_than_its_node_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            depth:
              type: string
              enum: [quick, 5]
        ",
        "args.properties.depth: the `enum` of a `string` node lists strings, and it lists an \
         integer at line 8 column 7",
        (9, 7),
    );
    for (value, kind) in [
        ("5", "an integer"),
        ("1.5", "a number"),
        ("true", "a boolean"),
        ("~", "a null"),
        ("[a]", "a sequence"),
        ("{ a: 1 }", "a mapping"),
    ] {
        assert_field_refused(
            &format!("{{ type: string, enum: [quick, {value}] }}"),
            &format!("the `enum` of a `string` node lists strings, and it lists {kind}"),
        );
    }
    for (value, kind) in [
        ("two", "a string"),
        ("1.5", "a number"),
        ("true", "a boolean"),
        ("~", "a null"),
        ("[2]", "a sequence"),
        ("{ a: 2 }", "a mapping"),
    ] {
        assert_field_refused(
            &format!("{{ type: integer, enum: [1, {value}] }}"),
            &format!("the `enum` of an `integer` node lists integers, and it lists {kind}"),
        );
    }
    // An integer past 64 bits is named by its value.
    assert_field_refused(
        "{ type: integer, enum: [1, 18446744073709551615] }",
        "the `enum` of an `integer` node lists integers that fit in 64 bits, and it lists \
         18446744073709551615",
    );
}

#[test]
fn a_default_of_another_type_than_its_node_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            limit:
              type: integer
              default: three
        ",
        "args.properties.limit: the default must be an integer, got string at line 8 column 7",
        (9, 7),
    );
    for (node, refusal) in [
        (
            "{ type: string, default: 42 }",
            "the default must be a string, got integer",
        ),
        (
            "{ type: boolean, default: 'true' }",
            "the default must be a boolean, got string",
        ),
        (
            "{ type: integer, default: 1.5 }",
            "the default must be an integer, got number",
        ),
        (
            "{ type: number, default: [1] }",
            "the default must be a number, got array",
        ),
        (
            "{ type: array, items: { type: string }, default: a }",
            "the default must be an array, got string",
        ),
        (
            "{ type: object, default: [] }",
            "the default must be an object, got array",
        ),
        (
            "{ type: integer, default: 18446744073709551615 }",
            "the default must be an integer that fits in 64 bits, got 18446744073709551615",
        ),
    ] {
        assert_field_refused(node, refusal);
    }
}

#[test]
fn a_default_outside_its_enum_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            depth:
              type: string
              enum: [quick, full]
              default: slow
        ",
        "args.properties.depth: the default must be one of \"quick\", \"full\", got \"slow\" at \
         line 8 column 7",
        (9, 7),
    );
    assert_field_refused(
        "{ type: integer, enum: [1, 2], default: 3 }",
        "the default must be one of 1, 2, got 3",
    );
}

#[test]
fn an_object_or_list_default_is_checked_against_what_its_node_declares() {
    // A value inside the default is named by its path from the default.
    assert_refused(
        "
        args:
          type: object
          properties:
            page:
              type: object
              properties:
                limit: { type: integer }
              default: { limit: many }
        ",
        "args.properties.page: the default's `limit` must be an integer, got string at line 8 \
         column 7",
        (9, 7),
    );
    for (node, refusal) in [
        (
            "{ type: object, properties: { size: { type: integer } }, default: { n: 1 } }",
            "the default's `n` is not declared; the declared fields of this node are `size`",
        ),
        (
            "{ type: object, default: { n: 1 } }",
            "the default's `n` is not declared; this node declares no fields",
        ),
        (
            "{ type: object, properties: { size: { type: integer } }, required: [size], \
             default: {} }",
            "the default's `size` is required",
        ),
        (
            "{ type: object, properties: { size: { type: integer } }, required: [size], \
             default: { size: ~ } }",
            "the default's `size` is required, got null",
        ),
        (
            "{ type: array, items: { type: string }, default: [a, 5] }",
            "the default's `[1]` must be a string, got integer",
        ),
        (
            "{ type: object, properties: { tags: { type: array, items: { type: string } } }, \
             default: { tags: [a, ~] } }",
            "the default's `tags[1]` must be a string, got null",
        ),
        (
            "{ type: object, properties: { sort: { type: object, properties: { by: { type: \
             string } } } }, default: { sort: { desc: true } } }",
            "the default's `sort.desc` is not declared; the declared fields of `sort` are `by`",
        ),
    ] {
        assert_field_refused(node, refusal);
    }
}

#[test]
fn a_default_holding_a_number_that_is_not_finite_or_a_tagged_value_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            ratio:
              type: number
              default: .inf
        ",
        "args.properties.ratio: the default holds a number that is not finite at line 8 column 7",
        (9, 7),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            token:
              type: string
              default: !secret hidden
        ",
        "args.properties.token: the default holds a tagged value at line 8 column 7",
        (9, 7),
    );
    for (node, refusal) in [
        (
            "{ type: number, default: -.inf }",
            "the default holds a number that is not finite",
        ),
        (
            "{ type: number, default: .nan }",
            "the default holds a number that is not finite",
        ),
        (
            "{ type: object, additionalProperties: true, default: { ratio: [.inf] } }",
            "the default holds a number that is not finite",
        ),
        (
            "{ type: array, items: { type: number }, default: [1, !x 2] }",
            "the default holds a tagged value",
        ),
        (
            "{ type: object, additionalProperties: true, default: { 1: a } }",
            "the default holds a key that is not a string",
        ),
    ] {
        assert_field_refused(node, refusal);
    }
}

#[test]
fn an_argument_name_outside_the_name_grammar_is_refused_where_properties_opens() {
    // The refusal is placed where the `properties` map opens, at its first
    // name, whichever name it is about.
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
            1bad: { type: string }
        ",
        "args.properties: invalid argument name `1bad`: expected [A-Za-z][A-Za-z0-9_-]{0,63} at \
         line 7 column 5",
        (8, 5),
    );
    let too_long = "x".repeat(65);
    for (written, name) in [
        ("'has space'", "has space"),
        ("a.b", "a.b"),
        ("_a", "_a"),
        ("''", ""),
        ("caf\u{e9}", "caf\u{e9}"),
        (too_long.as_str(), too_long.as_str()),
    ] {
        assert_refused(
            &format!("args:\n  type: object\n  properties:\n    {written}: {{ type: string }}\n"),
            &format!(
                "args.properties: invalid argument name `{name}`: expected \
                 [A-Za-z][A-Za-z0-9_-]{{0,63}} at line 7 column 5"
            ),
            (8, 5),
        );
    }
}

#[test]
fn an_argument_name_may_be_a_lua_keyword_or_the_name_of_a_global() {
    // A name is a field of the `args` object, so any name of the grammar
    // is declared like any other, up to 64 characters.
    let longest = "x".repeat(64);
    let schema = published(&format!(
        "args: {{ type: object, properties: {{ end: {{ type: boolean }}, store: {{ type: string \
         }}, args: {{ type: string }}, a_b-c9: {{ type: string }}, {longest}: {{ type: string }} \
         }} }}"
    ));
    let names: Vec<&str> = schema["properties"]
        .as_object()
        .expect("an object publishes its properties")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, ["a_b-c9", "args", "end", "store", longest.as_str()]);
}

#[test]
fn an_argument_declared_twice_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
            limit: { type: integer }
            query: { type: integer }
        ",
        "args.properties: duplicate argument name `query`: contract map keys must be unique at \
         line 7 column 5",
        (8, 5),
    );
}

#[test]
fn an_args_value_that_is_a_scalar_or_a_sequence_is_refused() {
    for (written, found) in [
        ("5", "integer `5`"),
        ("text", "string \"text\""),
        ("true", "boolean `true`"),
        ("~", "unit value"),
        ("[query, limit]", "sequence"),
    ] {
        assert_refused(
            &format!("args: {written}"),
            &format!(
                "args: invalid type: {found}, expected a map of schema keywords at line 4 column 7"
            ),
            (5, 7),
        );
    }
    assert_refused(
        "
        args:
          - query
          - limit
        ",
        "args: invalid type: sequence, expected a map of schema keywords at line 5 column 3",
        (6, 3),
    );
}

#[test]
fn an_args_key_with_no_value_is_a_node_with_no_type() {
    // Only a prompt with no `args:` key gets the default declaration.
    assert_refused(
        "args:",
        &format!("args: {NEEDS_TYPE} at line 4 column 6"),
        (5, 6),
    );
}

#[test]
fn additional_properties_that_is_not_a_boolean_is_refused() {
    assert_refused(
        "
        args:
          type: object
          additionalProperties: 5
        ",
        "args.additionalProperties: invalid type: integer `5`, expected a boolean at line 6 \
         column 25",
        (7, 25),
    );
    // JSON Schema also takes a schema here, and the subset takes `true` or
    // `false` alone.
    for (written, found) in [
        ("{ type: string }", "map"),
        ("yes", "string \"yes\""),
        ("[true]", "sequence"),
    ] {
        assert_refused(
            &format!("args:\n  type: object\n  additionalProperties: {written}\n"),
            &format!(
                "args.additionalProperties: invalid type: {found}, expected a boolean at line 6 \
                 column 25"
            ),
            (7, 25),
        );
    }
}

#[test]
fn required_that_is_not_a_list_of_strings_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
          required: query
        ",
        "args.required: invalid type: string \"query\", expected a sequence at line 8 column 13",
        (9, 13),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
          required:
            - query
            - [limit]
        ",
        "args.required[1]: invalid type: sequence, expected a string at line 10 column 7",
        (11, 7),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            query: { type: string }
          required: { query: true }
        ",
        "args.required: invalid type: map, expected a sequence at line 8 column 13",
        (9, 13),
    );
}

#[test]
fn a_description_that_is_not_a_string_is_refused() {
    assert_refused(
        "
        args:
          type: object
          properties:
            query:
              type: string
              description: [What, to, search, for]
        ",
        "args.properties.query.description: invalid type: sequence, expected a string at line 9 \
         column 20",
        (10, 20),
    );
    assert_refused(
        "
        args:
          type: object
          description:
            en: What a search takes
        ",
        "args.description: invalid type: map, expected a string at line 7 column 5",
        (8, 5),
    );
}

#[test]
fn a_node_or_a_keyword_value_of_the_wrong_yaml_kind_is_refused_at_the_value() {
    assert_refused(
        "
        args:
          type: object
          properties: [query]
        ",
        "args.properties: invalid type: sequence, expected a map of argument name keys to \
         declarations at line 6 column 15",
        (7, 15),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            query: string
        ",
        "args.properties.query: invalid type: string \"string\", expected a map of schema \
         keywords at line 7 column 12",
        (8, 12),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            focus:
              type: array
              items: string
        ",
        "args.properties.focus.items: invalid type: string \"string\", expected a map of schema \
         keywords at line 9 column 14",
        (10, 14),
    );
    assert_refused(
        "
        args:
          type: object
          properties:
            depth:
              type: string
              enum: quick
        ",
        "args.properties.depth.enum: invalid type: string \"quick\", expected a sequence at line \
         9 column 13",
        (10, 13),
    );
}
