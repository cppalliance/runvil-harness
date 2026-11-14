//! The run's first step refusing its arguments. Text that is not JSON,
//! JSON that is not an object, and an object the prompt's `args`
//! declaration does not accept each end the run at its first step with
//! the code `args_invalid`: the failure has the reason and, for one
//! argument, its path as facts, no location, and a message that names
//! what is required and what was sent, and the run reports no event. The
//! check runs after the version gate and before the store probe and the
//! requirements refusal, and a cancel flag set before the first step ends
//! the run canceled instead.

use std::collections::BTreeMap;

use runvil_prompt::test_support::{one_section, prompt};
use runvil_types::error::{Error, ErrorCode, ErrorFact};
use runvil_types::models::ThinkingMode;
use runvil_vfs::{MemoryBackend, VfsRef};
use serde_json::json;

use super::{NESTED, SEARCH, search_prompt};
use crate::test_support::scenario::{Scenario, test_model_with};

/// The body of a section a refused run never reaches: it writes
/// `ran.txt` to the store.
const NEVER_RUNS: &str = "store.write('ran.txt', 'x')\nreturn 'never runs'";

/// The failure `scenario`'s run ends with, after asserting that its first
/// step refused the arguments: the code is `ArgsInvalid`, the failure
/// points at no line, and the run reported no event, issued no effect,
/// took no model turn, and ran no section.
#[track_caller]
fn refusal(scenario: Scenario) -> Error {
    let trace = scenario.run();
    let error = trace.error().clone();
    assert_eq!(error.code, ErrorCode::ArgsInvalid, "{error:?}");
    assert_eq!(
        error.at, None,
        "the arguments are the caller's to fix, so the failure names no line: {error:?}"
    );
    assert_eq!(error.located_message(), error.message);
    assert!(
        trace.events().is_empty(),
        "a refused run reports no event: {:?}",
        trace.events()
    );
    assert!(
        trace.effects().is_empty(),
        "a refused run issues nothing: {:?}",
        trace.effects()
    );
    assert_eq!(trace.turns(), 0, "a refused run never starts");
    assert!(
        trace.read("ran.txt").is_err(),
        "a refused run runs no section"
    );
    error
}

/// Asserts that a run of `source` given `args` is refused for `reason`
/// with `message`, naming the argument `name`: the facts are the reason
/// and the name alone, and the prose is the message and then the code's
/// default text.
#[track_caller]
fn assert_argument_refused(source: &str, args: &str, reason: &str, name: &str, message: &str) {
    let error = refusal(Scenario::new(source).args(args));
    assert_eq!(error.message, message, "arguments {args}");
    assert_eq!(
        error.prose,
        format!("{message}. {}", ErrorCode::ArgsInvalid.prose()),
        "arguments {args}"
    );
    assert_eq!(
        error.facts,
        BTreeMap::from([
            (ErrorFact::Reason, json!(reason)),
            (ErrorFact::Name, json!(name)),
        ]),
        "arguments {args}"
    );
}

#[test]
fn text_that_is_not_json_ends_the_run_at_its_first_step_with_the_parsers_cause() {
    let source = search_prompt(NEVER_RUNS);
    for (args, cause) in [
        ("papers", "expected value at line 1 column 1"),
        ("", "EOF while parsing a value at line 1 column 0"),
        (
            r#"{"query":"papers""#,
            "EOF while parsing an object at line 1 column 17",
        ),
        (
            r#"{"query":"papers"} and more"#,
            "trailing characters at line 1 column 20",
        ),
    ] {
        let error = refusal(Scenario::new(&source).args(args));
        assert_eq!(
            error.message,
            format!("the arguments are not valid JSON: {cause}"),
            "arguments {args:?}"
        );
        assert_eq!(
            error.prose,
            format!(
                "the arguments are not valid JSON. {}",
                ErrorCode::ArgsInvalid.prose()
            ),
            "the parser's cause stays out of the prose: {args:?}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([(ErrorFact::Reason, json!("not_json"))]),
            "arguments {args:?}"
        );
    }
}

#[test]
fn json_that_is_not_an_object_ends_the_run_at_its_first_step_naming_its_kind() {
    let source = search_prompt(NEVER_RUNS);
    for (args, kind) in [
        ("null", "null"),
        ("true", "boolean"),
        ("5", "integer"),
        ("2.5", "number"),
        (r#""papers""#, "string"),
        (r#"["papers"]"#, "array"),
    ] {
        let error = refusal(Scenario::new(&source).args(args));
        let message = format!("the arguments must be a JSON object, got {kind}");
        assert_eq!(error.message, message, "arguments {args}");
        assert_eq!(
            error.prose,
            format!("{message}. {}", ErrorCode::ArgsInvalid.prose()),
            "arguments {args}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([(ErrorFact::Reason, json!("not_an_object"))]),
            "arguments {args}"
        );
    }
}

#[test]
fn an_argument_the_declaration_refuses_ends_the_run_with_its_reason_and_its_name() {
    let source = search_prompt(NEVER_RUNS);
    for (args, reason, name, message) in [
        ("{}", "required", "query", "argument `query` is required"),
        (
            r#"{"query":null}"#,
            "required",
            "query",
            "argument `query` is required, got null",
        ),
        (
            r#"{"query":5}"#,
            "type",
            "query",
            "argument `query` must be a string, got integer",
        ),
        (
            r#"{"query":"papers","limit":2.5}"#,
            "type",
            "limit",
            "argument `limit` must be an integer, got number",
        ),
        (
            r#"{"query":"papers","depth":"deep"}"#,
            "enum",
            "depth",
            r#"argument `depth` must be one of "quick", "full", got "deep""#,
        ),
        (
            r#"{"query":"papers","n":2}"#,
            "undeclared",
            "n",
            "argument `n` is not declared; the declared arguments are `depth`, `limit`, `query`",
        ),
    ] {
        assert_argument_refused(&source, args, reason, name, message);
    }
}

#[test]
fn a_refused_argument_inside_an_object_or_a_list_is_named_by_its_path() {
    let source = prompt(NESTED, &[("Only", NEVER_RUNS)]);
    for (args, reason, name, message) in [
        (
            r#"{"opts":{"depth":"deep"}}"#,
            "type",
            "opts.depth",
            "argument `opts.depth` must be an integer, got string",
        ),
        (
            r#"{"opts":{"width":1}}"#,
            "undeclared",
            "opts.width",
            "argument `opts.width` is not declared; the declared fields of `opts` are `depth`",
        ),
        (
            r#"{"focus":["intro",7]}"#,
            "type",
            "focus[1]",
            "argument `focus[1]` must be a string, got integer",
        ),
        (
            r#"{"focus":[null]}"#,
            "type",
            "focus[0]",
            "argument `focus[0]` must be a string, got null",
        ),
        (
            r#"{"focus":"intro"}"#,
            "type",
            "focus",
            "argument `focus` must be an array, got string",
        ),
    ] {
        assert_argument_refused(&source, args, reason, name, message);
    }
}

#[test]
fn a_prompt_with_no_args_key_refuses_every_argument_but_a_string_named_text() {
    let source = one_section(NEVER_RUNS);
    assert_argument_refused(
        &source,
        r#"{"query":1}"#,
        "undeclared",
        "query",
        "argument `query` is not declared; the declared arguments are `text`",
    );
    assert_argument_refused(
        &source,
        r#"{"text":5}"#,
        "type",
        "text",
        "argument `text` must be a string, got integer",
    );
}

#[test]
fn a_declaration_of_no_fields_refuses_every_argument() {
    let source = prompt("args: { type: object }", &[("Only", NEVER_RUNS)]);
    assert_argument_refused(
        &source,
        r#"{"n":1}"#,
        "undeclared",
        "n",
        "argument `n` is not declared; the prompt declares no arguments",
    );
}

#[test]
fn an_unsupported_version_is_reported_before_refused_arguments() {
    let source = search_prompt(NEVER_RUNS).replace("runvil: 0", "runvil: 7");
    let trace = Scenario::new(source).args("papers").run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.located_message()),
        (
            ErrorCode::UnsupportedVersion,
            "unsupported runvil version: 7 (this build supports major 0) (at prompt:4)".to_owned()
        ),
        "the version gate refuses the run first"
    );
    assert!(trace.events().is_empty(), "nothing ran, nothing reported");
}

#[test]
fn refused_arguments_are_reported_before_an_undeclared_store() {
    let source = search_prompt(NEVER_RUNS);
    let no_store = || VfsRef::new(MemoryBackend::new());
    let error = refusal(Scenario::new(&source).vfs(no_store()));
    assert_eq!(error.message, "argument `query` is required");

    let accepted = Scenario::new(&source)
        .vfs(no_store())
        .args(r#"{"query":"papers"}"#)
        .run();
    assert_eq!(
        accepted.error().code,
        ErrorCode::NoStore,
        "with arguments the declaration accepts, the store probe is next"
    );
}

#[test]
fn refused_arguments_are_reported_before_an_unmet_requirement() {
    let front = format!("{SEARCH}\nmodels: {{ strong: {{ min_context: 200000 }} }}");
    let source = prompt(&front, &[("Only", NEVER_RUNS)]);
    let small = || test_model_with("small", 32_000, ThinkingMode::Switchable);
    let error = refusal(
        Scenario::new(&source)
            .current_model(small())
            .args(r#"{"query":5}"#),
    );
    assert_eq!(
        error.message,
        "argument `query` must be a string, got integer"
    );

    let accepted = Scenario::new(&source)
        .current_model(small())
        .args(r#"{"query":"papers"}"#)
        .run();
    assert_eq!(
        accepted.error().code,
        ErrorCode::RequirementsUnmet,
        "with arguments the declaration accepts, the requirements refusal is next"
    );
}

#[test]
fn refused_arguments_after_the_runs_cancel_fired_end_the_run_canceled_with_no_event() {
    let source = search_prompt(NEVER_RUNS);
    for args in ["papers", "5", r#"{"query":5}"#] {
        let mut driving = Scenario::new(&source).args(args).start();
        driving.cancel();
        let trace = driving.finish();
        assert!(
            trace.error().is_canceled(),
            "arguments {args} after a cancel end the run canceled, got {:?}",
            trace.result()
        );
        assert!(
            trace.events().is_empty(),
            "a run that never started reports no event: {:?}",
            trace.events()
        );
    }
}
