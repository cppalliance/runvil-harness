//! The run surface a Host builds through the public API: a run over a
//! prompt string reports the text the prompt returned, and a run that an
//! error ends reports that error.

use std::sync::Arc;

use runvil::harness::MemoryRecorder;
use runvil::{ErrorCode, Harness, RunSetup};

use crate::support::{Offline, launch, no_packs};

/// A prompt that returns its `text` argument, shouted.
const SHOUTS: &str = concat!(
    "---\nname: shouts\ndescription: Shouts its text argument\nrunvil: 0\n",
    "---\n\n## Only\n\n```lua\n",
    "return string.upper(args.text)\n",
    "```\n",
);

/// A Harness running `source` with `hello` as its `text` argument.
fn harness(source: &str) -> Harness {
    let setup = RunSetup {
        args: r#"{"text":"hello"}"#.to_owned(),
        ..RunSetup::new(Arc::new(MemoryRecorder::new()), Arc::new(Offline))
    };
    launch(no_packs(), source, setup)
}

#[tokio::test]
async fn a_run_over_a_prompt_string_reports_the_text_the_prompt_returned() {
    let report = harness(SHOUTS).run().await.expect("the run is reported");
    assert_eq!(
        report.result,
        Ok("HELLO".to_owned()),
        "the report carries the value the prompt returned"
    );
}

#[tokio::test]
async fn a_run_that_an_error_ends_reports_the_error() {
    let failing = SHOUTS.replace("return", "error('no shout') return");
    let report = harness(&failing).run().await.expect("the run is reported");
    assert_eq!(
        report.result.as_ref().map_err(|error| error.code),
        Err(ErrorCode::LuaRuntime),
        "the raised error ends the run"
    );
}
