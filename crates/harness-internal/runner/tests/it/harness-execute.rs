//! A run whose prompt executes a prompt it lists, under the Harness: the
//! run executes the prompt `Host::parse` linked, with no find of its own,
//! so it does so though the Host's set has lost that prompt's file since;
//! each store operation's record names the store of the prompt whose
//! chain made it; and each prompt's sections get the preludes of the
//! Packs that prompt declares, while a tool of any Pack answers either.

use std::fs;
use std::path::{Path, PathBuf};

use harness_runner::{Host, PromptSet};
use runvil_engine::PromptFinder;

use super::*;
use crate::prepare::echo_pack;
use crate::support::{host_finding, registry};

/// A directory under the temp directory named for a test and this
/// process, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir()
            .join("runvil-tests")
            .join(env!("CARGO_PKG_NAME"))
            .join(format!("{label}-{}", std::process::id()));
        // An earlier process with the same id may have left the directory.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the scratch directory is made");
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A finder that serves `plan` as `plan.md` and `greet` as `greet.md`.
fn two_prompts(plan: &str, greet: &str) -> Arc<dyn PromptFinder> {
    let mut set = PromptSet::new();
    set.insert("plan.md", plan);
    set.insert("greet.md", greet);
    Arc::new(set)
}

/// Runs the prompt `host` parses as `plan.md` to its end, recorded
/// through `recorder`.
async fn run_plan(host: &Arc<Host>, recorder: &Arc<MemoryRecorder>) -> RunReport {
    let prompt = host.parse("plan.md").expect("the Host parses the plan");
    Harness::new(
        Arc::clone(host),
        prompt,
        setup(recorder.clone(), ScriptedBroker::replying()),
    )
    .run()
    .await
    .expect("the run is reported")
}

/// A prompt that lists `greet.md`, executes it with the text `Ada`, and
/// returns what it gave.
const PLAN: &str = "---\nname: plan\ndescription: d\nrunvil: 0\nprompts: [greet.md]\n---\n\n\
    ## Only\n\n```lua\nreturn 'plan got ' .. execute('greet.md', 'Ada')\n```\n";

/// A prompt that greets the text it is given.
const GREET: &str = "---\nname: greet\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\nreturn 'hello ' .. args.text\n```\n";

#[tokio::test]
async fn a_run_executes_the_prompt_its_parse_linked_though_the_set_has_lost_that_prompts_file() {
    let dir = Scratch::new("a_run_executes_the_prompt_its_parse_linked");
    fs::write(dir.path().join("plan.md"), PLAN).expect("the plan is written");
    let listed = dir.path().join("greet.md");
    fs::write(&listed, GREET).expect("the greeting is written");
    let mut set = PromptSet::new();
    set.insert_dir(dir.path()).expect("the directory lists");
    let set = Arc::new(set);
    let host = Host::builder(set.clone()).build();
    let prompt = host.parse("plan.md").expect("the set serves both prompts");

    fs::remove_file(&listed).expect("the greeting's file is removed");
    assert!(set.rescan().is_empty(), "the directory still lists");
    assert_eq!(set.names(), ["plan.md"]);
    let error = host
        .parse("plan.md")
        .expect_err("a parse after the rescan finds no greet.md");
    assert_eq!(error.message, "no prompt named greet.md");

    let recorder = Arc::new(MemoryRecorder::new());
    let report = Harness::new(
        host,
        prompt,
        setup(recorder.clone(), ScriptedBroker::replying()),
    )
    .run()
    .await
    .expect("the run is reported");
    assert_eq!(
        report.result,
        Ok("plan got hello Ada".to_owned()),
        "the run executes the prompt its parse read, and finds nothing itself"
    );
}

/// A prompt that writes `note.md`, executes `greet.md`, and returns what
/// that gave beside its own note.
const PLAN_KEEPS_A_NOTE: &str = "---\nname: plan\ndescription: d\nrunvil: 0\n\
    prompts: [greet.md]\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('note.md', 'plan')\n\
    local greeted = execute('greet.md', 'Ada')\n\
    return greeted .. ' | ' .. store.read('note.md')\n```\n";

/// A prompt that writes its own `note.md` and returns it.
const GREET_KEEPS_A_NOTE: &str = "---\nname: greet\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('note.md', 'greet ' .. args.text)\n\
    return store.read('note.md')\n```\n";

#[tokio::test]
async fn each_store_record_names_the_store_of_the_prompt_whose_chain_made_the_operation() {
    let host = Host::builder(two_prompts(PLAN_KEEPS_A_NOTE, GREET_KEEPS_A_NOTE)).build();
    let recorder = Arc::new(MemoryRecorder::new());
    let report = run_plan(&host, &recorder).await;
    assert_eq!(
        report.result,
        Ok("greet Ada | plan".to_owned()),
        "one path is one file in each prompt's store"
    );

    let records = recorder.records(report.run_id);
    let operations: Vec<(String, String)> = records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .filter_map(|effect| {
            let vfs = effect.payload.get("Vfs")?;
            let store = vfs["store"].as_str()?.to_owned();
            let operation = vfs["op"].as_object()?.keys().next()?.clone();
            Some((store, operation))
        })
        .collect();
    let operation = |store: &str, kind: &str| (store.to_owned(), kind.to_owned());
    assert_eq!(
        operations,
        [
            operation("0", "Write"),
            operation("0.0", "Write"),
            operation("0.0", "Read"),
            operation("0", "Read"),
        ],
        "the run's own store is `0`, and the executed prompt's is named by its chain, `0.0`"
    );
    assert_eq!(
        answers_to(&records, "Vfs"),
        [
            json!({ "Vfs": { "Ok": "Unit" } }),
            json!({ "Vfs": { "Ok": "Unit" } }),
            json!({ "Vfs": { "Ok": { "Text": "greet Ada" } } }),
            json!({ "Vfs": { "Ok": { "Text": "plan" } } }),
        ],
        "the loop performed each operation through the view of its own store"
    );
}

/// A prompt that declares the Pack `mine`, executes `greet.md`, and
/// returns which of the two Packs' prelude globals each prompt has,
/// around what the other Pack's tool echoes.
const PLAN_USES_MINE: &str = "---\nname: plan\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/mine\nprompts: [greet.md]\n---\n\n\
    ## Only\n\n```lua\n\
    local seen = tostring(mine_loaded) .. ',' .. tostring(theirs_loaded)\n\
    return seen .. ' ' .. tools.call('theirs/echo', { value = 'plan' })\n\
      .. ' | ' .. execute('greet.md')\n```\n";

/// A prompt that declares the Pack `theirs` and returns which of the two
/// Packs' prelude globals it has, beside what the other Pack's tool
/// echoes.
const GREET_USES_THEIRS: &str = "---\nname: greet\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/theirs\n---\n\n\
    ## Only\n\n```lua\n\
    local seen = tostring(mine_loaded) .. ',' .. tostring(theirs_loaded)\n\
    return seen .. ' ' .. tools.call('mine/echo', { value = 'greet' })\n```\n";

#[tokio::test]
async fn each_prompts_sections_get_its_own_packs_preludes_and_a_tool_of_any_pack_answers_either() {
    let packs = registry(&[
        echo_pack("tests/mine", Some("mine_loaded = 'mine'\n")),
        echo_pack("tests/theirs", Some("theirs_loaded = 'theirs'\n")),
    ]);
    let host = host_finding(packs, two_prompts(PLAN_USES_MINE, GREET_USES_THEIRS));
    let report = run_plan(&host, &Arc::new(MemoryRecorder::new())).await;
    assert_eq!(
        report.result,
        Ok("mine,nil plan | nil,theirs greet".to_owned()),
        "a prompt installs no prelude of a Pack only the other declares, and calls that \
         Pack's tool all the same"
    );
}
