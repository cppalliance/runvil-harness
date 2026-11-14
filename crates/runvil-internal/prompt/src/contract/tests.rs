//! Tests for the frontmatter contract keys: `uses`, `args`, and
//! `models`.

use std::num::NonZeroU32;

use runvil_types::error::{ErrorCode, ErrorFact};
use runvil_types::models::ModelRole;
use serde_json::json;

use crate::test_support::{assert_parse_error, parse_err, parse_ok, prompt};
use crate::tests::location;

#[path = "tests-args.rs"]
mod args;
#[path = "tests-args-check.rs"]
mod args_check;
#[path = "tests-packs.rs"]
mod packs;

/// Writes the prompt `t` whose frontmatter ends with the YAML block
/// `front`, minus the indentation every line of it shares, so a nested
/// declaration keeps its own indentation.
///
/// The block's first line is file line 5, which the YAML parser counts as
/// line 4, and a column counts from the block's left edge.
fn block_prompt(front: &str) -> String {
    let lines: Vec<&str> = front
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let shared = lines
        .iter()
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let block: String = lines
        .iter()
        .flat_map(|line| [&line[shared..], "\n"])
        .collect();
    format!("---\nname: t\ndescription: d\nrunvil: 0\n{block}---\n\n## S\n\np\n")
}

#[test]
fn the_full_contract_declaration_parses_and_round_trips() {
    let prompt = parse_ok(&prompt(
        "
        uses: [web, mcp]
        args: { type: object, properties: { use_mcp: { type: boolean, default: true, \
                description: Search MCP-connected private sources } } }
        models: { strong: { min_context: 200000, thinking: true }, \
                  fast: { thinking: false } }
        ",
        &[],
    ));

    let fm = prompt.frontmatter();

    let packs = fm.uses();
    assert_eq!(packs.len(), 2);
    assert_eq!(packs[0].id().to_string(), "web");
    assert_eq!(packs[1].id().to_string(), "mcp");

    assert_eq!(
        fm.args().as_json()["properties"]["use_mcp"],
        json!({
            "type": "boolean",
            "default": true,
            "description": "Search MCP-connected private sources",
        })
    );

    let strong = fm
        .role_requirements(ModelRole::Strong)
        .expect("the role is declared");
    assert_eq!(strong.min_context(), NonZeroU32::new(200_000));
    assert_eq!(strong.thinking(), Some(true));
    let fast = fm
        .role_requirements(ModelRole::Fast)
        .expect("the role is declared");
    assert_eq!(fast.min_context(), None);
    assert_eq!(fast.thinking(), Some(false));
}

#[test]
fn a_prompt_without_contract_keys_behaves_exactly_as_today() {
    let prompt = parse_ok(&prompt("", &[]));
    let fm = prompt.frontmatter();
    assert!(fm.uses().is_empty());
    for role in ModelRole::ALL {
        assert!(
            fm.role_requirements(role).is_none(),
            "a prompt with no models key declares nothing for {role:?}"
        );
    }
}

#[test]
fn an_unknown_key_inside_a_contract_entry_is_rejected() {
    // A misspelled key inside a contract key's entry is an authoring
    // error, never a key the parse ignores.
    for front in [
        "args: { type: object, properties: { flag: { tipe: boolean } } }",
        "models: { strong: { tier: 1 } }",
    ] {
        let error = parse_err(&prompt(front, &[]));
        assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error}");
    }
}

#[test]
fn each_role_declared_with_both_requirements_reads_them_back() {
    for role in ModelRole::ALL {
        let front = format!(
            "models: {{ {}: {{ min_context: 4096, thinking: true }} }}",
            role.as_str()
        );
        let prompt = parse_ok(&prompt(&front, &[]));
        let declared = prompt
            .frontmatter()
            .role_requirements(role)
            .expect("the role is declared");
        assert_eq!(declared.min_context(), NonZeroU32::new(4096), "{role:?}");
        assert_eq!(declared.thinking(), Some(true), "{role:?}");
    }
}

#[test]
fn an_empty_role_declares_neither_requirement() {
    let prompt = parse_ok(&prompt("models: { fast: {} }", &[]));
    let fast = prompt
        .frontmatter()
        .role_requirements(ModelRole::Fast)
        .expect("an empty entry still declares the role");
    assert_eq!(fast.min_context(), None);
    assert_eq!(fast.thinking(), None);
    assert!(
        prompt
            .frontmatter()
            .role_requirements(ModelRole::Strong)
            .is_none(),
        "a role the prompt does not name has no entry"
    );
}

#[test]
fn an_unknown_role_name_is_a_parse_error_that_lists_the_five_roles() {
    assert_parse_error(
        &prompt("models: { reviewer: {} }", &[]),
        ErrorCode::FrontmatterInvalid,
        "expected one of `default`, `strong`, `small`, `fast`, `creative`",
    );
}

#[test]
fn an_unknown_field_in_a_role_is_a_parse_error_that_names_the_two_fields() {
    assert_parse_error(
        &prompt("models: { strong: { tier: 1 } }", &[]),
        ErrorCode::FrontmatterInvalid,
        "unknown field `tier`, expected `min_context` or `thinking`",
    );
}

#[test]
fn a_duplicate_role_key_is_rejected() {
    let error = parse_err(&prompt(
        "models: { strong: { min_context: 1 }, strong: { thinking: true } }",
        &[],
    ));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error}");
}

#[test]
fn a_zero_min_context_is_rejected() {
    let error = parse_err(&prompt("models: { strong: { min_context: 0 } }", &[]));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
}

#[test]
fn contract_errors_report_their_frontmatter_line_and_column() {
    // The retained serde_yaml_ng location surfaces on the parse
    // error, so a rejection inside a contract key points at its own line
    // and column instead of being a bare message.
    let src = concat!(
        "---\n",               // line 1
        "name: x\n",           // line 2
        "description: d\n",    // line 3
        "uses:\n",             // line 4
        "  - not a Pack id\n", // line 5: the offending scalar
        "---\n",
        "\n## S\n\np\n",
    );
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    assert_eq!(
        location(&error),
        Some(("prompt", 5, 5, 5, 5)),
        "the offending scalar's line and column: {error}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        None,
        "a frontmatter failure predates the prompt's name"
    );
}
