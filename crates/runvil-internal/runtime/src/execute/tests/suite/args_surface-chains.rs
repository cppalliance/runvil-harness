//! The arguments a contained chain reads. `call(target, args)` and
//! `tasks.spawn(target, { args = ... })` give the chain a table of named
//! fields as its `args`, or a string as the one field `text`, and a chain
//! given none reads its caller's. A list and a value of any other type
//! are refused at the call site. Nothing checks a chain's own arguments
//! against the prompt's declaration or fills a default in them. A
//! `TaskStarted` event reports the args a spawn gave its chain, as the
//! chain reads them, and a fanout arm reads its caller's.

use runvil_prompt::test_support::{PromptSource, prompt};
use runvil_types::error::ErrorCode;
use runvil_types::event::Event;
use serde_json::{Value, json};

use super::{SEARCH, papers};
use crate::test_support::scenario::Trace;
use crate::test_support::{at_line, refused};

/// What a refused `args` must be, the text each refusal starts with.
const ARGS_SHAPE: &str = "args must be a table of named fields or a string";

/// The body of a chain that returns its `args` as JSON.
const ECHO: &str = "return json.encode(args)";

/// The `args` object a chain reads when it reads its caller's, in a run
/// of [`papers`] over a prompt declaring [`SEARCH`].
const RUN_ARGS: &str = r#"{"limit":3,"query":"papers"}"#;

/// A prompt declaring [`SEARCH`] whose `Main` runs `main` and whose `Sub`
/// returns its `args` as JSON.
fn calling(main: &str) -> String {
    prompt(SEARCH, &[("Main", main), ("Sub", ECHO)])
}

/// A prompt declaring [`SEARCH`] whose `Main` spawns `Child` with the
/// options `opts`, joins it, and returns its result, and whose `Child`
/// returns its `args` as JSON.
fn spawning(opts: &str) -> String {
    let main = format!(
        "local task = tasks.spawn('../Child', {opts})
         local _, ok, result = tasks.join_any({{ task }})
         assert(ok, 'the task returns its args')
         return result"
    );
    prompt(SEARCH, &[("Main", &main), ("Child", ECHO)])
}

/// The `args` of every `TaskStarted` event `trace` reports, in order.
fn started_args(trace: &Trace) -> Vec<Option<Value>> {
    trace
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::TaskStarted { args, .. } => Some(args.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_call_passes_a_table_of_named_fields_as_the_chains_args() {
    let source = calling(
        "return call('../Sub', { topic = 'chain', pages = { 1, 2 }, opts = { deep = true } })",
    );
    assert_eq!(
        papers(source).run().text(),
        r#"{"opts":{"deep":true},"pages":[1,2],"topic":"chain"}"#,
        "the chain reads the table it was passed in place of the caller's args"
    );
}

#[test]
fn a_call_passes_a_string_as_the_one_field_text() {
    let source = calling("return call('../Sub', 'plain words') .. call('../Sub', '')");
    assert_eq!(
        papers(source).run().text(),
        r#"{"text":"plain words"}{"text":""}"#
    );
}

#[test]
fn a_call_that_passes_args_itself_hands_the_chain_the_callers_arguments() {
    let source = calling(
        "return call('../Sub', args) .. call('../Sub', { outer = args, limit = args.limit })",
    );
    assert_eq!(
        papers(source).run().text(),
        format!(r#"{RUN_ARGS}{{"limit":3,"outer":{RUN_ARGS}}}"#),
        "a read-only args table converts as the object it holds, at any depth"
    );
}

#[test]
fn a_call_with_an_empty_table_gives_the_chain_empty_args() {
    let source = calling("return call('../Sub', {})");
    assert_eq!(papers(source).run().text(), "{}");
}

#[test]
fn a_call_that_passes_no_args_reads_the_callers_args() {
    let source = calling("return call('../Sub') .. call('../Sub', nil)");
    assert_eq!(
        papers(source).run().text(),
        format!("{RUN_ARGS}{RUN_ARGS}"),
        "the main walk's chain reads the run's args"
    );
    let nested = prompt(
        SEARCH,
        &[
            ("Main", "return call('../Mid', { query = 'mid' })"),
            ("Mid", "return call('../Sub')"),
            ("Sub", ECHO),
        ],
    );
    assert_eq!(
        papers(nested).run().text(),
        r#"{"query":"mid"}"#,
        "a chain inside a called chain reads that chain's args, not the run's"
    );
}

#[test]
fn a_placeholder_in_a_chain_reads_the_args_that_chain_reads() {
    let source = PromptSource::new()
        .front(SEARCH)
        .section(
            "Main",
            "
            local task = tasks.spawn('../Told', { args = { query = 'task' } })
            local _, _, told = tasks.join_any({ task })
            return call('../Told', { query = 'chain' }) .. '|' .. told .. '|' .. call('../Told')",
        )
        .prose(
            "Told",
            "
            Asked for {{ args.query }}.

            ```lua
            return prose
            ```",
        );
    assert_eq!(
        papers(source.build()).run().text(),
        "Asked for chain.|Asked for task.|Asked for papers."
    );
}

#[test]
fn a_chains_args_are_not_checked_against_the_declaration_and_take_no_default() {
    // `SEARCH` requires a string `query`, defaults `limit` to 3, allows
    // `depth` one of two values, and declares no `topic`.
    let source = calling(
        "return call('../Sub', { topic = 'chain', depth = 'deep' })
           .. call('../Sub', { query = 5 })",
    );
    assert_eq!(
        papers(source).run().text(),
        r#"{"depth":"deep","topic":"chain"}{"query":5}"#
    );
}

#[test]
fn a_call_with_a_list_is_refused_at_the_call_site_and_pcall_catches_it() {
    let source = calling(
        "local ok, err = pcall(call, '../Sub', { 'a', 'b' })
         local empty_ok, empty_err = pcall(call, '../Sub', json.array())
         return table.concat({ tostring(ok), err.kind, tostring(err),
           tostring(empty_ok), tostring(empty_err) }, '|')",
    );
    let refusal = format!("{ARGS_SHAPE}, got a list");
    assert_eq!(
        papers(source).run().text(),
        format!(
            "false|lua|{}|false|{}",
            refused(11, &refusal),
            refused(12, &refusal)
        ),
        "a list, a marked empty one included, has no named fields"
    );
}

#[test]
fn an_uncaught_call_with_a_list_ends_the_run_as_a_lua_error_on_the_calls_line() {
    let source = calling("local ready = true\nreturn call('../Sub', { 'a', 'b' })");
    let trace = papers(source).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error:?}");
    assert_eq!(error.message, format!("{ARGS_SHAPE}, got a list"));
    assert_eq!(error.at, at_line(12), "{error:?}");
}

#[test]
fn a_call_with_args_of_another_type_is_refused_naming_the_type() {
    for (value, kind) in [
        ("5", "integer"),
        ("2.5", "number"),
        ("true", "boolean"),
        ("function() end", "function"),
    ] {
        let source = calling(&format!(
            "local ok, err = pcall(call, '../Sub', {value})
             return tostring(ok) .. '|' .. err.kind .. '|' .. tostring(err)"
        ));
        assert_eq!(
            papers(source).run().text(),
            format!(
                "false|lua|{}",
                refused(11, &format!("{ARGS_SHAPE}, got {kind}"))
            ),
            "call args {value}"
        );
    }
}

#[test]
fn a_spawn_gives_its_chain_the_args_it_passes_and_task_started_reports_them() {
    for (opts, chain_reads) in [
        (
            "{ args = { topic = 'task', pages = { 1, 2 } } }",
            json!({ "topic": "task", "pages": [1, 2] }),
        ),
        ("{ args = 'plain words' }", json!({ "text": "plain words" })),
        ("{ args = args }", json!({ "limit": 3, "query": "papers" })),
        ("{ args = {} }", json!({})),
    ] {
        let trace = papers(spawning(opts)).run();
        assert_eq!(trace.text(), chain_reads.to_string(), "spawn opts {opts}");
        assert_eq!(
            started_args(&trace),
            [Some(chain_reads)],
            "the start event reports the chain's args as the chain reads them: {opts}"
        );
    }
}

#[test]
fn a_spawn_that_passes_no_args_reads_the_spawners_and_task_started_reports_none() {
    for opts in ["nil", "{}", "{ item = 'member', index = 2 }"] {
        let trace = papers(spawning(opts)).run();
        assert_eq!(trace.text(), RUN_ARGS, "spawn opts {opts}");
        assert_eq!(started_args(&trace), [None], "spawn opts {opts}");
    }
}

#[test]
fn a_spawned_chains_args_are_not_checked_against_the_declaration_and_take_no_default() {
    let trace = papers(spawning("{ args = { topic = 'task', depth = 5 } }")).run();
    assert_eq!(trace.text(), r#"{"depth":5,"topic":"task"}"#);
}

#[test]
fn a_spawn_with_a_list_or_another_type_as_its_args_is_refused_and_starts_no_task() {
    let source = prompt(
        SEARCH,
        &[
            (
                "Main",
                "local function refused(opts)
                   local ok, err = pcall(tasks.spawn, '../Child', opts)
                   assert(not ok, 'the spawn is refused')
                   return err.kind .. ':' .. tostring(err)
                 end
                 return refused({ args = { 'a', 'b' } }) .. '|' .. refused({ args = json.array() })
                   .. '|' .. refused({ args = 5 }) .. '|' .. refused({ args = true })",
            ),
            ("Child", ECHO),
        ],
    );
    let trace = papers(source).run();
    let lua = |what: &str| format!("lua:{}", refused(12, &format!("{ARGS_SHAPE}, got {what}")));
    assert_eq!(
        trace.text(),
        [lua("a list"), lua("a list"), lua("integer"), lua("boolean")].join("|")
    );
    assert!(
        started_args(&trace).is_empty(),
        "a refused spawn starts no task: {:?}",
        trace.events()
    );
}

#[test]
fn a_fanout_arm_reads_its_callers_args_and_task_started_reports_none_for_it() {
    let source = prompt(
        SEARCH,
        &[
            (
                "Parent",
                "local r = tasks.fanout('Worker', { 'a', 'b' })
                 return r[1].text .. '|' .. r[2].text",
            ),
            (
                "### Worker",
                "return item .. ':' .. args.query .. ':' .. args.limit",
            ),
        ],
    );
    let trace = papers(source).run();
    assert_eq!(trace.text(), "a:papers:3|b:papers:3");
    assert_eq!(started_args(&trace), [None, None]);
}

#[test]
fn a_fanout_arm_inside_a_called_chain_reads_that_chains_args() {
    let source = prompt(
        SEARCH,
        &[
            ("Main", "return call('../Parent', { query = 'chain' })"),
            (
                "Parent",
                "local r = tasks.fanout('Worker', { 'a' })
                 return r[1].text",
            ),
            ("### Worker", "return item .. ':' .. args.query"),
        ],
    );
    assert_eq!(papers(source).run().text(), "a:chain");
}
