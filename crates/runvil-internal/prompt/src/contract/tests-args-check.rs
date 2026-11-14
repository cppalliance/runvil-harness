//! Tests for the check of a caller's arguments against the `args`
//! declaration: the object a run holds, with each default filled, and each
//! refusal with its message, prose, facts, and no location.

use std::collections::BTreeMap;

use runvil_types::error::Error;
use serde_json::Value as Json;

use super::*;
use crate::Prompt;
use crate::detail::accept_args;

/// A required string, an integer with a default, and a string with an
/// `enum`.
const SEARCH: &str = "
    args:
      type: object
      properties:
        query: { type: string, description: What to search for }
        limit: { type: integer, default: 3 }
        depth: { type: string, enum: [quick, full] }
      required: [query]
";

/// A nested object, a list of strings, a list of objects, and a list of
/// lists, none of them required.
const NESTED: &str = "
    args:
      type: object
      properties:
        opts:
          type: object
          properties:
            depth: { type: integer, default: 2 }
            mode: { type: string }
        focus: { type: array, items: { type: string } }
        people:
          type: array
          items:
            type: object
            properties:
              name: { type: string }
              role: { type: string, default: guest }
            required: [name]
        grid: { type: array, items: { type: array, items: { type: integer } } }
";

/// The prompt `t` whose frontmatter ends with the YAML block `front`.
#[track_caller]
fn declaring(front: &str) -> Prompt {
    parse_ok(&block_prompt(front))
}

/// The object a run of `prompt` holds for the arguments `text`.
#[track_caller]
fn accepted(prompt: &Prompt, text: &str) -> Json {
    match accept_args(prompt, text) {
        Ok(args) => args,
        Err(error) => panic!("expected `{text}` to pass, got {error:?}"),
    }
}

/// The failure of the arguments `text` for a run of `prompt`.
#[track_caller]
fn refused(prompt: &Prompt, text: &str) -> Error {
    match accept_args(prompt, text) {
        Ok(args) => panic!("expected `{text}` to be refused, got {args}"),
        Err(error) => error,
    }
}

/// Asserts that a run of `prompt` holds `held`, as compact JSON text, for
/// the arguments `text`.
#[track_caller]
fn assert_held(prompt: &Prompt, text: &str, held: &str) {
    assert_eq!(accepted(prompt, text).to_string(), held, "{text}");
}

/// Asserts that `prompt` refuses the arguments `text` about the argument
/// `name`: an `ArgsInvalid` failure with `message`, the `Reason` fact
/// `reason`, the `Name` fact `name`, no location, and the prose that is
/// the message and then the code's default.
#[track_caller]
fn assert_refusal(prompt: &Prompt, text: &str, reason: &str, name: &str, message: &str) {
    let error = refused(prompt, text);
    assert_eq!(error.code, ErrorCode::ArgsInvalid, "{text}");
    assert_eq!(error.message, message, "{text}");
    assert_eq!(
        error.prose,
        format!("{message}. {}", ErrorCode::ArgsInvalid.prose()),
        "{text}"
    );
    assert_eq!(
        error.facts,
        BTreeMap::from([
            (ErrorFact::Reason, reason.into()),
            (ErrorFact::Name, name.into())
        ]),
        "{text}"
    );
    assert_eq!(error.at, None, "{text}");
}

#[test]
fn text_that_is_not_json_is_refused_with_the_json_parsers_cause() {
    let prompt = declaring(SEARCH);
    for text in ["papers", "", r#"{"query":"#, r#"{"query":"papers"} x"#] {
        let cause = serde_json::from_str::<Json>(text)
            .expect_err("the text is not JSON")
            .to_string();
        let error = refused(&prompt, text);
        assert_eq!(error.code, ErrorCode::ArgsInvalid, "{text}");
        assert_eq!(
            error.message,
            format!("the arguments are not valid JSON: {cause}"),
            "{text}"
        );
        // The parser's cause joins the message alone, so the prose holds
        // the message's own words and then the code's default.
        assert_eq!(
            error.prose,
            format!(
                "the arguments are not valid JSON. {}",
                ErrorCode::ArgsInvalid.prose()
            ),
            "{text}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([(ErrorFact::Reason, "not_json".into())]),
            "no argument is named: {text}"
        );
        assert_eq!(error.at, None, "{text}");
    }
}

#[test]
fn json_that_is_not_an_object_is_refused_naming_its_kind() {
    let prompt = declaring(SEARCH);
    for (text, kind) in [
        ("5", "integer"),
        ("5.5", "number"),
        (r#""x""#, "string"),
        ("[1]", "array"),
        ("true", "boolean"),
        ("null", "null"),
    ] {
        let message = format!("the arguments must be a JSON object, got {kind}");
        let error = refused(&prompt, text);
        assert_eq!(error.code, ErrorCode::ArgsInvalid, "{text}");
        assert_eq!(error.message, message, "{text}");
        assert_eq!(
            error.prose,
            format!("{message}. {}", ErrorCode::ArgsInvalid.prose()),
            "{text}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([(ErrorFact::Reason, "not_an_object".into())]),
            "no argument is named: {text}"
        );
        assert_eq!(error.at, None, "{text}");
    }
}

#[test]
fn each_type_takes_its_own_json_kind_and_refuses_every_other() {
    // Each JSON value, with the kind a refusal names it by.
    let values = [
        (r#""x""#, "string"),
        ("true", "boolean"),
        ("5", "integer"),
        ("5.5", "number"),
        ("[]", "array"),
        ("{}", "object"),
    ];
    // Each type's node, what a refusal says a value must be, and the kinds
    // the type takes.
    let types: [(&str, &str, &[&str]); 6] = [
        ("{ type: string }", "a string", &["string"]),
        ("{ type: boolean }", "a boolean", &["boolean"]),
        ("{ type: integer }", "an integer", &["integer"]),
        ("{ type: number }", "a number", &["integer", "number"]),
        (
            "{ type: array, items: { type: string } }",
            "an array",
            &["array"],
        ),
        ("{ type: object }", "an object", &["object"]),
    ];
    for (node, expected, taken) in types {
        let prompt = declaring(&format!(
            "args: {{ type: object, properties: {{ value: {node} }} }}"
        ));
        for (value, kind) in values {
            let text = format!(r#"{{"value":{value}}}"#);
            if taken.contains(&kind) {
                assert_held(&prompt, &text, &text);
            } else {
                assert_refusal(
                    &prompt,
                    &text,
                    "type",
                    "value",
                    &format!("argument `value` must be {expected}, got {kind}"),
                );
            }
        }
    }
}

#[test]
fn a_missing_field_takes_its_default_and_one_with_no_default_stays_absent() {
    let prompt = declaring(SEARCH);
    // `limit` takes its default, and `depth`, which declares none, is not
    // added.
    assert_held(
        &prompt,
        r#"{"query":"papers"}"#,
        r#"{"limit":3,"query":"papers"}"#,
    );
}

#[test]
fn a_sent_value_is_kept_where_the_field_has_a_default() {
    let prompt = declaring(SEARCH);
    assert_held(
        &prompt,
        r#"{"query":"papers","limit":7,"depth":"full"}"#,
        r#"{"depth":"full","limit":7,"query":"papers"}"#,
    );
}

#[test]
fn a_required_field_left_out_is_refused() {
    assert_refusal(
        &declaring(SEARCH),
        "{}",
        "required",
        "query",
        "argument `query` is required",
    );
    assert_refusal(
        &declaring(NESTED),
        r#"{"people":[{"name":"ada"},{}]}"#,
        "required",
        "people[1].name",
        "argument `people[1].name` is required",
    );
}

#[test]
fn a_null_for_a_required_field_is_refused() {
    assert_refusal(
        &declaring(SEARCH),
        r#"{"query":null}"#,
        "required",
        "query",
        "argument `query` is required, got null",
    );
    assert_refusal(
        &declaring(NESTED),
        r#"{"people":[{"name":null}]}"#,
        "required",
        "people[0].name",
        "argument `people[0].name` is required, got null",
    );
}

#[test]
fn a_required_field_with_a_default_takes_it_when_left_out_and_refuses_a_null() {
    let prompt = declaring(
        "
        args:
          type: object
          properties:
            limit: { type: integer, default: 3 }
          required: [limit]
        ",
    );
    assert_held(&prompt, "{}", r#"{"limit":3}"#);
    assert_held(&prompt, r#"{"limit":7}"#, r#"{"limit":7}"#);
    assert_refusal(
        &prompt,
        r#"{"limit":null}"#,
        "required",
        "limit",
        "argument `limit` is required, got null",
    );
}

#[test]
fn an_undeclared_field_is_refused_listing_the_declared_arguments() {
    let prompt = declaring(SEARCH);
    let message =
        "argument `n` is not declared; the declared arguments are `depth`, `limit`, `query`";
    assert_refusal(
        &prompt,
        r#"{"query":"papers","n":2}"#,
        "undeclared",
        "n",
        message,
    );
    // A null sent for a field the declaration does not list is a field
    // all the same.
    assert_refusal(
        &prompt,
        r#"{"query":"papers","n":null}"#,
        "undeclared",
        "n",
        message,
    );
}

#[test]
fn an_undeclared_field_is_refused_by_a_declaration_with_no_fields() {
    let prompt = declaring("args: { type: object }");
    assert_held(&prompt, "{}", "{}");
    assert_refusal(
        &prompt,
        r#"{"n":2}"#,
        "undeclared",
        "n",
        "argument `n` is not declared; the prompt declares no arguments",
    );
}

#[test]
fn an_undeclared_field_inside_a_nested_object_is_named_by_its_path() {
    let prompt = declaring(NESTED);
    assert_refusal(
        &prompt,
        r#"{"opts":{"n":1}}"#,
        "undeclared",
        "opts.n",
        "argument `opts.n` is not declared; the declared fields of `opts` are `depth`, `mode`",
    );
    assert_refusal(
        &prompt,
        r#"{"people":[{"name":"ada","age":36}]}"#,
        "undeclared",
        "people[0].age",
        "argument `people[0].age` is not declared; the declared fields of `people[0]` are \
         `name`, `role`",
    );
    let bare = declaring("args: { type: object, properties: { opts: { type: object } } }");
    assert_refusal(
        &bare,
        r#"{"opts":{"n":1}}"#,
        "undeclared",
        "opts.n",
        "argument `opts.n` is not declared; `opts` declares no fields",
    );
}

#[test]
fn an_open_object_keeps_the_fields_it_does_not_declare_a_null_among_them() {
    let prompt = declaring(
        "
        args:
          type: object
          additionalProperties: true
          properties:
            query: { type: string }
            extras:
              type: object
              additionalProperties: true
              properties:
                kept: { type: string, default: all }
        ",
    );
    assert_held(
        &prompt,
        r#"{"query":"papers","note":null,"extras":{"n":1,"gone":null}}"#,
        r#"{"extras":{"gone":null,"kept":"all","n":1},"note":null,"query":"papers"}"#,
    );
    // The value of such a field is kept whole, a null inside it included.
    let nested = r#"{"tags":["a",null,{"x":null}]}"#;
    assert_held(&prompt, nested, nested);
    // A null for a field the open object declares still counts as the
    // field left out.
    assert_held(
        &prompt,
        r#"{"extras":{"kept":null,"gone":null}}"#,
        r#"{"extras":{"gone":null,"kept":"all"}}"#,
    );
    // An open object checks the fields it declares.
    assert_refusal(
        &prompt,
        r#"{"query":5,"note":1}"#,
        "type",
        "query",
        "argument `query` must be a string, got integer",
    );
    assert_refusal(
        &prompt,
        r#"{"extras":{"kept":5,"n":1}}"#,
        "type",
        "extras.kept",
        "argument `extras.kept` must be a string, got integer",
    );
}

#[test]
fn a_string_enum_takes_only_the_values_it_lists() {
    let prompt = declaring(SEARCH);
    for depth in ["quick", "full"] {
        assert_held(
            &prompt,
            &format!(r#"{{"query":"papers","depth":"{depth}"}}"#),
            &format!(r#"{{"depth":"{depth}","limit":3,"query":"papers"}}"#),
        );
    }
    for sent in ["slow", "Quick", ""] {
        assert_refusal(
            &prompt,
            &format!(r#"{{"query":"papers","depth":"{sent}"}}"#),
            "enum",
            "depth",
            &format!(r#"argument `depth` must be one of "quick", "full", got "{sent}""#),
        );
    }
}

#[test]
fn a_refused_long_string_is_cut_after_40_characters() {
    let prompt = declaring(SEARCH);
    let forty = "0123456789".repeat(4);
    let accented = "\u{e9}".repeat(40);
    for (sent, shown) in [
        // A string of 40 characters is shown whole.
        (forty.clone(), forty.clone()),
        // A longer one is cut after its first 40 characters, which count
        // as characters, not as bytes.
        (format!("{forty}x"), format!("{forty}...")),
        (format!("{forty} and far more"), format!("{forty}...")),
        (format!("{accented}\u{e9}"), format!("{accented}...")),
    ] {
        assert_refusal(
            &prompt,
            &format!(r#"{{"query":"papers","depth":"{sent}"}}"#),
            "enum",
            "depth",
            &format!(r#"argument `depth` must be one of "quick", "full", got "{shown}""#),
        );
    }
}

#[test]
fn an_integer_enum_takes_only_the_values_it_lists() {
    let prompt = declaring(
        "args: { type: object, properties: { level: { type: integer, enum: [1, 2, 3] } } }",
    );
    assert_held(&prompt, r#"{"level":2}"#, r#"{"level":2}"#);
    // A whole number is the integer it equals.
    assert_held(&prompt, r#"{"level":2.0}"#, r#"{"level":2}"#);
    for (sent, shown) in [("5", "5"), ("5.0", "5"), ("-1", "-1")] {
        assert_refusal(
            &prompt,
            &format!(r#"{{"level":{sent}}}"#),
            "enum",
            "level",
            &format!("argument `level` must be one of 1, 2, 3, got {shown}"),
        );
    }
}

#[test]
fn a_mismatch_inside_a_nested_object_or_a_list_is_named_by_its_path() {
    let prompt = declaring(NESTED);
    for (text, name, said) in [
        (
            r#"{"opts":{"depth":"deep"}}"#,
            "opts.depth",
            "must be an integer, got string",
        ),
        // A list index counts from 0.
        (
            r#"{"focus":[5]}"#,
            "focus[0]",
            "must be a string, got integer",
        ),
        (
            r#"{"focus":["a",5]}"#,
            "focus[1]",
            "must be a string, got integer",
        ),
        (
            r#"{"people":[{"name":"ada"},{"name":7}]}"#,
            "people[1].name",
            "must be a string, got integer",
        ),
        (
            r#"{"grid":[[1,2],[3,"x"]]}"#,
            "grid[1][1]",
            "must be an integer, got string",
        ),
    ] {
        assert_refusal(
            &prompt,
            text,
            "type",
            name,
            &format!("argument `{name}` {said}"),
        );
    }
}

#[test]
fn a_whole_number_for_an_integer_field_comes_back_as_an_integer() {
    let prompt = declaring(SEARCH);
    for (sent, held) in [
        ("5.0", "5"),
        ("1e3", "1000"),
        ("-2.0", "-2"),
        ("-0.0", "0"),
        ("5", "5"),
    ] {
        assert_held(
            &prompt,
            &format!(r#"{{"query":"papers","limit":{sent}}}"#),
            &format!(r#"{{"limit":{held},"query":"papers"}}"#),
        );
    }
    // The same holds inside a nested object and inside a list.
    assert_held(
        &declaring(NESTED),
        r#"{"grid":[[1.0,2e0]],"opts":{"depth":3.0}}"#,
        r#"{"grid":[[1,2]],"opts":{"depth":3}}"#,
    );
}

#[test]
fn a_number_with_a_fraction_is_refused_for_an_integer_field() {
    let prompt = declaring(SEARCH);
    for sent in ["5.5", "1e-1", "-0.5"] {
        assert_refusal(
            &prompt,
            &format!(r#"{{"query":"papers","limit":{sent}}}"#),
            "type",
            "limit",
            "argument `limit` must be an integer, got number",
        );
    }
}

#[test]
fn an_integer_past_64_bits_is_refused() {
    let prompt = declaring(SEARCH);
    // The refusal shows the number as the JSON parser read it, so one sent
    // with an exponent is shown in the form the parser writes.
    for (sent, shown) in [
        ("18446744073709551615", "18446744073709551615"),
        ("9223372036854775808", "9223372036854775808"),
        ("1e300", "1e+300"),
        ("-1e30", "-1e+30"),
        // One past the smallest integer reads as a float equal to it.
        ("-9223372036854775809", "-9.223372036854776e+18"),
    ] {
        assert_refusal(
            &prompt,
            &format!(r#"{{"query":"papers","limit":{sent}}}"#),
            "type",
            "limit",
            &format!("argument `limit` must be an integer that fits in 64 bits, got {shown}"),
        );
    }
    // The largest and the smallest integer 64 bits hold pass.
    for sent in ["9223372036854775807", "-9223372036854775808"] {
        assert_held(
            &prompt,
            &format!(r#"{{"query":"papers","limit":{sent}}}"#),
            &format!(r#"{{"limit":{sent},"query":"papers"}}"#),
        );
    }
}

#[test]
fn a_number_field_takes_any_number_and_keeps_it_as_sent() {
    let prompt = declaring("args: { type: object, properties: { ratio: { type: number } } }");
    for sent in [
        "5",
        "5.0",
        "-0.5",
        "1e300",
        "18446744073709551615",
        "-9223372036854775808",
    ] {
        let text = format!(r#"{{"ratio":{sent}}}"#);
        let parsed: Json = serde_json::from_str(&text).expect("the arguments are JSON");
        // JSON values compare an integer and a number with a fraction or
        // an exponent as different, so each stays what the caller sent.
        assert_eq!(accepted(&prompt, &text), parsed, "{text}");
    }
    assert_held(&prompt, r#"{"ratio":5}"#, r#"{"ratio":5}"#);
    assert_held(&prompt, r#"{"ratio":5.0}"#, r#"{"ratio":5.0}"#);
}

#[test]
fn a_default_is_filled_inside_a_nested_object_the_caller_sent() {
    let prompt = declaring(NESTED);
    assert_held(&prompt, r#"{"opts":{}}"#, r#"{"opts":{"depth":2}}"#);
    assert_held(
        &prompt,
        r#"{"opts":{"mode":"fast"}}"#,
        r#"{"opts":{"depth":2,"mode":"fast"}}"#,
    );
}

#[test]
fn a_default_is_filled_inside_each_object_item_of_a_list() {
    let prompt = declaring(NESTED);
    assert_held(
        &prompt,
        r#"{"people":[{"name":"ada"},{"name":"bob","role":"chair"},{"name":"cy"}]}"#,
        concat!(
            r#"{"people":[{"name":"ada","role":"guest"},{"name":"bob","role":"chair"},"#,
            r#"{"name":"cy","role":"guest"}]}"#,
        ),
    );
}

#[test]
fn an_object_default_arrives_with_the_defaults_of_its_own_fields() {
    let prompt = declaring(
        "
        args:
          type: object
          properties:
            page:
              type: object
              properties:
                size: { type: integer, default: 20 }
                sort: { type: string }
              default: { sort: date }
        ",
    );
    assert_held(&prompt, "{}", r#"{"page":{"size":20,"sort":"date"}}"#);
    // An object the caller sent stands in place of the whole default, and
    // its own missing fields are filled.
    assert_held(&prompt, r#"{"page":{}}"#, r#"{"page":{"size":20}}"#);
}

#[test]
fn an_object_left_out_stays_absent_when_it_declares_no_default() {
    // `opts.depth` declares a default, which is filled inside an `opts`
    // that is there. `opts` declares none, so a caller that leaves it out
    // gets no `opts`.
    assert_held(&declaring(NESTED), "{}", "{}");
}

#[test]
fn a_null_for_a_field_that_is_not_required_counts_as_left_out() {
    // `limit` takes its default, and `depth`, which declares none, is
    // absent.
    assert_held(
        &declaring(SEARCH),
        r#"{"query":"papers","limit":null,"depth":null}"#,
        r#"{"limit":3,"query":"papers"}"#,
    );
    let nested = declaring(NESTED);
    assert_held(
        &nested,
        r#"{"opts":{"depth":null,"mode":null}}"#,
        r#"{"opts":{"depth":2}}"#,
    );
    assert_held(&nested, r#"{"opts":null,"focus":null}"#, "{}");
}

#[test]
fn a_null_inside_a_list_is_refused() {
    let prompt = declaring(NESTED);
    for (text, name, said) in [
        (
            r#"{"focus":["a",null]}"#,
            "focus[1]",
            "must be a string, got null",
        ),
        (
            r#"{"people":[null]}"#,
            "people[0]",
            "must be an object, got null",
        ),
        (
            r#"{"grid":[[1,null]]}"#,
            "grid[0][1]",
            "must be an integer, got null",
        ),
        (
            r#"{"grid":[null]}"#,
            "grid[0]",
            "must be an array, got null",
        ),
    ] {
        assert_refusal(
            &prompt,
            text,
            "type",
            name,
            &format!("argument `{name}` {said}"),
        );
    }
}

#[test]
fn the_mismatch_in_the_field_that_sorts_first_by_name_is_reported() {
    let prompt = declaring(SEARCH);
    // `depth` sorts before `limit`, whichever the caller wrote first.
    assert_refusal(
        &prompt,
        r#"{"query":"papers","limit":"many","depth":"slow"}"#,
        "enum",
        "depth",
        r#"argument `depth` must be one of "quick", "full", got "slow""#,
    );
    // `limit` sorts before `query`, which is required and missing.
    assert_refusal(
        &prompt,
        r#"{"limit":"many"}"#,
        "type",
        "limit",
        "argument `limit` must be an integer, got string",
    );
}

#[test]
fn a_missing_required_field_is_reported_before_an_undeclared_field() {
    assert_refusal(
        &declaring(SEARCH),
        r#"{"n":1}"#,
        "required",
        "query",
        "argument `query` is required",
    );
}

#[test]
fn a_declared_fields_mismatch_is_reported_before_an_undeclared_field() {
    // `a` sorts before `limit`, and the declared fields are checked first.
    assert_refusal(
        &declaring(SEARCH),
        r#"{"a":1,"query":"papers","limit":"many"}"#,
        "type",
        "limit",
        "argument `limit` must be an integer, got string",
    );
}

#[test]
fn the_undeclared_field_that_sorts_first_by_name_is_reported() {
    assert_refusal(
        &declaring(SEARCH),
        r#"{"query":"papers","zeta":1,"beta":2}"#,
        "undeclared",
        "beta",
        "argument `beta` is not declared; the declared arguments are `depth`, `limit`, `query`",
    );
}

#[test]
fn a_value_of_the_wrong_type_is_refused_for_its_type_before_its_enum() {
    assert_refusal(
        &declaring(SEARCH),
        r#"{"query":"papers","depth":5}"#,
        "type",
        "depth",
        "argument `depth` must be a string, got integer",
    );
}

#[test]
fn the_contents_of_a_field_are_checked_before_the_next_field() {
    // `opts` sorts before `people`, so the undeclared field inside `opts`
    // is reported and the wrong type of `people` is not.
    assert_refusal(
        &declaring(NESTED),
        r#"{"people":"all","opts":{"n":1}}"#,
        "undeclared",
        "opts.n",
        "argument `opts.n` is not declared; the declared fields of `opts` are `depth`, `mode`",
    );
}

#[test]
fn the_items_of_a_list_are_checked_in_index_order() {
    assert_refusal(
        &declaring(NESTED),
        r#"{"focus":["a",1,2]}"#,
        "type",
        "focus[1]",
        "argument `focus[1]` must be a string, got integer",
    );
}

#[test]
fn an_empty_object_passes_a_declaration_with_no_required_field() {
    let prompt = declaring(
        "
        args:
          type: object
          properties:
            limit: { type: integer, default: 3 }
            depth: { type: string }
        ",
    );
    assert_held(&prompt, "{}", r#"{"limit":3}"#);
}

#[test]
fn the_default_declaration_takes_an_empty_object_or_a_text() {
    let prompt = parse_ok(&prompt("", &[]));
    assert_held(&prompt, "{}", "{}");
    assert_held(&prompt, r#"{"text":"hello"}"#, r#"{"text":"hello"}"#);
    assert_held(&prompt, r#"{"text":null}"#, "{}");
    assert_refusal(
        &prompt,
        r#"{"text":5}"#,
        "type",
        "text",
        "argument `text` must be a string, got integer",
    );
    assert_refusal(
        &prompt,
        r#"{"query":"papers"}"#,
        "undeclared",
        "query",
        "argument `query` is not declared; the declared arguments are `text`",
    );
}
