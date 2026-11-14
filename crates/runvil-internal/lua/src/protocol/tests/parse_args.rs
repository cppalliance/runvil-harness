//! The `args` a `call` or a spawn hands the chain it starts: a table of
//! named fields parses as the JSON object it holds, read through
//! read-only views, a string parses as that string, and a list, a value
//! of any other type, or a table with no JSON form is the call's error.
//! The last tests run the shims: what `call`, `tasks.spawn`, and a
//! `tasks.fanout` arm yield, and a refusal raised at the call site.

use super::*;
use crate::test_vm::{BuiltVm, TestVm};
use crate::{CoroStep, LuaBlockResult, LuaProgram};

/// The requests that take `args`.
const OPS: [&str; 2] = ["call", "spawn"];

/// The refusal of a list.
const LIST_REFUSAL: &str = "args must be a table of named fields or a string, got a list";

/// Parses the `op` request whose `args` field is what the Lua expression
/// `args` gives on `vm`.
#[track_caller]
fn parse_args(vm: &BuiltVm, op: &str, args: &str) -> YieldParse {
    let table: mlua::Table = vm.eval(&format!(
        "return {{ op = '{op}', target = '../Child', args = {args}, var = {{}} }}"
    ));
    Request::from_yield(vm.lua(), &Value::Table(table))
}

/// The `args` of the `call` or spawn request a yield parsed to.
#[track_caller]
fn parsed_args(parse: YieldParse) -> Option<serde_json::Value> {
    match expect_request(parse) {
        Request::Call { args, .. } | Request::Spawn { args, .. } => args,
        other => panic!("expected a call or a spawn request, got {other:?}"),
    }
}

/// The message of the call's error a yield was refused with.
#[track_caller]
fn refusal(parse: YieldParse) -> String {
    match parse {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => message,
        other => panic!("expected the call's error, got {other:?}"),
    }
}

#[test]
fn a_table_of_named_fields_parses_as_its_json_object() {
    let vm = TestVm::new().build();
    for op in OPS {
        let args = parsed_args(parse_args(
            &vm,
            op,
            "{ query = 'papers', limit = 3, tags = { 'a', 'b' }, \
               deep = { on = true, none = json.null, list = json.array() } }",
        ));
        assert_eq!(
            args,
            Some(json!({
                "query": "papers",
                "limit": 3,
                "tags": ["a", "b"],
                "deep": { "on": true, "none": null, "list": [] },
            })),
            "{op}"
        );
    }
}

#[test]
fn a_string_parses_as_that_string() {
    let vm = TestVm::new().build();
    for op in OPS {
        for (source, text) in [("'papers'", "papers"), ("''", "")] {
            assert_eq!(
                parsed_args(parse_args(&vm, op, source)),
                Some(json!(text)),
                "{op} {source}"
            );
        }
    }
}

#[test]
fn a_read_only_view_parses_as_the_object_it_stands_for() {
    let data = json!({
        "query": "papers",
        "tags": ["a", "b"],
        "nested": { "n": 1 },
        "none": null,
    });
    let vm = TestVm::new().args(&data).var(&data).build();
    for op in OPS {
        for (source, object) in [
            ("args", data.clone()),
            ("args.nested", json!({ "n": 1 })),
            ("var.nested", json!({ "n": 1 })),
            (
                "{ q = args.query, filter = args.nested, tags = args.tags, cfg = var.nested }",
                json!({
                    "q": "papers",
                    "filter": { "n": 1 },
                    "tags": ["a", "b"],
                    "cfg": { "n": 1 },
                }),
            ),
        ] {
            assert_eq!(
                parsed_args(parse_args(&vm, op, source)),
                Some(object),
                "{op} {source}"
            );
        }
    }
}

#[test]
fn an_unmarked_empty_table_parses_as_the_empty_object() {
    let vm = TestVm::new().build();
    for op in OPS {
        assert_eq!(
            parsed_args(parse_args(&vm, op, "{}")),
            Some(json!({})),
            "{op}"
        );
    }
}

#[test]
fn a_list_is_the_calls_error() {
    let data = json!({ "tags": ["a", "b"], "none": [] });
    let vm = TestVm::new().args(&data).build();
    for op in OPS {
        for list in [
            "{ 1, 2 }",
            "json.array()",
            "json.array({ 'a' })",
            "args.tags",
            "args.none",
        ] {
            assert_eq!(
                refusal(parse_args(&vm, op, list)),
                LIST_REFUSAL,
                "{op} {list}"
            );
        }
    }
}

#[test]
fn a_value_that_is_neither_a_table_nor_a_string_is_the_calls_error_naming_its_type() {
    let vm = TestVm::new().build();
    for op in OPS {
        for (value, kind) in [
            ("5", "integer"),
            ("1.5", "number"),
            ("true", "boolean"),
            ("function() end", "function"),
        ] {
            assert_eq!(
                refusal(parse_args(&vm, op, value)),
                format!("args must be a table of named fields or a string, got {kind}"),
                "{op} {value}"
            );
        }
    }
}

#[test]
fn a_table_with_no_json_form_is_the_calls_error_naming_the_field() {
    let vm = TestVm::new().build();
    for op in OPS {
        for (table, message) in [
            (
                "{ f = function() end }",
                "args: value.f must be JSON data, got function",
            ),
            (
                "{ deep = { n = 0/0 } }",
                "args: value.deep.n is not a finite number",
            ),
            (
                "{ 'a', name = 'x' }",
                "args: value mixes list items and named keys",
            ),
            (
                "{ [true] = 1 }",
                "args: value has a boolean key; object keys must be strings",
            ),
            (
                "{ meta = sys }",
                "args: value.meta cannot be read (sys is sealed); \
                 copy the fields you need into a plain table",
            ),
        ] {
            assert_eq!(refusal(parse_args(&vm, op, table)), message, "{op} {table}");
        }
    }
}

#[test]
fn a_string_that_is_not_utf8_is_the_calls_error() {
    let vm = TestVm::new().build();
    for op in OPS {
        assert_eq!(
            refusal(parse_args(&vm, op, "'\\xff'")),
            "args must be a valid UTF-8 string",
            "{op}"
        );
    }
}

/// Runs `source` as a block on `vm` to its first yield and returns the
/// block, its coroutine, and what the yield parsed to.
#[track_caller]
fn first_yield(vm: &BuiltVm, source: &str) -> (LuaProgram, mlua::Thread, YieldParse) {
    let block = LuaProgram::compile(source, "prompt", "args test block", NonZeroU32::MIN)
        .expect("the block compiles");
    let step = vm.start_block_coro(&block).expect("the block starts");
    let CoroStep::Yielded(thread, values) = step else {
        panic!("the block must suspend on a request, got {step:?}");
    };
    let parse = vm.request_from_yield(&values);
    (block, thread, parse)
}

#[test]
fn call_and_spawn_yield_the_args_their_author_wrote() {
    let data = json!({ "query": "papers", "tags": ["a", "b"] });
    let fields = json!({ "query": "notes", "limit": 3 });
    for (source, args) in [
        (
            "return call('../Child', { query = 'notes', limit = 3 })",
            Some(fields.clone()),
        ),
        ("return call('../Child', 'notes')", Some(json!("notes"))),
        ("return call('../Child', args)", Some(data.clone())),
        ("return call('../Child')", None),
        (
            "return tasks.spawn('../Child', { args = { query = 'notes', limit = 3 } })",
            Some(fields.clone()),
        ),
        (
            "return tasks.spawn('../Child', { args = 'notes' })",
            Some(json!("notes")),
        ),
        (
            "return tasks.spawn('../Child', { args = args })",
            Some(data.clone()),
        ),
        ("return tasks.spawn('../Child', { item = 'x' })", None),
        ("return tasks.spawn('../Child')", None),
        ("return tasks.fanout('../Child', args.tags)", None),
    ] {
        let vm = TestVm::new().args(&data).shims().build();
        let (_, _, parse) = first_yield(&vm, source);
        assert_eq!(parsed_args(parse), args, "{source}");
    }
}

#[test]
fn a_refused_args_value_raises_at_the_call_site_where_pcall_catches_it() {
    for call in [
        "call('../Child', { 1, 2 })",
        "tasks.spawn('../Child', { args = { 1, 2 } })",
    ] {
        let vm = TestVm::new().shims().build();
        let (block, thread, parse) = first_yield(
            &vm,
            &format!(
                "local ok, err = pcall(function() return {call} end)\n\
                 return tostring(ok) .. '|' .. tostring(err.kind) .. '|' .. tostring(err)"
            ),
        );
        let YieldParse::Refused(error) = parse else {
            panic!("{call}: expected the call's error, got {parse:?}");
        };
        let step = vm
            .resume_block_coro_answer(&block, &thread, Err(error))
            .expect("the pcall catches the refusal");
        let CoroStep::Done(LuaBlockResult::Returned(Some(caught))) = step else {
            panic!("{call}: the block must return what it caught, got {step:?}");
        };
        assert_eq!(
            caught,
            format!(
                "false|lua|prompt:1: {LIST_REFUSAL}. {}",
                ErrorCode::LuaRuntime.prose()
            ),
            "{call}"
        );
    }
}
