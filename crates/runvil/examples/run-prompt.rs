//! A Host runs one prompt through a Harness: it builds a Host over a
//! fresh Pack registry, which holds only the built-ins, a finder that
//! serves its one prompt, and a tokio timer, parses the prompt by its
//! name, hands the run a recorder, an offline inference broker, and the
//! operator's name, and prints the greeting the prompt returns.

use std::sync::Arc;
use std::time::Duration;

use runvil::effect::Round;
use runvil::harness::{InferenceBroker, MemoryRecorder, Packs, Timer};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::plugin::PackRegistry;
use runvil::{BoxFuture, Error, ErrorCode, Harness, Host, RunSetup};

/// The name the Host finds the greeting prompt by.
const GREET_NAME: &str = "greet.md";

/// Returns a greeting for the name its `text` argument holds.
const GREET: &str = concat!(
    "---\nname: greet\ndescription: Greets the operator by name\nrunvil: 0\n",
    "---\n\n## Answer\n\n```lua\n",
    "return 'Hello, ' .. args.text .. '.'\n",
    "```\n",
);

/// Serves [`GREET`] under [`GREET_NAME`] and finds no other prompt.
fn find(name: &str) -> Result<String, Error> {
    if name == GREET_NAME {
        return Ok(GREET.to_owned());
    }
    let code = ErrorCode::NotFound;
    Err(Error::new(
        code,
        format!("no prompt named {name}"),
        code.prose(),
    ))
}

/// Lists no model and refuses every round, since the prompt asks no model.
struct Offline;

impl InferenceBroker for Offline {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

/// Sleeps on tokio's timer.
struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the Host once: its installed Packs, how it finds prompts,
    //    and its timer.
    let host = Arc::new(Host {
        packs: Packs::new(PackRegistry::new()),
        prompts: Arc::new(find),
        timer: Arc::new(Clock),
    });

    // 2. Find the prompt by its name and parse it.
    let prompt = host.parse(GREET_NAME)?;

    // 3. Give this run its recorder, its broker, and the operator's name;
    //    every other field keeps its default, such as a fresh store.
    let recorder = Arc::new(MemoryRecorder::new());
    let setup = RunSetup {
        args: r#"{"text":"desk"}"#.to_owned(),
        ..RunSetup::new(recorder.clone(), Arc::new(Offline))
    };

    // 4. Run it to its end; the Harness is spent.
    let report = Harness::new(host, prompt, setup).run().await?;

    // 5. The report says how the run ended: its result is the greeting
    //    the prompt returned, and the recorder holds the same result.
    assert_eq!(recorder.result(report.run_id), Some(report.result.clone()));
    let greeting = report.result?;
    assert_eq!(greeting, "Hello, desk.");
    println!("{greeting}");
    Ok(())
}
