//! Tests for the deterministic `pairs`/`next` installer.

use mlua::{LuaSerdeExt, MetaMethod, UserData, UserDataMethods, Value};
use serde_json::json;

use super::{ITERATION_CHUNK_NAME, ITERATION_SOURCE};
use crate::error::root_text;
use crate::test_vm::{BuiltVm, TestVm};
use crate::tests::assert_chunk_name_resolves;
use crate::vm::DEFAULT_LUA_LOG_EVENTS;
use crate::walk_budget::DEFAULT_LUA_MEMORY_BYTES;

/// A userdata whose `__pairs` yields `(i, i * 10)` for `i` in `1..=count`.
struct Counted {
    count: i64,
}

impl UserData for Counted {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            let count = this.count;
            let mut index = 0;
            lua.create_function_mut(move |_, ()| {
                index += 1;
                Ok(if index <= count {
                    (Value::Integer(index), Value::Integer(index * 10))
                } else {
                    (Value::Nil, Value::Nil)
                })
            })
        });
    }
}

/// A userdata with no metamethods.
struct Opaque;

impl UserData for Opaque {}

/// Evaluates a chunk that returns a sequence table and reads it back as JSON.
#[track_caller]
fn sequence(vm: &BuiltVm, source: &str) -> Vec<serde_json::Value> {
    let value: Value = vm.eval(source);
    vm.lua().from_value(value).expect("the result is JSON data")
}

#[test]
fn pairs_visits_string_keys_in_byte_order() {
    // Two insertion orders produce the same key sequence: the order is a
    // function of the keys, not of the table's hash state.
    let vm = TestVm::new().build();
    let source = "local out = {}; for k in pairs({zeta=1, alpha=2, mid=3, beta=4}) \
                  do out[#out+1] = k end; return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!("alpha"), json!("beta"), json!("mid"), json!("zeta")]
    );
    let source = "local out = {}; for k in pairs({beta=4, mid=3, alpha=2, zeta=1}) \
                  do out[#out+1] = k end; return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!("alpha"), json!("beta"), json!("mid"), json!("zeta")]
    );
}

#[test]
fn pairs_visits_the_array_part_before_the_hash_part() {
    let vm = TestVm::new().build();
    let source = "local out = {}; for k in pairs({10, 20, 30, extra='x', another='y'}) \
                  do out[#out+1] = k end; return out";
    assert_eq!(
        sequence(&vm, source),
        vec![
            json!(1),
            json!(2),
            json!(3),
            json!("another"),
            json!("extra")
        ]
    );
}

#[test]
fn pairs_orders_mixed_key_types_indices_then_booleans_then_numbers_then_strings() {
    let vm = TestVm::new().build();
    let source = "local out = {}; \
                  for k in pairs({[true]='t', [7]='seven', b='bee', [false]='f', [2.5]='half', a='ay', [0]='zero'}) \
                  do out[#out+1] = k end; return out";
    assert_eq!(
        sequence(&vm, source),
        vec![
            json!(7),
            json!(false),
            json!(true),
            json!(0),
            json!(2.5),
            json!("a"),
            json!("b")
        ]
    );
}

#[test]
fn pairs_honors_a_pairs_metamethod() {
    // The table is empty, so only the metamethod can yield the two pairs; a
    // stock `pairs` would return nothing.
    let vm = TestVm::new().build();
    let source = "local t = setmetatable({}, {__pairs = function() \
                    local i = 0 \
                    return function() i = i + 1; if i <= 2 then return i, i * 10 end end \
                  end}) \
                  local out = {} \
                  for k, v in pairs(t) do out[#out+1] = k .. ':' .. v end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("1:10"), json!("2:20")]);
}

#[test]
fn pairs_and_next_walk_a_sealed_view_as_the_table_it_stands_for() {
    let vm = TestVm::new().build();
    let source = "local view = setmetatable({}, {__index = {10, 20, extra = 'x'}, __metatable = 'view'}) \
                  local out = {} \
                  for k, v in pairs(view) do out[#out+1] = k .. ':' .. v end \
                  local first = next(view) \
                  out[#out+1] = 'first ' .. first \
                  out[#out+1] = 'last ' .. tostring(next(view, 'extra')) \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![
            json!("1:10"),
            json!("2:20"),
            json!("extra:x"),
            json!("first 1"),
            json!("last nil"),
        ]
    );
}

#[test]
fn pairs_walks_a_table_whose_index_is_not_sealed_as_itself() {
    // Without `__metatable` the table is no view, and a view whose data
    // only a function can read holds no table to walk.
    let vm = TestVm::new().build();
    let source = "local open = setmetatable({own = 1}, {__index = {other = 2}}) \
                  local computed = setmetatable({}, {__index = function() return 1 end, __metatable = 'view'}) \
                  local out = {} \
                  for k in pairs(open) do out[#out+1] = k end \
                  for k in pairs(computed) do out[#out+1] = k end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("own")]);
}

#[test]
fn a_pairs_metamethod_on_a_sealed_view_still_wins() {
    let vm = TestVm::new().build();
    let source = "local view = setmetatable({}, {__index = {a = 1}, __metatable = 'view', \
                    __pairs = function() \
                      local done = false \
                      return function() if not done then done = true; return 'own', 'walk' end end \
                    end}) \
                  local out = {} \
                  for k, v in pairs(view) do out[#out+1] = k .. ':' .. v end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("own:walk")]);
}

#[test]
fn pairs_honors_a_userdata_pairs_metamethod() {
    let vm = TestVm::new().build();
    let counted = vm
        .lua()
        .create_userdata(Counted { count: 2 })
        .expect("the userdata is created");
    vm.lua()
        .globals()
        .set("counted", counted)
        .expect("the global is set");
    let source = "local out = {} \
                  for k, v in pairs(counted) do out[#out+1] = k .. ':' .. v end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("1:10"), json!("2:20")]);
}

#[test]
fn pairs_refuses_a_userdata_without_a_pairs_metamethod() {
    let vm = TestVm::new().build();
    let opaque = vm
        .lua()
        .create_userdata(Opaque)
        .expect("the userdata is created");
    vm.lua()
        .globals()
        .set("opaque", opaque)
        .expect("the global is set");
    let message = vm.eval_err("return pairs(opaque)").to_string();
    assert!(
        message.contains("bad argument #1 to 'pairs' (table expected, got userdata)"),
        "the refusal must name the expected and actual types: {message}"
    );
}

#[test]
fn next_is_nil_for_an_empty_table() {
    let vm = TestVm::new().build();
    let source = "local t = {}; return {next(t) == nil, next(t, nil) == nil}";
    assert_eq!(sequence(&vm, source), vec![json!(true), json!(true)]);
}

#[test]
fn next_returns_the_first_pair_of_a_non_empty_table() {
    let vm = TestVm::new().build();
    let source = "local k, v = next({only=1}); return {k == 'only', v == 1}";
    assert_eq!(sequence(&vm, source), vec![json!(true), json!(true)]);
}

#[test]
fn next_resumes_after_a_cleared_boolean_key_without_replaying_the_array_part() {
    // The stateless `next` rebuilds the walk from the live table. A boolean
    // hash key ranks below numbers in `SortKey`, so a resume that compared raw
    // sort positions would place the array segment after the cleared boolean
    // and replay it. Clearing each boolean key mid-walk must still visit every
    // key exactly once and stop at the end.
    let vm = TestVm::new().build();
    let source = "local t = {10, 20, [false] = 'f', [true] = 't'} \
                  local out = {} \
                  for k in next, t do \
                    if type(k) == 'boolean' then \
                      out[#out+1] = tostring(k) \
                      t[k] = nil \
                    else \
                      out[#out+1] = k \
                    end \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!(1), json!(2), json!("false"), json!("true")]
    );
}

#[test]
fn next_visits_a_live_boolean_key_after_array_elements_are_cleared() {
    // `next` resumed using the live `#t` as the array border: clearing an
    // array element shrinks that border, so a cleared integer key reclassified
    // as a scalar number and the resume stopped before the boolean hash key.
    // An integer key must stay in the array segment regardless of the mutable
    // length hint, so the walk still reaches the live `false`.
    let vm = TestVm::new().build();
    let source = "local t = {10, 20, [false] = 'f'} \
                  local out = {} \
                  for k in next, t do \
                    out[#out+1] = tostring(k) \
                    t[k] = nil \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!("1"), json!("2"), json!("false")]
    );
    let source = "local t = {10, [false] = 'f'} \
                  local out = {} \
                  for k in next, t do \
                    out[#out+1] = tostring(k) \
                    t[k] = nil \
                  end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("1"), json!("false")]);
}

#[test]
fn next_meets_every_key_once_when_the_loop_clears_keys() {
    // A key has its place whatever else the table holds, so a cleared key
    // moves no other. With a place for `7` that asked whether the table
    // held it, the walk went back from the cleared `7` to `false` and met
    // `false` and `3` twice. With a place for `3` that asked where the
    // list ends, a cleared `1` would put `3` behind `false`.
    let vm = TestVm::new().build();
    for (table, cleared_at, cleared, expected) in [
        ("{ [false] = 'b', [3] = 'c', [7] = 'y' }", 7, 7, "3,7,false"),
        ("{ 10, 20, 30, [false] = 'f' }", 3, 1, "1,2,3,false"),
    ] {
        let source = format!(
            "local t = {table} \
             local out = {{}} \
             for k in next, t do \
               out[#out+1] = tostring(k) \
               if k == {cleared_at} then t[{cleared}] = nil end \
             end \
             return table.concat(out, ',')"
        );
        assert_eq!(vm.eval::<String>(&source), expected, "{table}");
    }
}

#[test]
fn pairs_skips_a_key_cleared_before_it_is_visited() {
    let vm = TestVm::new().build();
    let source = "local t = {a=1, b=2, c=3} \
                  local out = {} \
                  for k in pairs(t) do \
                    out[#out+1] = k \
                    if k == 'a' then t.b = nil end \
                  end \
                  return out";
    assert_eq!(sequence(&vm, source), vec![json!("a"), json!("c")]);
}

#[test]
fn pairs_skips_the_key_cleared_by_the_current_step() {
    // Clearing the current key leaves the previous-key cursor pointing at a
    // key no longer in the table; the walk resumes at the next live key
    // instead of erroring or dropping the rest of the traversal.
    let vm = TestVm::new().build();
    let source = "local t = {a=1, b=2, c=3} \
                  local out = {} \
                  for k in pairs(t) do \
                    out[#out+1] = k \
                    t[k] = nil \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!("a"), json!("b"), json!("c")]
    );
}

#[test]
fn pairs_does_not_revisit_the_array_part_when_a_boolean_key_is_cleared() {
    // A boolean hash key ranks below numbers in `SortKey`, so a resumption
    // that compares raw sort positions places the array part after the
    // boolean key and replays it. The array part, the boolean key, and the
    // clear must all coexist for the defect to surface; each key must still
    // be yielded exactly once.
    let vm = TestVm::new().build();
    let source = "local t = {10, 20, [false] = 'f'} \
                  local out = {} \
                  for k in pairs(t) do \
                    out[#out+1] = k \
                    if k == false then t[k] = nil end \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!(1), json!(2), json!(false)]
    );
}

#[test]
fn pairs_and_next_refuse_a_table_with_a_key_only_its_address_tells_apart() {
    // Lua orders two table keys by a hash of their addresses, so a walk
    // over them would differ from run to run. The table stays readable by
    // key.
    let vm = TestVm::new().build();
    vm.exec("key = {}; set = {a = 1, [key] = true}");
    let in_the_table = |function: &str, type_name: &str| {
        format!(
            "'{function}' walks boolean, number, and string keys only: the table has a \
             {type_name} key, which has no order that is the same in two runs"
        )
    };
    assert_eq!(refusal(&vm, "pairs(set)"), in_the_table("pairs", "table"));
    assert_eq!(refusal(&vm, "next(set)"), in_the_table("next", "table"));
    assert_eq!(
        refusal(&vm, "next(set, 'a')"),
        in_the_table("next", "table")
    );
    // Of two such keys, Lua's own walk meets either first, and the refusal
    // names the same one.
    assert_eq!(
        refusal(&vm, "pairs({[{}] = 1, [type] = 1, a = 1})"),
        in_the_table("pairs", "function")
    );
    assert_eq!(
        refusal(&vm, "next({a = 1}, type)"),
        "'next' walks boolean, number, and string keys only: got a function key to go on from"
    );
    assert!(vm.eval::<bool>("return set[key]"));
}

#[test]
fn pairs_places_an_infinite_number_and_a_string_that_is_not_utf8_by_value() {
    // Neither key has a label, so the shared `sort_key` refuses both, but
    // each has a value to sort by. Left to Lua's own order, the two strings
    // would come out as its string hash placed them.
    let vm = TestVm::new().build();
    let source = "local t = {['\\xff'] = 1, b = 1, ['\\xfe'] = 1, [1/0] = 1, [5] = 1, [-1/0] = 1} \
                  local out = {} \
                  for k in pairs(t) do \
                    out[#out+1] = type(k) == 'string' and string.byte(k) or tostring(k) \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![
            json!("5"),
            json!("-inf"),
            json!("inf"),
            json!(98),
            json!(254),
            json!(255),
        ]
    );
}

#[test]
fn next_ends_a_walk_over_an_integer_and_a_float_that_round_to_one_float() {
    // `math.maxinteger` as a float is `0x1p63`. A `next` that took the two
    // keys for one went on from the integer again and never ended.
    let vm = TestVm::new().build();
    let source = "local t = {[math.maxinteger] = 'integer', [0x1p63] = 'float', [-1] = 'low'} \
                  local out, key, value = {}, nil, nil \
                  for _ = 1, 5 do \
                    key, value = next(t, key) \
                    if key == nil then break end \
                    out[#out+1] = value \
                  end \
                  return out";
    assert_eq!(
        sequence(&vm, source),
        vec![json!("integer"), json!("low"), json!("float")]
    );
}

#[test]
fn next_takes_a_float_with_the_value_of_an_integer_for_that_integers_key() {
    let vm = TestVm::new().build();
    let source = "local t = {10, 20, 30, [-2] = 'low', x = 'last'} \
                  return {(next(t, 2.0)), (next(t, 3.0)), (next(t, -2.0))}";
    assert_eq!(sequence(&vm, source), vec![json!(3), json!(-2), json!("x")]);
}

#[test]
fn walks_left_early_keep_no_slot_for_a_key_of_their_table() {
    // `mlua` panics when it cannot grow the stack that holds the Lua
    // values Rust refers to, and near the memory limit that stack stops
    // growing early. Lua collects an iterator some time after the loop
    // leaves it, so iterators that each held their table's string keys
    // filled the stack.
    let vm = TestVm::new().build();
    let fill = DEFAULT_LUA_MEMORY_BYTES - 4 * 1024 * 1024;
    let walks: i64 = vm.eval(&format!(
        "local fill = string.rep('x', {fill}) \
         local t = {{}} for i = 1, 2000 do t['k' .. i] = i end \
         local walks = 0 \
         for _ = 1, 150 do for _ in pairs(t) do walks = walks + 1 break end end \
         return walks + #fill * 0"
    ));
    assert_eq!(walks, 150);
}

#[test]
fn a_walk_yields_the_tables_own_key_and_makes_no_copy_of_it() {
    // The key takes more than half of the VM's memory, so a second string
    // of its length does not fit.
    let vm = TestVm::new().build();
    let long = DEFAULT_LUA_MEMORY_BYTES / 8 * 5;
    let same: Vec<bool> = vm.eval(&format!(
        "local key = string.rep('x', {long}) \
         local t = {{ [key] = 1 }} \
         local walked \
         for k in pairs(t) do walked = k end \
         return {{ (next(t)) == key, walked == key, next(t, key) == nil }}"
    ));
    assert_eq!(same, [true, true, true]);
}

#[test]
fn a_weak_table_is_refused_by_a_walk_and_read_by_key() {
    let vm = TestVm::new().build();
    let refusal = |function: &str| {
        format!(
            "'{function}' does not walk a weak table, one whose metatable sets __mode: the \
             entries the collector leaves in it are not the same in two runs"
        )
    };
    for mode in ["k", "v", "kv"] {
        for function in ["pairs", "next"] {
            let error = vm.eval_err(&format!(
                "return {function}(setmetatable({{ a = 1 }}, {{ __mode = '{mode}' }}))"
            ));
            assert_eq!(root_text(&error), refusal(function), "{function} {mode}");
        }
    }
    // An empty weak table with `__index` and `__metatable` has the shape
    // of a view. It is refused, and so is a view of it: the walk would
    // read the `__index` table only while the collector left the weak
    // table empty.
    for function in ["pairs", "next"] {
        for table in [
            "weak",
            "setmetatable({}, { __metatable = false, __index = weak })",
        ] {
            let error = vm.eval_err(&format!(
                "local weak = setmetatable({{}}, \
                   {{ __mode = 'v', __metatable = false, __index = {{ a = 1 }} }}) \
                 return {function}({table})"
            ));
            assert_eq!(root_text(&error), refusal(function), "{function} {table}");
        }
    }
    // Lua reads `__mode` when it collects, so a table is weak from the
    // moment its metatable names a mode, and no longer once it names none.
    // Lua reads the field up to its first zero byte.
    let late: Vec<String> = vm.eval(
        "local meta = {} \
         local t = setmetatable({ a = 1 }, meta) \
         local before = next(t) \
         meta.__mode = 'v' \
         local walked = pcall(pairs, t) \
         local read = t.a \
         meta.__mode = 'x\\0k' \
         return { before, tostring(walked), tostring(read), (next(t)) }",
    );
    assert_eq!(late, ["a", "false", "1", "a"]);
}

#[test]
fn a_chain_of_views_with_more_mode_than_one_call_scans_is_refused() {
    // Each view on the way to the data has a metatable of its own, and
    // all of them share one `__mode` of 1 MiB that makes no table weak.
    // The read scans it at each view: four views are the 4 MiB that a
    // 4 MiB heap gives one call, and five are past it.
    let vm = TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .build();
    vm.exec(
        "local mode = string.rep('x', 2^20) \
         function chain(views, longer) \
           local mode = longer and mode .. 'x' or mode \
           local t = { a = 1 } \
           for _ = 1, views do \
             t = setmetatable({}, { __index = t, __metatable = false, __mode = mode }) \
           end \
           return t \
         end",
    );
    let walked = vm.eval::<Vec<String>>(
        "local keys = { (next(chain(4))) } \
         for key in pairs(chain(4)) do keys[#keys + 1] = key end \
         return keys",
    );
    assert_eq!(walked, ["a", "a"]);
    for function in ["pairs", "next"] {
        let error = vm.eval_err(&format!("return {function}(chain(5))"));
        assert_eq!(
            root_text(&error),
            format!(
                "'{function}' does not walk a table with 5242880 bytes of __mode to scan: one \
                 call scans at most 4194304 bytes"
            )
        );
    }
    // The count is the bytes that the call came to: with one byte more
    // of `__mode`, three scans and the fourth, which is past what they
    // left.
    assert_eq!(
        root_text(&vm.eval_err("return pairs(chain(4, true))")),
        "'pairs' does not walk a table with 4194308 bytes of __mode to scan: one call scans at \
         most 4194304 bytes"
    );
}

#[test]
fn a_table_with_a_gap_in_its_indices_walks_the_same_way_in_every_vm() {
    // `#t` of this table is 3 in one VM and 5 in another: the indices sit
    // in the hash part, where Lua looks for the end of the list with the
    // seed of the state. A walk that put the indices up to `#t` first
    // visited `5` before `false` in one VM and after it in another.
    for state in 0..32 {
        let vm = TestVm::new().build();
        let walks: String = vm.eval(
            "local t = { [1] = 1, [2] = 2, [3] = 3, [5] = 5, [false] = false } \
             local by_pairs, by_next = {}, {} \
             for key in pairs(t) do by_pairs[#by_pairs + 1] = tostring(key) end \
             for key in next, t do by_next[#by_next + 1] = tostring(key) end \
             return table.concat(by_pairs, ',') .. ' ' .. table.concat(by_next, ',')",
        );
        assert_eq!(walks, "1,2,3,5,false 1,2,3,5,false", "state {state}");
    }
}

#[test]
fn a_next_loop_and_a_pairs_loop_visit_the_same_keys_in_the_same_order() {
    // The two walks share no code past the order of two keys: `pairs`
    // sorts a list, and `next` looks for the least key after the one it
    // was given. The table mixes every kind of key, and a list with holes.
    let vm = TestVm::new().build();
    let (by_pairs, by_next, count): (String, String, i64) = vm.eval(
        "local t, count = {}, 0 \
         local function put(key) if t[key] == nil then count = count + 1 end t[key] = true end \
         for i = 1, 40 do if i % 7 ~= 0 then put(i) end end \
         local seed = 12345 \
         for _ = 1, 300 do \
           seed = (seed * 1103515245 + 12345) % 2147483648 \
           local kind, n = seed % 6, seed // 6 % 1000 \
           if kind == 0 then put(n - 500) \
           elseif kind == 1 then put(n / 8 - 60) \
           elseif kind == 2 then put('k' .. n) \
           elseif kind == 3 then put(n % 2 == 0) \
           elseif kind == 4 then put(math.maxinteger - n) \
           else put(2.0 ^ 63 + n * 4096) end \
         end \
         local function text(key) return math.type(key) == 'float' and ('%a'):format(key) or tostring(key) end \
         local a, b = {}, {} \
         for key in pairs(t) do a[#a + 1] = text(key) end \
         local key = next(t) \
         while key ~= nil do b[#b + 1] = text(key) key = next(t, key) end \
         assert(#a == count, 'pairs visits every key once') \
         return table.concat(a, ' '), table.concat(b, ' '), count",
    );
    assert_eq!(by_next, by_pairs);
    assert_eq!(
        by_pairs.split(' ').count(),
        usize::try_from(count).expect("a count")
    );
}

#[test]
fn the_chunk_name_resolves_to_the_embedded_file() {
    assert_chunk_name_resolves(
        "ITERATION_CHUNK_NAME",
        ITERATION_CHUNK_NAME,
        ITERATION_SOURCE,
    );
}

/// The text of the failure `source` raises.
#[track_caller]
fn refusal(vm: &BuiltVm, source: &str) -> String {
    root_text(&vm.eval_err(source))
}
