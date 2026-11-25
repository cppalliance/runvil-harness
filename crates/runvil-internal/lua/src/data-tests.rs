//! Tests for [`data_of`]: one result in every VM for a table with a gap,
//! for a value with several faults, for a weak table, and for a value
//! nested too deep.

use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use serde_json::{Value as Json, json};

use super::data_of;
use crate::walk_budget::{DEPTH_LIMIT, HeapCeiling};

/// How many VMs a test reads a value in: each has its own hash seed, so
/// each walks a table in its own order and measures a gap its own way.
const STATES: usize = 16;

#[test]
fn a_list_with_a_gap_is_refused_and_names_the_missing_item_in_every_vm() {
    for state in 0..STATES {
        let lua = Lua::new();
        for (source, reason) in [
            // `#t` is 3 in one VM and 5 in another.
            (
                "return { [1] = 1, [2] = 2, [3] = 3, [5] = 5 }",
                "value is missing list item [4]",
            ),
            (
                "return { a = { 'x', nil, 'z' } }",
                "value.a is missing list item [2]",
            ),
            ("return { [2] = 'b' }", "value is missing list item [1]"),
        ] {
            assert_eq!(refusal(&lua, source), reason, "state {state}: {source}");
        }
        // What any other key of a list holds is left out of the data, so
        // a gap there refuses nothing.
        assert_eq!(
            converted(&lua, "return { 'a', extra = { [2] = 1 } }"),
            json!(["a"]),
            "state {state}"
        );
    }
}

#[test]
fn of_several_faults_the_one_under_the_first_key_is_named_in_every_vm() {
    for state in 0..STATES {
        let lua = Lua::new();
        for (source, reason) in [
            (
                "return { a = function() end, b = coroutine.create(function() end) }",
                "unsupported value type `function`",
            ),
            (
                "return { zulu = 1, mike = 1, alpha = 1, oscar = 1, [true] = 1 }",
                "invalid type: boolean `true`, expected a string key",
            ),
            // The fault of a key comes before the fault of its value.
            (
                "return { [true] = function() end }",
                "invalid type: boolean `true`, expected a string key",
            ),
            (
                "return { a = { [1] = 1, [3] = 3 }, b = function() end }",
                "value.a is missing list item [2]",
            ),
            (
                "return { z = { y = function() end }, a = { b = { [0] = 1 } } }",
                "invalid type: integer `0`, expected a string key",
            ),
            // A table with a list converts as its list alone, so a fault
            // under a name of such a table is none of the conversion's.
            (
                "return { rows = { 'a', bad = { [1] = 1, [3] = 3 } }, z = function() end }",
                "unsupported value type `function`",
            ),
            // The path of a fault some tables down reads from the value
            // to the fault, a step for each table.
            (
                "return {\n\
                   b = { 1, nil, 3 },\n\
                   a = { 'x', { z = { [{}] = 1 }, k = { [1] = 1, [3] = 3 } } },\n\
                 }",
                "value.a[2].k is missing list item [2]",
            ),
            // `from_value` says `map` of a key table without a list and
            // `sequence` of one with a list, and the two keys have no
            // order.
            (
                "local t = {}\n\
                 for i = 1, 32 do\n\
                   local k = {}\n\
                   if i % 2 == 0 then k[1] = 1 end\n\
                   t[k] = 1\n\
                 end\n\
                 return { rows = t }",
                "value.rows has a table key; object keys must be strings",
            ),
            (
                "return { [{}] = 1, [function() end] = 1, [coroutine.create(function() end)] = 1 }",
                "value has a function key; object keys must be strings",
            ),
        ] {
            assert_eq!(refusal(&lua, source), reason, "state {state}: {source}");
        }
    }
}

#[test]
fn a_value_with_one_fault_is_refused_in_the_words_of_from_value() {
    let lua = Lua::new();
    for source in [
        "return { f = function() end }",
        "return { 'a', coroutine.create(function() end) }",
        "return { text = '\\255' }",
        "return { ['\\255'] = 1 }",
        "return { text = 'ab\\200cd' }",
        "return { ['ab\\200cd'] = 1 }",
        "return { [true] = 1 }",
        "local t = {}\nt.me = t\nreturn t",
    ] {
        let direct = lua
            .from_value::<Json>(value(&lua, source))
            .expect_err("the value has no JSON form")
            .to_string();
        assert_eq!(
            format!("deserialize error: {}", refusal(&lua, source)),
            direct,
            "{source}"
        );
    }
}

#[test]
fn a_number_that_json_cannot_hold_becomes_null() {
    let lua = Lua::new();
    assert_eq!(
        converted(&lua, "return { 0/0, 1/0, -1/0 }"),
        json!([null, null, null])
    );
    assert_eq!(
        converted(&lua, "return { n = 0/0, p = 1/0 }"),
        json!({ "n": null, "p": null })
    );
}

#[test]
fn a_key_that_serde_json_keeps_for_itself_is_a_name_in_every_vm() {
    // With its `raw_value` feature on, which the tests of this crate turn
    // on, `serde_json::Value` reads a map whose first key is this name as
    // the JSON text under the key. Lua's own walk says which key comes
    // first.
    for state in 0..STATES {
        let lua = Lua::new();
        assert_eq!(
            converted(
                &lua,
                r#"return { ["$serde_json::private::RawValue"] = "[1,2]" }"#
            ),
            json!({ "$serde_json::private::RawValue": "[1,2]" }),
            "state {state}"
        );
        assert_eq!(
            converted(
                &lua,
                r#"return { ["$serde_json::private::RawValue"] = "[1,2]", a = 1 }"#
            ),
            json!({ "$serde_json::private::RawValue": "[1,2]", "a": 1 }),
            "state {state}"
        );
    }
}

#[test]
fn a_weak_table_is_refused_with_its_entries_and_once_the_collector_took_them() {
    // Without its entry the table is empty and has `__index` and
    // `__metatable`, which is the shape of a view of the `__index` table.
    // It is refused as the weak table it is in both states, so the
    // collector does not choose the result.
    let lua = Lua::new();
    let weak = value(
        &lua,
        "return { w = setmetatable({ transient = {} }, \
           { __mode = 'v', __metatable = false, __index = { x = 1 } }) }",
    );
    let refused = |lua: &Lua| match data_of(lua, weak.clone()) {
        Err(mlua::Error::DeserializeError(reason)) => reason,
        other => panic!("not a refusal: {other:?}"),
    };
    let held = refused(&lua);
    lua.gc_collect().expect("the collector runs");
    lua.gc_collect().expect("the collector runs");
    let left = lua
        .load("local value = ... return next(value.w) ~= nil")
        .call::<bool>(weak.clone())
        .expect("the table reads");
    assert_eq!(
        (left, held.as_str(), refused(&lua).as_str()),
        (
            false,
            "value.w is a weak table, one whose metatable sets __mode: the entries the \
             collector leaves in it are not the same in two runs",
            "value.w is a weak table, one whose metatable sets __mode: the entries the \
             collector leaves in it are not the same in two runs"
        )
    );
}

#[test]
fn a_table_whose_metatable_is_weak_is_refused_with_its_fields_and_once_the_collector_took_them() {
    // With its `__index` the table is a view of `{ x = 1 }`, and without
    // it the empty table it is. The field is an entry of a weak
    // metatable, which the collector takes, so the table is refused in
    // both states.
    let lua = Lua::new();
    let under = value(
        &lua,
        "local fields = { __metatable = false, __index = { x = 1 } } \
         return { w = setmetatable({}, setmetatable(fields, { __mode = 'v' })) }",
    );
    let refused = |lua: &Lua| match data_of(lua, under.clone()) {
        Err(mlua::Error::DeserializeError(reason)) => reason,
        other => panic!("not a refusal: {other:?}"),
    };
    let held = refused(&lua);
    lua.gc_collect().expect("the collector runs");
    lua.gc_collect().expect("the collector runs");
    let Value::Table(root) = &under else {
        panic!("the value is no table: {under:?}");
    };
    let fields = root
        .raw_get::<Table>("w")
        .expect("the value holds the table")
        .metatable()
        .expect("the table has a metatable");
    let left = fields
        .raw_get::<Value>("__index")
        .expect("the metatable reads");
    assert_eq!(
        (left, held.as_str(), refused(&lua).as_str()),
        (
            Value::Nil,
            "value.w is a table whose metatable is a weak table: the fields the collector \
             leaves in that metatable are not the same in two runs",
            "value.w is a table whose metatable is a weak table: the fields the collector \
             leaves in that metatable are not the same in two runs"
        )
    );
}

#[test]
fn a_value_nested_past_the_depth_limit_is_refused_and_a_shallower_one_reads_back() {
    let lua = Lua::new();
    let nested =
        |levels: usize| format!("local t = {{}}\nfor _ = 2, {levels} do t = {{ t }} end\nreturn t");
    let deepest = converted(&lua, &nested(DEPTH_LIMIT));
    assert_eq!(
        serde_json::from_str::<Json>(&deepest.to_string()).expect("the text reads back"),
        deepest
    );
    // Without the limit the second value overflows the stack.
    for levels in [DEPTH_LIMIT + 1, 100_000] {
        assert_eq!(
            refusal(&lua, &nested(levels)),
            format!(
                "value{} is nested more than {DEPTH_LIMIT} levels deep",
                "[1]".repeat(DEPTH_LIMIT)
            ),
            "{levels} levels"
        );
    }
    // A view whose data only a function can read converts to an empty
    // object, so it is a level like any table.
    let around_opaque = |tables: usize| {
        format!(
            "local t = setmetatable({{}}, {{ __metatable = false, __index = function() end }})\n\
             for _ = 1, {tables} do t = {{ t }} end\nreturn t"
        )
    };
    assert_eq!(converted(&lua, &around_opaque(DEPTH_LIMIT - 1)), deepest);
    assert_eq!(
        refusal(&lua, &around_opaque(DEPTH_LIMIT)),
        format!(
            "value{} is nested more than {DEPTH_LIMIT} levels deep",
            "[1]".repeat(DEPTH_LIMIT)
        )
    );
}

#[test]
fn a_table_that_holds_itself_through_a_view_is_refused() {
    let lua = Lua::new();
    // The conversion reads a copy of the data of a view, and in the copy
    // `top` is a table it has not met.
    assert_eq!(
        refusal(
            &lua,
            "local top = {}\n\
             local data = { top = top }\n\
             top.view = setmetatable({}, { __metatable = false, __index = data })\n\
             return top"
        ),
        "recursive table detected"
    );
}

#[test]
fn what_a_list_holds_beside_its_items_is_left_out_and_not_checked() {
    let lua = Lua::new();
    lua.globals()
        .set("marked", lua.array_metatable())
        .expect("the marker is a global");
    // A list is its items, so the conversion does not read what `x`
    // holds. A walk that went on into `t` there would go on to the depth
    // limit, twice at each level. A table that is marked as a list is one
    // with no item too.
    for (source, data) in [
        ("local t = { 'a' } t.x = { t, t } return t", json!(["a"])),
        (
            "return setmetatable({ x = { [1] = 1, [3] = 3 } }, marked)",
            json!([]),
        ),
    ] {
        assert_eq!(converted(&lua, source), data, "{source}");
    }
    // Beside a fault, what is left out is no fault, though its key comes
    // first: a weak table, a value with no JSON form, and text that is not
    // UTF-8.
    for unread in [
        "setmetatable({}, { __mode = 'k' })",
        "coroutine.create(print)",
        "'\\255'",
    ] {
        assert_eq!(
            refusal(
                &lua,
                &format!("return {{ function() end, [true] = {unread} }}")
            ),
            "unsupported value type `function`",
            "{unread}"
        );
    }
}

#[test]
fn whatever_views_and_cycles_a_value_holds_its_data_reads_back() {
    let lua = Lua::new();
    // A chain of 100 to 130 tables that ends in a view only a function can
    // read, with up to two more links between tables of the chain. A link
    // is the table or a view of it.
    let build = lua
        .load(
            "return function(seed)\n\
               math.randomseed(seed)\n\
               local function linked(t)\n\
                 if math.random(3) > 1 then return t end\n\
                 return setmetatable({}, { __metatable = false, __index = t })\n\
               end\n\
               local count = math.random(100, 130)\n\
               local chain = {}\n\
               for i = 1, count do chain[i] = {} end\n\
               for i = 1, count - 1 do chain[i].next = linked(chain[i + 1]) end\n\
               chain[count].last = setmetatable({}, { __metatable = false, __index = function() end })\n\
               for _ = 1, math.random(0, 2) do\n\
                 chain[math.random(count)].link = linked(chain[math.random(count)])\n\
               end\n\
               return linked(chain[1])\n\
             end",
        )
        .eval::<Function>()
        .expect("the builder loads");
    let mut converted = 0;
    for seed in 0..256 {
        let value = build.call::<Value>(seed).expect("the value builds");
        if let Ok(data) = data_of(&lua, value) {
            converted += 1;
            assert_eq!(
                serde_json::from_str::<Json>(&data.to_string()).ok(),
                Some(data),
                "seed {seed}"
            );
        }
    }
    assert_ne!(converted, 0, "no seed gave a value that converts");
}

#[test]
fn a_value_past_the_budget_is_too_large_whatever_else_it_holds() {
    // A 4 MiB heap gives a walk 524,288 values, and `t` holds about a
    // million. A walk that stopped at the gap would refuse the value for
    // the gap in the VMs that met it first.
    for state in 0..STATES / 2 {
        let lua = Lua::new();
        lua.set_app_data(HeapCeiling(4 * 1024 * 1024));
        assert_eq!(
            refusal(
                &lua,
                "local t = { 1 }\nfor _ = 1, 19 do t = { t, t } end\n\
                 return { a = t, b = { [2] = 1 }, c = t, d = { [2] = 1 } }"
            ),
            "the value is too large to convert",
            "state {state}"
        );
    }
}

#[test]
fn a_weak_table_under_a_gap_spends_the_same_budget_in_every_vm() {
    // The weak table holds more values than a 4 MiB heap gives a walk. It
    // sits past the gap, so the conversion reads it in the VMs whose `#t`
    // is 5 and not in those whose `#t` is 3. A walk that went into it only
    // where the conversion does not read it would be too large in those.
    for state in 0..STATES {
        let lua = Lua::new();
        lua.set_app_data(HeapCeiling(4 * 1024 * 1024));
        assert_eq!(
            refusal(
                &lua,
                "local w = setmetatable({}, { __mode = 'k' })
                 for i = 1, 600000 do w[i] = i end
                 return { [1] = 1, [2] = 2, [3] = 3, [5] = w }"
            ),
            "value is missing list item [4]",
            "state {state}"
        );
    }
}

#[test]
fn text_past_the_byte_budget_is_too_large_with_a_fault_and_without_in_every_vm() {
    // A 4 MiB heap gives a walk 4 MiB of text. The data holds a copy of
    // every string of the value, and the walk that names a fault reads
    // every one, so both walks count the bytes of each string before
    // they read it, each time the string occurs.
    let clean = |keys: usize| {
        format!(
            "local text = string.rep('x', 1024 * 1024)\n\
             local t = {{}}\n\
             for i = 1, {keys} do t['k' .. i] = text end\n\
             return t"
        )
    };
    let shared = |keys: usize| {
        format!(
            "local text = string.rep('x', 1024 * 1024) .. '\\255'\n\
             local t = {{ f = function() end }}\n\
             for i = 1, {keys} do t['k' .. i] = text end\n\
             return t"
        )
    };
    let keyed = |tables: usize, fault: &str| {
        format!(
            "local name = string.rep('k', 1024 * 1024)\n\
             local t = {{ {fault} }}\n\
             for i = 1, {tables} do t['k' .. i] = {{ [name] = 1 }} end\n\
             return t"
        )
    };
    // Whether the conversion reads an entry of a table with a gap differs
    // between two VMs, so both walks count what it does not read too:
    // here, what a list holds under a name. That is 5 MiB of text, or
    // 1,171 tables of six numbers, which are 8,197 values against the
    // 8,192 that a 64 KiB heap gives a walk.
    let unread = |item: &str, held: &str| format!("{held}\nreturn {{ {item}, x = held }}");
    let text = "local text = string.rep('x', 1024 * 1024)\n\
                local held = { text, text, text, text, text }";
    let values = "local leaf, held = { 1, 2, 3, 4, 5, 6 }, {}\n\
                  for i = 1, 1171 do held[i] = leaf end";
    for state in 0..STATES / 2 {
        let lua = Lua::new();
        lua.set_app_data(HeapCeiling(4 * 1024 * 1024));
        assert_eq!(
            refusal(&lua, &shared(64)),
            "the value is too large to convert",
            "state {state}"
        );
        for item in ["'a'", "function() end"] {
            assert_eq!(
                refusal(&lua, &unread(item, text)),
                "the value is too large to convert",
                "state {state}, {item}"
            );
        }
        for (tables, fault) in [(64, "f = function() end"), (5, "")] {
            assert_eq!(
                refusal(&lua, &keyed(tables, fault)),
                "the value is too large to convert",
                "state {state}, {tables} tables, {fault}"
            );
        }
        assert_eq!(
            refusal(&lua, &shared(3)),
            "unsupported value type `function`",
            "state {state}"
        );
        assert_eq!(
            refusal(&lua, &clean(5)),
            "the value is too large to convert",
            "state {state}"
        );
        let lengths = converted(&lua, &clean(3))
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, text)| text.as_str().map(str::len))
            .collect::<Vec<_>>();
        assert_eq!(lengths, [Some(1024 * 1024); 3], "state {state}");
        lua.set_app_data(HeapCeiling(64 * 1024));
        for item in ["'a'", "function() end"] {
            assert_eq!(
                refusal(&lua, &unread(item, values)),
                "the value is too large to convert",
                "state {state}, {item}"
            );
        }
    }
}

#[test]
fn a_shared_mode_scanned_past_the_byte_budget_is_too_large_in_every_vm() {
    // Both walks scan the `__mode` of a table's metatable to tell whether
    // the table is weak, once for each reference to the table. Five
    // references to one table with a `__mode` of 1 MiB are 5 MiB against
    // the 4 MiB of a 4 MiB heap, with a fault beside them or without, and
    // three are within it.
    let shared = |references: usize, fault: &str| {
        format!(
            "local value = setmetatable({{}}, {{ __mode = string.rep('x', 1024 * 1024) }})\n\
             local t = {{ {fault} }}\n\
             for i = 1, {references} do t['k' .. i] = value end\n\
             return t"
        )
    };
    for state in 0..STATES / 2 {
        let lua = Lua::new();
        lua.set_app_data(HeapCeiling(4 * 1024 * 1024));
        for fault in ["", "f = function() end"] {
            assert_eq!(
                refusal(&lua, &shared(5, fault)),
                "the value is too large to convert",
                "state {state}, {fault}"
            );
        }
        assert_eq!(
            converted(&lua, &shared(3, "")),
            json!({ "k1": {}, "k2": {}, "k3": {} }),
            "state {state}"
        );
    }
}

/// The value `source` returns.
fn value(lua: &Lua, source: &str) -> Value {
    lua.load(source).eval().expect("the value builds")
}

/// The data of the value `source` returns.
fn converted(lua: &Lua, source: &str) -> Json {
    data_of(lua, value(lua, source)).expect("the value has a JSON form")
}

/// Why the value `source` returns has no JSON form.
fn refusal(lua: &Lua, source: &str) -> String {
    match data_of(lua, value(lua, source)).expect_err("the value has no JSON form") {
        mlua::Error::DeserializeError(reason) => reason,
        other => panic!("not a refusal: {other}"),
    }
}
