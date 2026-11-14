//! Pack preludes in a run: every section VM installs the preludes
//! handed to `RunParams::with_preludes`, fanout arms and spawned task chains
//! included, and the libraries a section requires see them; a prelude
//! that fails to load or whose global collides fails the run as a Lua
//! failure before it issues any effect; a prelude that calls a tool or
//! the store while it installs fails to load, and one that catches that
//! failure installs with the store untouched; and a prelude's functions,
//! called from a block, reach the yielding store and tool shims, through
//! a store function the prelude captured too.

use runvil_types::packs::{PackId, Prelude};

use super::fixtures::echo_tool;
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller};
use crate::lua::VfsOp;
/// A prelude contributed by the Pack `id`.
pub(super) fn prelude(id: &str, source: &str) -> Prelude {
    Prelude::new(PackId::parse(id).expect("a valid Pack id"), source)
}

/// A prelude defining one table global whose function greets a name.
const GREETER: &str = "greeter = { greeting = 'hello' }\n\
    function greeter.greet(name)\n\
      return greeter.greeting .. ' ' .. name\n\
    end";

/// A prompt whose preamble writes to the store, so a run that sets up its
/// first section VM issues a store effect at once.
const EFFECT_FIRST: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
    ```lua\nstore.write('preamble.txt', 'x')\n```\n\n\
    ## Only\n\n\
    ```lua\nreturn 'done'\n```\n";

/// Runs `md` over the echo tool as its catalog, the test model, and
/// `preludes`.
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
        EFFECT_FIRST,
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
        EFFECT_FIRST,
        vec![prelude("seeder", "store.write('seeded.txt', 'seed')")],
        &[
            "Pack `seeder`: its prelude failed to load",
            "it must not call tools or the store while loading",
        ],
    );
}

#[test]
fn a_prelude_that_catches_its_store_call_installs_and_leaves_the_store_untouched() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn tostring(seeded) .. '|' .. tostring(store.exists('seeded.txt'))\n```\n";
    let seeder = "local ok = pcall(function() store.write('seeded.txt', 'seed') end)\n\
        seeded = ok";
    let trace = prelude_run(md, vec![prelude("seeder", seeder)]);
    assert_eq!(
        trace.text(),
        "false|false",
        "the call failed inside the prelude, and no write reached the store"
    );
    assert_eq!(
        records(&trace),
        vec![EffectRecord::Vfs {
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
        EFFECT_FIRST,
        vec![prelude("first", GREETER), prelude("second", "greeter = {}")],
        &[
            "Pack `second`: its prelude defines the global `greeter`",
            "which Pack `first`'s prelude already defines",
        ],
    );
}

#[test]
fn a_prelude_global_named_like_an_offered_tools_wire_name_installs() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\nuses:\n  - tools\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn tools_echo .. '|' .. tools.get('tools/echo').id\n```\n";
    let trace = prelude_run(md, vec![prelude("echoes", "tools_echo = 'mine'")]);
    assert_eq!(
        trace.text(),
        "mine|tools/echo",
        "no tool is a global, so the prelude's global and the tool sit side by side"
    );
}

#[test]
fn a_prelude_global_named_like_a_model_role_label_installs() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn strong .. '|' .. models.get('strong').name\n```\n";
    let trace = prelude_run(md, vec![prelude("strong", "strong = 'mine'")]);
    assert_eq!(
        trace.text(),
        "mine|strong",
        "the prelude's global and the role's handle sit side by side"
    );
}

#[test]
fn a_prelude_defining_ui_or_item_collides_on_a_run_that_binds_neither() {
    for name in ["ui", "item"] {
        assert_fails_before_any_effect(
            EFFECT_FIRST,
            vec![prelude("reserved", &format!("{name} = {{}}"))],
            &[
                &format!("Pack `reserved`: its prelude defines the global `{name}`"),
                "which is reserved as an Engine global",
            ],
        );
    }
}

#[test]
fn a_library_a_fanout_arm_and_a_spawned_task_all_see_a_prelude_global() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
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
        ```\n";
    let trace = prelude_run(md, vec![prelude("greeter", GREETER)]);
    assert_eq!(
        trace.text(),
        "hello library|hello arm|hello task",
        "the library reads the prelude while loading, and the arm and the task chain see it"
    );
}

/// The two store effects of a prelude function that writes `text` to
/// `path` and reads it back.
fn write_then_read(path: &str, text: &str) -> Vec<EffectRecord> {
    vec![
        EffectRecord::Vfs {
            op: VfsOp::Write {
                path: path.to_owned(),
                contents: text.to_owned(),
            },
        },
        EffectRecord::Vfs {
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
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn keeper.keep('kept')\n```\n";
    let keeper = "local write, read = store.write, store.read\n\
        keeper = {}\n\
        function keeper.keep(text)\n\
          write('kept.md', text)\n\
          return read('kept.md')\n\
        end";
    let trace = prelude_run(md, vec![prelude("keeper", keeper)]);
    assert_eq!(trace.text(), "kept");
    assert_eq!(
        records(&trace),
        write_then_read("kept.md", "kept"),
        "each call through a captured store function is a store effect"
    );
}

#[test]
fn a_prelude_function_called_from_a_block_reaches_the_yielding_store_shims() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn notes.keep('kept')\n```\n";
    let notes = "notes = {}\n\
        function notes.keep(text)\n\
          store.write('notes.md', text)\n\
          return store.read('notes.md')\n\
        end";
    let trace = prelude_run(md, vec![prelude("notes", notes)]);
    assert_eq!(trace.text(), "kept");
    assert_eq!(
        records(&trace),
        write_then_read("notes.md", "kept"),
        "each store call inside the prelude function is a store effect"
    );
}

#[test]
fn a_tool_call_made_inside_a_prelude_function_records_a_script_caller() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Only\n\n\
        ```lua\nreturn echoer.say('hi')\n```\n";
    let echoer = "echoer = {}\n\
        function echoer.say(value)\n\
          return tools.call('tools/echo', { value = value })\n\
        end";
    let trace = prelude_run(md, vec![prelude("echoer", echoer)]);
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
