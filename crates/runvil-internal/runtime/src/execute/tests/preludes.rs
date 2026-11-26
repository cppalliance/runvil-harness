//! Pack preludes in a run: every section VM installs the preludes handed
//! to `RunParams::with_preludes` whose Pack the prompt declares, in the
//! order the prompt declares the Packs, fanout arms and spawned task
//! chains included, and the libraries a section requires see them; a
//! prelude of a Pack the prompt does not declare is not installed; a
//! prelude that fails to load or whose global collides fails the run as a
//! Lua failure before it issues any effect; a prelude that calls a tool or
//! the store while it installs fails to load, and one that catches that
//! failure installs with the store untouched; and a prelude's functions,
//! called from a block, reach the yielding store and tool shims, through
//! a store function the prelude captured too.

use runvil_types::ids::ChainId;
use runvil_types::packs::{PackId, Prelude};
use runvil_vfs::VfsOp;

use super::fixtures::echo_tool;
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller, VfsAsker};
/// A prelude contributed by the Pack `id`.
pub(super) fn prelude(id: &str, source: &str) -> Prelude {
    Prelude::new(PackId::parse(id).expect("a valid Pack id"), source)
}

/// A prelude defining one table global whose function greets a name.
const GREETER: &str = "greeter = { greeting = 'hello' }\n\
    function greeter.greet(name)\n\
      return greeter.greeting .. ' ' .. name\n\
    end";

/// A prompt that declares the Packs `uses`, in that order, over `body`.
fn declaring(uses: &[&str], body: &str) -> String {
    format!(
        "---\nname: t\ndescription: d\nrunvil: 0\nuses: [{}]\n---\n\n{body}",
        uses.join(", ")
    )
}

/// A prompt that declares the Packs `uses` and whose preamble writes to
/// the store, so a run that sets up its first section VM issues a store
/// effect at once.
fn effect_first(uses: &[&str]) -> String {
    declaring(
        uses,
        "```lua\nstore.write('preamble.txt', 'x')\n```\n\n\
         ## Only\n\n\
         ```lua\nreturn 'done'\n```\n",
    )
}

/// Runs `md` over the echo tool as its catalog, the test model, and
/// `preludes`, of which a section installs those whose Pack `md` declares.
#[track_caller]
fn prelude_run(md: &str, preludes: Vec<Prelude>) -> Trace {
    Scenario::new(md)
        .bound()
        .tool(echo_tool())
        .params(move |ctx| ctx.with_preludes(preludes))
        .run()
}

/// The record of every effect `trace`'s run issued, in issue order.
fn records(trace: &Trace) -> Vec<EffectRecord> {
    trace
        .effects()
        .iter()
        .map(|(effect, _)| effect.clone())
        .collect()
}

/// Asserts `md` issues an effect when run without preludes, and that with
/// `preludes` it instead fails with a Lua runtime error naming every
/// fragment, before it issues any effect.
#[track_caller]
fn assert_fails_before_any_effect(md: &str, preludes: Vec<Prelude>, fragments: &[&str]) {
    let control = prelude_run(md, Vec::new());
    assert!(
        control.result().is_ok(),
        "without preludes the prompt succeeds: {:?}",
        control.result()
    );
    assert!(
        !control.effects().is_empty(),
        "without preludes the prompt issues an effect, so the check below is not vacuous"
    );
    let trace = prelude_run(md, preludes);
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error:?}");
    let message = error.to_string();
    for fragment in fragments {
        assert!(
            message.contains(fragment),
            "the failure names `{fragment}`: {message}"
        );
    }
    let records = records(&trace);
    assert!(records.is_empty(), "no effect is issued: {records:?}");
}

#[test]
fn a_prelude_that_calls_a_tool_while_loading_fails_the_run_as_lua_before_its_first_effect() {
    assert_fails_before_any_effect(
        &effect_first(&["eager"]),
        vec![prelude(
            "eager",
            "tools.call('tools/echo', { value = 'early' })",
        )],
        &[
            "Pack `eager`: its prelude failed to load",
            "it must not call tools or the store while loading",
        ],
    );
}

#[test]
fn a_prelude_that_calls_the_store_while_loading_fails_the_run_as_lua_before_its_first_effect() {
    assert_fails_before_any_effect(
        &effect_first(&["seeder"]),
        vec![prelude("seeder", "store.write('seeded.txt', 'seed')")],
        &[
            "Pack `seeder`: its prelude failed to load",
            "it must not call tools or the store while loading",
        ],
    );
}

#[test]
fn a_cancel_that_lands_while_a_step_fails_ends_the_run_as_canceled() {
    use runvil_types::cancel::CancelHandle;
    use runvil_types::event::Event;

    use crate::test_support::scenario::{MAIN_PROMPT, TEST_SEED};

    // A Pack prelude logs once and then loops, and a second thread sets the
    // run's cancel flag while it loops. The instruction hook aborts the
    // prelude because of that flag, and `install_preludes` turns the abort
    // into the prelude's load failure without reading the flag, so the step
    // fails as `lua_runtime`. The step polls the flag once before the chain
    // runs, so the cancel lands after that poll and only the failed-step arm
    // can still honor it.
    //
    // The setter waits for the section's start in the run's buffer, which the
    // frame construction reports immediately before it installs the
    // preludes, so the cancel lands while the prelude runs without a fixed
    // time, as the instruction-hook cancel test in `run/tests.rs` does.
    let md = declaring(&["eager"], "## Only\n\n```lua\nreturn 'done'\n```\n");
    let handle = CancelHandle::new();
    let cancel = handle.clone();
    let mut state = None;
    let mut run = Run::new_for_test(
        Arc::new(
            Prompt::parse(&|_: &str| Ok(md.clone()), MAIN_PROMPT)
                .expect("the prelude cancel prompt parses"),
        ),
        "{}",
        RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
            .with_cancel(handle)
            .with_preludes(vec![prelude("eager", "while true do end")]),
        |fresh| {
            state = Some(fresh.clone());
            fresh
        },
    );
    let state = state.expect("the run starts over the state it was handed");
    let setter = std::thread::spawn(move || {
        let mut read = Vec::new();
        while !read
            .iter()
            .any(|event| matches!(event, Event::SectionStarted { .. }))
        {
            read.extend(state.take_events());
            std::thread::yield_now();
        }
        cancel.cancel();
    });
    let step = run.step();
    setter.join().expect("the cancelling thread does not panic");
    let Step::Done { result, .. } = step else {
        panic!("the hook aborts the prelude and nothing is outstanding, got {step:?}");
    };
    let error = result.expect_err("the aborted prelude fails the run");
    assert!(
        error.is_canceled(),
        "a cancel that lands while a step fails must end the run canceled, got {error:?}"
    );
}

#[test]
fn a_failing_step_without_a_cancel_keeps_its_own_code() {
    // The same failing prelude without a cancel: the fix must not swallow a
    // genuine failure that lands while no cancel is set.
    let md = declaring(&["eager"], "## Only\n\n```lua\nreturn 'done'\n```\n");
    let trace = prelude_run(&md, vec![prelude("eager", "error('the prelude gives up')")]);
    assert_eq!(
        trace.error().code,
        ErrorCode::LuaRuntime,
        "{:?}",
        trace.result()
    );
}

#[test]
fn a_prelude_that_catches_its_store_call_installs_and_leaves_the_store_untouched() {
    let md = declaring(
        &["seeder"],
        "## Only\n\n\
         ```lua\nreturn tostring(seeded) .. '|' .. tostring(store.exists('seeded.txt'))\n```\n",
    );
    let seeder = "local ok = pcall(function() store.write('seeded.txt', 'seed') end)\n\
        seeded = ok";
    let trace = prelude_run(&md, vec![prelude("seeder", seeder)]);
    assert_eq!(
        trace.text(),
        "false|false",
        "the call failed inside the prelude, and no write reached the store"
    );
    assert_eq!(
        records(&trace),
        vec![EffectRecord::Vfs {
            asker: root_store(),
            op: VfsOp::Exists {
                path: "seeded.txt".to_owned(),
            },
        }],
        "the block's own check is the run's one store effect"
    );
}

#[test]
fn two_preludes_defining_one_global_fail_the_run_as_lua_before_its_first_effect() {
    assert_fails_before_any_effect(
        &effect_first(&["first", "second"]),
        vec![prelude("first", GREETER), prelude("second", "greeter = {}")],
        &[
            "Pack `second`: its prelude defines the global `greeter`",
            "which Pack `first`'s prelude already defines",
        ],
    );
}

#[test]
fn the_preludes_install_in_the_order_the_prompt_declares_their_packs() {
    // The caller hands `first` before `second`, and the prompt declares
    // `second` before `first`, so `second` installs first and the
    // collision is `first`'s.
    assert_fails_before_any_effect(
        &effect_first(&["second", "first"]),
        vec![prelude("first", GREETER), prelude("second", "greeter = {}")],
        &[
            "Pack `first`: its prelude defines the global `greeter`",
            "which Pack `second`'s prelude already defines",
        ],
    );
}

#[test]
fn a_prelude_of_a_pack_the_prompt_does_not_declare_is_not_installed() {
    let md = declaring(
        &["greeter"],
        "## Only\n\n\
         ```lua\nreturn greeter.greet('you') .. '|' .. tostring(stray)\n```\n",
    );
    let trace = prelude_run(
        &md,
        vec![
            prelude("stray", "stray = 'installed'"),
            prelude("greeter", GREETER),
        ],
    );
    assert_eq!(
        trace.text(),
        "hello you|nil",
        "the section has the global of the declared Pack's prelude and not the other's"
    );
    // A prelude that would fail to load fails nothing when no section
    // installs it.
    let trace = prelude_run(
        &effect_first(&[]),
        vec![prelude("stray", "store.write('seeded.txt', 'seed')")],
    );
    assert_eq!(trace.text(), "done");
}

#[test]
fn a_prelude_global_named_like_an_offered_tools_wire_name_installs() {
    let md = declaring(
        &["tools", "echoes"],
        "## Only\n\n\
         ```lua\nreturn tools_echo .. '|' .. tools.get('tools/echo').id\n```\n",
    );
    let trace = prelude_run(&md, vec![prelude("echoes", "tools_echo = 'mine'")]);
    assert_eq!(
        trace.text(),
        "mine|tools/echo",
        "no tool is a global, so the prelude's global and the tool sit side by side"
    );
}

#[test]
fn a_prelude_global_named_like_a_model_role_label_installs() {
    let md = declaring(
        &["strong"],
        "## Only\n\n\
         ```lua\nreturn strong .. '|' .. models.get('strong').name\n```\n",
    );
    let trace = prelude_run(&md, vec![prelude("strong", "strong = 'mine'")]);
    assert_eq!(
        trace.text(),
        "mine|strong",
        "the prelude's global and the role's handle sit side by side"
    );
}

#[test]
fn a_prelude_defining_item_collides_on_a_run_that_does_not_bind_it() {
    assert_fails_before_any_effect(
        &effect_first(&["reserved"]),
        vec![prelude("reserved", "item = {}")],
        &[
            "Pack `reserved`: its prelude defines the global `item`",
            "which is reserved as an Engine global",
        ],
    );
}

#[test]
fn a_library_a_fanout_arm_and_a_spawned_task_all_see_a_prelude_global() {
    let md = declaring(
        &["greeter"],
        "## Main\n\n\
         ```lua\n\
         require '/greetings'\n\
         local arms = tasks.fanout('Arm', { 'arm' })\n\
         local t = tasks.spawn('../Child')\n\
         local _, ok, child = tasks.join_any({ t })\n\
         assert(ok, tostring(child))\n\
         return library_greeting() .. '|' .. arms[1].text .. '|' .. child\n\
         ```\n\n\
         ### Arm\n\n\
         ```lua\nreturn greeter.greet(item)\n```\n\n\
         ## Child\n\n\
         ```lua\nreturn greeter.greet('task')\n```\n\n\
         ## greetings\n\n\
         ```lua\n\
         local at_load = greeter.greet('library')\n\
         function library_greeting() return at_load end\n\
         ```\n",
    );
    let trace = prelude_run(&md, vec![prelude("greeter", GREETER)]);
    assert_eq!(
        trace.text(),
        "hello library|hello arm|hello task",
        "the library reads the prelude while loading, and the arm and the task chain see it"
    );
}

/// The asker of a `store` call on the store the run starts with.
fn root_store() -> VfsAsker {
    VfsAsker::Lua {
        store: ChainId::root(),
    }
}

/// The two store effects of a prelude function that writes `text` to
/// `path` and reads it back.
fn write_then_read(path: &str, text: &str) -> Vec<EffectRecord> {
    vec![
        EffectRecord::Vfs {
            asker: root_store(),
            op: VfsOp::Write {
                path: path.to_owned(),
                contents: text.as_bytes().to_vec(),
            },
        },
        EffectRecord::Vfs {
            asker: root_store(),
            op: VfsOp::Read {
                path: path.to_owned(),
                start: None,
                end: None,
            },
        },
    ]
}

#[test]
fn a_store_function_a_prelude_captured_while_it_installed_works_from_a_block() {
    let md = declaring(
        &["keeper"],
        "## Only\n\n\
         ```lua\nreturn keeper.keep('kept')\n```\n",
    );
    let keeper = "local write, read = store.write, store.read\n\
        keeper = {}\n\
        function keeper.keep(text)\n\
          write('kept.md', text)\n\
          return read('kept.md')\n\
        end";
    let trace = prelude_run(&md, vec![prelude("keeper", keeper)]);
    assert_eq!(trace.text(), "kept");
    assert_eq!(
        records(&trace),
        write_then_read("kept.md", "kept"),
        "each call through a captured store function is a store effect"
    );
}

#[test]
fn a_prelude_function_called_from_a_block_reaches_the_yielding_store_shims() {
    let md = declaring(
        &["notes"],
        "## Only\n\n\
         ```lua\nreturn notes.keep('kept')\n```\n",
    );
    let notes = "notes = {}\n\
        function notes.keep(text)\n\
          store.write('notes.md', text)\n\
          return store.read('notes.md')\n\
        end";
    let trace = prelude_run(&md, vec![prelude("notes", notes)]);
    assert_eq!(trace.text(), "kept");
    assert_eq!(
        records(&trace),
        write_then_read("notes.md", "kept"),
        "each store call inside the prelude function is a store effect"
    );
}

#[test]
fn a_tool_call_made_inside_a_prelude_function_records_a_script_caller() {
    let md = declaring(
        &["echoer"],
        "## Only\n\n\
         ```lua\nreturn echoer.say('hi')\n```\n",
    );
    let echoer = "echoer = {}\n\
        function echoer.say(value)\n\
          return tools.call('tools/echo', { value = value })\n\
        end";
    let trace = prelude_run(&md, vec![prelude("echoer", echoer)]);
    assert_eq!(trace.text(), "echoed: hi");
    assert_eq!(
        records(&trace),
        vec![EffectRecord::ToolCall {
            tool: ToolId::parse("tools/echo").expect("a valid id"),
            alias: "tools/echo".to_owned(),
            args: json!({ "value": "hi" }),
            origin: ToolCallOrigin::new("Only", ToolCaller::Script).with_entry("0.0"),
        }]
    );
}
