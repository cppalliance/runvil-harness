//! The setup's arguments through preparation: a new setup holds the empty
//! object `{}`, which a prompt with no `args:` key runs on; the setup's
//! `args` reach the run as the text of one JSON object; and arguments the
//! prompt's `args` declaration refuses end the run at its first step as a
//! recorded failed run, never an error of `Harness::run`.

use super::*;
use crate::support::assert_not_completed;

/// A prompt with no `args:` key that returns the type of its `args`
/// beside the one field such a prompt declares, `text`.
const READS_ARGS: &str = "---\nname: reads-args\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\nreturn type(args) .. '|' .. tostring(args.text)\n```\n";

/// A prompt that requires one string argument, `name`, and greets it.
const GREETS: &str = "---\nname: greets\ndescription: d\nrunvil: 0\n\
    args:\n  type: object\n  properties:\n    name: { type: string }\n  required: [name]\n\
    ---\n\n## Only\n\n```lua\nreturn 'Hello, ' .. args.name\n```\n";

/// The setup of a run recorded through `recorder` whose arguments are
/// `args`.
fn with_args(recorder: &Arc<MemoryRecorder>, args: &str) -> RunSetup {
    RunSetup {
        args: args.to_owned(),
        ..setup_over(recorder)
    }
}

#[tokio::test]
async fn a_new_setup_holds_the_empty_object_which_a_prompt_with_no_args_key_runs_on() {
    let setup = RunSetup::new(recorder(), Arc::new(ScriptedBroker::replying()));
    assert_eq!(setup.args, "{}", "a new setup's args are the empty object");

    let report = report_of(READS_ARGS, bare(), setup).await;
    assert_eq!(
        completed(report.result),
        "table|nil",
        "the run reads an `args` table with no field"
    );
}

#[tokio::test]
async fn args_the_prompt_refuses_end_the_run_as_a_recorded_failed_run_not_an_error_of_run() {
    let accepted = with_args(&recorder(), r#"{"name":"Ada"}"#);
    assert_eq!(
        completed(report_of(GREETS, bare(), accepted).await.result),
        "Hello, Ada",
        "the setup's args reach the run"
    );

    let recorder = recorder();
    let report = launch(
        bare(),
        Unused,
        GREETS,
        with_args(&recorder, r#"{"name":5}"#),
    )
    .run()
    .await
    .expect("refused arguments are the run's result, not an error of `run`");

    let error = report
        .result
        .as_ref()
        .expect_err("the prompt refuses the arguments");
    assert_eq!(error.code, ErrorCode::ArgsInvalid);
    assert_eq!(error.facts.get(&ErrorFact::Reason), Some(&json!("type")));
    assert_eq!(error.facts.get(&ErrorFact::Name), Some(&json!("name")));
    assert_eq!(
        error.located_message(),
        "argument `name` must be a string, got integer",
        "the failure is the Engine's, verbatim, and names no line of the prompt"
    );
    assert_eq!(
        recorder.result(report.run_id),
        Some(report.result.clone()),
        "the recorder holds the refusal as the run's failure"
    );
    assert_no_records(&recorder, report.run_id);
    assert_not_completed(&report.output);
}
