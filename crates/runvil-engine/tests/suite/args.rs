//! Passing a run its arguments, and reading them back as `args`.

use runvil_engine::{Error, ErrorCode, ErrorFact, Step};
use serde_json::json;

use super::support::{Scenario, TestResult, greeter, prompt};

/// Declares a required string `name` and an integer `times` whose default
/// is 1.
const DECLARED: &str = "args: { type: object, properties: { name: { type: string }, times: { type: integer, default: 1 } }, required: [name] }";

/// Greets its `text` argument in prose, and returns the prose as the
/// section's epilog reads it.
const GREETS_IN_PROSE: &str = concat!(
    "---\nname: greeter\ndescription: Greets its text argument in prose.\nrunvil: 0\n---\n\n",
    "## Greet\n\n",
    "Hello, {{ args.text }}.\n\n",
    "```lua\nreturn prose\n```\n",
);

/// The failure the first step of `scenario`'s run of `source` ends the run
/// with, after checking that the step is `Done`, that the failure's code
/// is `ArgsInvalid`, that it names no line of the prompt, and that the
/// step reports no event.
fn refused(scenario: Scenario, source: &str) -> TestResult<Error> {
    let mut driver = scenario.start(source)?;
    let Step::Done { result, events, .. } = driver.run.step() else {
        return Err("the first step ends a run whose arguments the prompt refuses".into());
    };
    let Err(error) = result else {
        return Err(format!("expected the arguments to be refused, got {result:?}").into());
    };
    assert_eq!(error.code, ErrorCode::ArgsInvalid, "{error:?}");
    assert_eq!(
        error.at, None,
        "the arguments are the caller's, so the failure names no line of the prompt"
    );
    assert!(
        events.is_empty(),
        "a refused run reports no events: {events:?}"
    );
    Ok(error)
}

#[test]
fn a_run_reads_the_arguments_its_declaration_accepts_and_the_default_of_one_left_out() -> TestResult
{
    let lua = "return args.name .. ' x' .. args.times";
    let source = prompt(DECLARED, &[("Greet", lua)]);

    let named = Scenario::default().args(r#"{"name":"Ada"}"#);
    assert_eq!(named.run(&source)?.text()?, "Ada x1");
    let both = Scenario::default().args(r#"{"name":"Ada","times":3}"#);
    assert_eq!(both.run(&source)?.text()?, "Ada x3");
    Ok(())
}

#[test]
fn prose_reads_a_field_of_the_arguments_through_a_placeholder() -> TestResult {
    let scenario = Scenario::default().args(r#"{"text":"Ada"}"#);
    assert_eq!(scenario.run(GREETS_IN_PROSE)?.text()?, "Hello, Ada.");
    Ok(())
}

#[test]
fn a_prompt_with_no_args_key_runs_on_the_empty_object_and_on_one_string_named_text() -> TestResult {
    let source = greeter("return tostring(args.text)");

    let empty = Scenario::default().args("{}");
    assert_eq!(empty.run(&source)?.text()?, "nil");
    let texted = Scenario::default().args(r#"{"text":"hello"}"#);
    assert_eq!(texted.run(&source)?.text()?, "hello");
    Ok(())
}

#[test]
fn text_that_is_not_a_json_object_ends_the_run_at_its_first_step_naming_no_argument() -> TestResult
{
    let source = greeter("return 'done'");
    let cases = [
        ("", "not_json", "the arguments are not valid JSON"),
        ("hello", "not_json", "the arguments are not valid JSON"),
        (
            r#""hello""#,
            "not_an_object",
            "the arguments must be a JSON object, got string",
        ),
        (
            "[]",
            "not_an_object",
            "the arguments must be a JSON object, got array",
        ),
    ];
    for (text, reason, message) in cases {
        let error = refused(Scenario::default().args(text), &source)?;
        assert_eq!(
            error.facts.get(&ErrorFact::Reason),
            Some(&json!(reason)),
            "{text:?}: {error:?}"
        );
        assert_eq!(
            error.facts.get(&ErrorFact::Name),
            None,
            "{text:?}: no one argument is at fault"
        );
        let located = error.located_message();
        assert!(located.starts_with(message), "{text:?}: {located}");
    }
    Ok(())
}

#[test]
fn a_field_the_declaration_does_not_list_ends_the_run_at_its_first_step_naming_the_field()
-> TestResult {
    let scenario = Scenario::default().args(r#"{"text":"hello","tone":"warm"}"#);
    let error = refused(scenario, &greeter("return 'done'"))?;
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&json!("undeclared"))
    );
    assert_eq!(error.facts.get(&ErrorFact::Name), Some(&json!("tone")));
    assert_eq!(
        error.located_message(),
        "argument `tone` is not declared; the declared arguments are `text`"
    );
    Ok(())
}
