//! Sealed views as the tables they stand for: a `var` table and an `args`
//! table measure, iterate, and convert as their data, a write through a
//! view still crosses its guard, and a value with no view inside converts
//! as it does with no walk before it.

use mlua::{LuaSerdeExt, Table};

use super::*;
use crate::proxy::plain;
use crate::test_vm::raised;

/// A VM whose `var` and `args` each hold a list, an object, and an empty
/// list.
fn views_vm() -> BuiltVm {
    let data = json!({
        "items": ["a", "b", "c"],
        "cfg": { "mode": "fast", "depth": 2 },
        "empty": [],
    });
    TestVm::new().var(&data).args(&data).shims().build()
}

#[test]
fn the_length_of_a_view_is_the_length_of_its_table() {
    let vm = views_vm();
    for root in ["var", "args"] {
        let lengths: (i64, i64, i64, i64) = vm.eval(&format!(
            "return #{root}.items, #{root}.cfg, #{root}.empty, #{root}"
        ));
        assert_eq!(lengths, (3, 0, 0, 0), "{root}");
    }
}

#[test]
fn pairs_walks_a_view_as_its_table_in_key_order() {
    let vm = views_vm();
    for root in ["var", "args"] {
        let walked: String = vm.eval(&format!(
            "local seen = {{}}\n\
             for key, value in pairs({root}) do seen[#seen + 1] = key .. ':' .. type(value) end\n\
             for index, value in pairs({root}.items) do seen[#seen + 1] = index .. '=' .. value end\n\
             for _ in pairs({root}.empty) do seen[#seen + 1] = 'never' end\n\
             return table.concat(seen, ' ')"
        ));
        assert_eq!(
            walked, "cfg:table empty:table items:table 1=a 2=b 3=c",
            "{root}"
        );
    }
}

#[test]
fn next_reads_a_view_as_its_table() {
    let vm = views_vm();
    for root in ["var", "args"] {
        let (first, value, second, ended, empty): (String, i64, String, bool, bool) =
            vm.eval(&format!(
                "local first, value = next({root}.cfg)\n\
                 local second = next({root}.cfg, first)\n\
                 return first, value, second, next({root}.cfg, second) == nil, \
                        next({root}.empty) == nil"
            ));
        assert_eq!(
            (first.as_str(), value, second.as_str()),
            ("depth", 2, "mode"),
            "{root}"
        );
        assert!(ended, "{root}: the walk ends after the last key");
        assert!(empty, "{root}: an empty list has no first key");
    }
}

#[test]
fn table_concat_and_table_unpack_read_a_view_as_its_table() {
    let vm = views_vm();
    for root in ["var", "args"] {
        let (joined, first, last, count): (String, String, String, i64) = vm.eval(&format!(
            "local first, _, last = table.unpack({root}.items)\n\
             return table.concat({root}.items, ','), first, last, \
                    select('#', table.unpack({root}.items))"
        ));
        assert_eq!(
            (joined.as_str(), first.as_str(), last.as_str(), count),
            ("a,b,c", "a", "c", 3),
            "{root}"
        );
    }
}

#[test]
fn a_var_list_grows_and_shrinks_by_its_length() {
    let vm = views_vm();
    let grown: String = vm.eval(
        "var.items[#var.items + 1] = 'd'\n\
         table.insert(var.items, 'e')\n\
         return table.concat(var.items, ',') .. ' ' .. #var.items",
    );
    assert_eq!(grown, "a,b,c,d,e 5");
    let shrunk: String = vm.eval(
        "local last = table.remove(var.items)\n\
         return last .. ' ' .. #var.items .. ' ' .. json.encode(var.items)",
    );
    assert_eq!(shrunk, r#"e 4 ["a","b","c","d"]"#);
}

#[test]
fn a_write_through_a_value_pairs_hands_back_crosses_the_views_guard() {
    let vm = views_vm();
    let refusal = raised(&vm, "for _, value in pairs(args) do value.extra = 1 end");
    assert_eq!(
        refusal_line(&refusal),
        Some("args is read-only: cannot set field 'extra'"),
        "a table inside args is handed back as its read-only view"
    );
    let refusal = raised(&vm, "table.insert(args.items, 'd')");
    assert_eq!(
        refusal_line(&refusal),
        Some("args is read-only: cannot set field 4")
    );
    let state_is_the_view: bool = vm.eval("local _, state = pairs(args)\nreturn state == args");
    assert!(state_is_the_view, "pairs hands back the view, not its data");

    vm.exec("for key, value in pairs(var) do if key == 'cfg' then value.seen = true end end");
    assert_eq!(
        encoded(&vm, "json.encode(var.cfg)"),
        r#"{"depth":2,"mode":"fast","seen":true}"#,
        "a table inside var is handed back as its guarded view"
    );
    let refusal = raised(
        &vm,
        "for key, value in pairs(var) do\n\
           if key == 'cfg' then value.f = function() end end\n\
         end",
    );
    assert_eq!(
        refusal_line(&refusal),
        Some("var.cfg.f must be JSON data, got function"),
        "the guard checks the write"
    );
}

#[test]
fn a_view_assigned_to_var_is_stored_as_its_data() {
    let vm = views_vm();
    vm.exec(
        "var.copy = var.cfg\n\
         var.from_args = args.items\n\
         var.none = args.empty\n\
         var.mixed = { list = args.items, cfg = var.cfg, n = 1 }",
    );
    for (call, text) in [
        ("json.encode(var.copy)", r#"{"depth":2,"mode":"fast"}"#),
        ("json.encode(var.from_args)", r#"["a","b","c"]"#),
        ("json.encode(var.none)", "[]"),
        (
            "json.encode(var.mixed)",
            r#"{"cfg":{"depth":2,"mode":"fast"},"list":["a","b","c"],"n":1}"#,
        ),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
    vm.exec("var.copy.mode = 'slow'");
    assert_eq!(
        encoded(&vm, "var.cfg.mode"),
        "fast",
        "the stored copy is its own data"
    );
    assert_eq!(
        vm.var().expect("var reads back")["from_args"],
        json!(["a", "b", "c"]),
        "the run's var holds the data"
    );
}

#[test]
fn a_value_with_no_view_inside_is_handed_back_as_it_is() {
    let vm = views_vm();
    let lua = vm.lua();
    for source in [
        "return { 1, { a = 'x' }, n = 2 }",
        "return { 1, nil, 3 }",
        "return {}",
        "return 'text'",
        "return sys",
    ] {
        let value: Value = lua.load(source).eval().expect("the value builds");
        let handed = plain(lua, value.clone()).expect("the walk ends");
        assert_eq!(handed, value, "{source}: the same value, not a copy");
    }
}

#[test]
fn a_view_inside_a_value_becomes_a_plain_copy_of_its_data() {
    let vm = views_vm();
    let lua = vm.lua();
    let cfg = json!({ "mode": "fast", "depth": 2 });
    let items = json!(["a", "b", "c"]);
    for (source, json) in [
        (
            "return args",
            json!({ "items": items.clone(), "cfg": cfg.clone(), "empty": [] }),
        ),
        ("return var.items", items.clone()),
        ("return args.empty", json!([])),
        ("return { 'first', var.cfg }", json!(["first", cfg])),
        (
            "return { n = 1, deep = { list = { args.items } }, plain = { 1, 2 } }",
            json!({ "n": 1, "deep": { "list": [items] }, "plain": [1, 2] }),
        ),
    ] {
        let value: Value = lua.load(source).eval().expect("the value builds");
        let handed = plain(lua, value.clone()).expect("the walk ends");
        assert_ne!(handed, value, "{source}: a copy, not the view");
        let converted: Json = lua.from_value(handed).expect("the copy is JSON data");
        assert_eq!(converted, json, "{source}");
    }
}

#[test]
fn a_table_beside_a_view_is_shared_with_the_copy_not_copied() {
    let vm = views_vm();
    let lua = vm.lua();
    let (value, beside): (Table, Table) = lua
        .load("local beside = { 1, 2 }\nreturn { view = args.items, beside = beside }, beside")
        .eval()
        .expect("the value builds");
    let Value::Table(handed) = plain(lua, Value::Table(value)).expect("the walk ends") else {
        panic!("a table stays a table");
    };
    let shared: Table = handed.raw_get("beside").expect("the entry reads");
    assert_eq!(shared, beside, "a table with no view inside is not copied");
}

#[test]
fn a_table_that_contains_itself_converts_as_it_does_with_no_walk_before_it() {
    let vm = views_vm();
    let lua = vm.lua();
    for source in [
        "local t = { n = 1 }\nt.me = t\nreturn t",
        "local t = { list = args.items }\nt.me = t\nreturn t",
    ] {
        let value: Value = lua.load(source).eval().expect("the value builds");
        let direct = lua
            .from_value::<serde_json::Value>(value.clone())
            .expect_err("a table that contains itself has no JSON form");
        let handed = plain(lua, value).expect("the walk ends at the repeated table");
        let walked = lua
            .from_value::<serde_json::Value>(handed)
            .expect_err("the walk leaves the table containing itself");
        assert_eq!(walked.to_string(), direct.to_string(), "{source}");
    }
}

#[test]
fn a_walk_past_its_budget_is_refused() {
    // A 4 MiB heap gives the walk 524,288 values. `t` is 19 levels of
    // `{ t, t }` over one list: 20 tables in memory, and about 1 million
    // values in a walk that reads a shared table once per reference.
    let vm = TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .build();
    let lua = vm.lua();
    let value: Value = lua
        .load("local t = { 1 }\nfor _ = 1, 19 do t = { t, t } end\nreturn t")
        .eval()
        .expect("the value builds");
    let error = plain(lua, value).expect_err("the walk runs out of budget");
    assert_eq!(
        error.to_string(),
        "deserialize error: the value is too large to convert"
    );
}
