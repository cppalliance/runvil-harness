//! The sandboxed global environment: no direct output, the safe standard
//! library without the dangerous globals, `setmetatable`'s refusals, and
//! the `untrusted` global.

use super::*;
use crate::error::author_prose;

#[test]
fn direct_output_is_absent_in_every_executable_lua_vm() {
    let library = program("assert(print == nil); assert(warn == nil); log('library load')");
    let library_vm = TestVm::new().build();
    run_scalar(&library_vm, &library, &null_emitter(), "Section")
        .expect("library VM must not expose direct output");
    library_vm.teardown();

    let vm = TestVm::new()
        .tools(fixture_set(&[("search", "search the web")]))
        .build();
    run_scalar(
        &vm,
        &program("assert(print == nil); assert(warn == nil)"),
        &null_emitter(),
        "Section",
    )
    .expect("prologue must not expose direct output");
    run_scalar(
        &vm,
        &program("assert(print == nil); assert(warn == nil)"),
        &null_emitter(),
        "Section",
    )
    .expect("epilog must not expose direct output");
    vm.teardown();

    assert_eq!(
        run(
            "return tostring(print) .. ':' .. tostring(warn)",
            &json!({})
        )
        .expect("compatibility VM must run")
        .returned
        .as_deref(),
        Some("nil:nil")
    );
}

#[test]
fn safe_stdlib_present() {
    let out = run("return string.upper(args.text)", &json!({ "text": "hi" })).unwrap();
    assert_eq!(out.returned.as_deref(), Some("HI"));
}

#[test]
fn the_utf8_library_counts_cuts_and_encodes_code_points() {
    let out = run(
        "return utf8.len('café') .. '|' .. ('café'):sub(1, utf8.offset('café', 0, 5) - 1) \
         .. '|' .. utf8.char(233)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("4|caf|é"));
}

#[test]
fn dangerous_globals_absent() {
    let out = run(
            "return tostring(io) .. ',' .. tostring(os) .. ',' .. tostring(require) .. ',' .. tostring(load) .. ',' .. tostring(package)",
            &json!({}),
        )
        .unwrap();
    assert_eq!(out.returned.as_deref(), Some("nil,nil,nil,nil,nil"));
}

#[test]
fn math_random_draws_only_from_the_seed_the_vm_was_given() {
    let vm = SectionVm::new(&test_nonce(), &null_emitter(), "Test").expect("VM must build");
    let eval = |source: &str| {
        vm.uninjected_lua()
            .load(source)
            .eval::<String>()
            .expect("the chunk runs")
    };
    // Every expected draw below was computed outside this crate with a
    // reference xoshiro256**. `math.random(0)` is one whole 64-bit draw.
    // A VM nothing seeded draws from the fixed seed (0, 0).
    let two_draws = "return math.random(0) .. ' ' .. math.random(0)";
    assert_eq!(eval(two_draws), "4554719557422691265 4331835599999590920");
    vm.seed_random([-3_572_835_231_721_551_567, -8_962_037_305_301_457_989])
        .expect("the seed installs");
    assert_eq!(eval(two_draws), "-2538964283181059592 -1382995782422572602");
    // With no argument the new seed is the generator's next two draws,
    // which it returns, and the draw after it follows from them.
    assert_eq!(
        eval("local n1, n2 = math.randomseed() return n1 .. ' ' .. n2 .. ' ' .. math.random(0)"),
        "-2860266774601705052 7099798641931708825 532391193148689082"
    );
}

#[test]
fn table_sort_keeps_elements_that_compare_equal_in_their_order() {
    // Stock Lua picks a pivot from the clock and an address once a
    // partition comes out unbalanced, which this list makes happen: three
    // small keys at the ends and the middle of 597 equal ones. The equal
    // elements then end in an order that differs from one run to the next.
    let sorted = run(
        "local list = {}\n\
         for i = 1, 600 do list[i] = { key = 1000, id = i } end\n\
         list[1].key, list[300].key, list[600].key = 1, 2, 3\n\
         table.sort(list, function(a, b) return a.key < b.key end)\n\
         local ids = {}\n\
         for i, element in ipairs(list) do ids[i] = element.id end\n\
         return table.concat(ids, ',')",
        &json!({}),
    )
    .expect("the block runs");
    // The stable order, from the definition and not from the sort: the
    // three small keys, then every other element where it was.
    let stable = [1, 300, 600]
        .into_iter()
        .chain((2..600).filter(|id| *id != 300))
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(sorted.returned.as_deref(), Some(stable.as_str()));
}

#[test]
fn table_sort_orders_by_less_than_or_by_the_comparison_function() {
    // `var.list` is a view, so the last sort reads and writes through
    // `__index` and `__newindex`.
    let sorted = run(
        "local numbers = { 3, 1.5, -2, 10, 1 }\n\
         table.sort(numbers)\n\
         local words = { 'pear', 'Apple', 'fig', 'apple' }\n\
         table.sort(words, function(a, b) return a > b end)\n\
         var.list = { 'c', 'a', 'b' }\n\
         table.sort(var.list)\n\
         table.sort({})\n\
         table.sort({ 1 }, 'no comparison is made')\n\
         store.write('order.txt', 'descending')\n\
         local waited = { 2, 3, 1 }\n\
         table.sort(waited, function(a, b)\n\
           if store.read('order.txt') == 'descending' then return a > b end\n\
           return a < b\n\
         end)\n\
         return table.concat(numbers, ' ') .. '|' .. table.concat(words, ' ')\n\
           .. '|' .. table.concat(var.list, ' ') .. '|' .. table.concat(waited, ' ')",
        &json!({}),
    )
    .expect("the block runs");
    // The last comparison function waits on the store at each call, which
    // stock Lua's C `table.sort` cannot yield across.
    assert_eq!(
        sorted.returned.as_deref(),
        Some("-2 1 1.5 3 10|pear fig apple Apple|a b c|3 2 1")
    );
}

#[test]
fn table_sort_and_table_concat_fail_on_the_callers_line_in_stock_luas_words() {
    for (call, reason) in [
        (
            "table.sort({ 1, 'x' })",
            "attempt to compare string with number",
        ),
        (
            "table.sort(nil)",
            "bad argument #1 to 'sort' (table expected, got nil)",
        ),
        (
            "table.sort({ 2, 1 }, 5)",
            "bad argument #2 to 'sort' (function expected, got number)",
        ),
        // A refused list is named as stock Lua names it, by the `__name`
        // of its metatable. A record has no `__len`.
        (
            "messages:user('a') table.sort(messages[1])",
            "bad argument #1 to 'sort' (table expected, got RecordView)",
        ),
        // The message list has `__index`, `__newindex`, and `__len`, so it
        // is a list to the sort, as it is to the stock one. The sort then
        // fails at what it does with the records.
        (
            "messages:user('a') messages:user('b') table.sort(messages)",
            "attempt to compare two RecordView values",
        ),
        (
            "messages:user('a') messages:user('b') \
             table.sort(messages, function() return true end)",
            "messages cannot be assigned to by index; add records with append(record), or \
             rewrite history with models.compact(compactor, reason)",
        ),
        // A `__len` that cannot be called fails where the sort takes the
        // list's length.
        (
            "table.sort(setmetatable({}, { __len = false }))",
            "attempt to call a boolean value",
        ),
        // A failure Lua raises in the sort's own read of the list, which a
        // comparison function does not put under a protected call.
        (
            "table.sort(setmetatable({}, { __len = function() return 3 end, __index = 5 }), \
             function(a, b) return a < b end)",
            "attempt to index a number value",
        ),
        (
            "table.concat(5)",
            "bad argument #1 to 'concat' (table expected, got number)",
        ),
        (
            "table.concat({}, '', {})",
            "bad argument #3 to 'concat' (number expected, got table)",
        ),
        (
            "table.concat({ 1, {} })",
            "invalid value (table) at index 2 in table for 'concat'",
        ),
        (
            "table.concat(setmetatable({}, { __len = function() return 1 end, __index = 5 }))",
            "attempt to index a number value",
        ),
    ] {
        let error =
            run(&format!("local first_line\n{call}"), &json!({})).expect_err("the call fails");
        assert_eq!(error.prose, refused(reason), "{call}");
        assert_eq!(error.at.map(|at| at.start_line.get()), Some(2), "{call}");
    }
    // A failure the comparison function raises stays on its own line.
    let error = run(
        "table.sort({ 2, 1 }, function(a, b)\n  error('boom')\nend)",
        &json!({}),
    )
    .expect_err("the comparison fails");
    assert_eq!(error.prose, refused("boom"));
    assert_eq!(error.at.map(|at| at.start_line.get()), Some(2));
    // The list is written once, after the last comparison, so a sort whose
    // comparison fails leaves it as it was.
    let kept = run(
        "local list = { 3, 1, 'x', 2 }\n\
         pcall(table.sort, list)\n\
         return table.concat(list, ' ')",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(kept.returned.as_deref(), Some("3 1 x 2"));
}

#[test]
fn tostring_and_string_format_print_no_address() {
    let shown = run(
        "local named = setmetatable({}, { __name = 'Named' })\n\
         local rendered = setmetatable({}, { __tostring = function() return 'rendered' end })\n\
         return table.concat({\n\
           tostring({}), tostring(log), tostring(json.null), tostring(messages),\n\
           tostring(named), tostring(rendered), tostring(nil), tostring(1.5),\n\
           string.format('%s %8s %-4s| %d%%', {}, log, rendered, 7),\n\
           ('%s'):format(named),\n\
         }, '|')",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(
        shown.returned.as_deref(),
        Some(
            "table|function|userdata|MessageList|Named|rendered|nil|1.5\
             |table function rendered| 7%|Named"
        )
    );
    // Stock Lua prints a `__name` as a C string, up to its first zero byte.
    // The whole name would fail a `%s` with a width, which takes no zero.
    let cut = run(
        "return string.format('%4s|', setmetatable({}, { __name = 'a\\0b' }))",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(cut.returned.as_deref(), Some("   a|"));
    let error = run("return string.format('%5p', {})", &json!({})).expect_err("%p is refused");
    assert_eq!(
        error.prose,
        refused("string.format: conversion '%p' is not available, it prints a memory address")
    );
    // The stock function's own refusals keep its name and the caller's line.
    let error = run(
        "local first_line\nreturn string.format('%d', {})",
        &json!({}),
    )
    .expect_err("the argument is refused");
    assert_eq!(
        error.prose,
        refused("bad argument #2 to 'format' (number expected, got table)")
    );
    assert_eq!(error.at.map(|at| at.start_line.get()), Some(2));
    // A failure a `__tostring` metamethod raises under `%s` is the
    // metamethod's: a string keeps its line and a table stays the table.
    let error = run(
        "local raising = setmetatable({}, { __tostring = function()\n\
           error('boom')\n\
         end })\n\
         return string.format('%s', raising)",
        &json!({}),
    )
    .expect_err("the metamethod fails");
    assert_eq!(error.prose, refused("boom"));
    assert_eq!(error.at.map(|at| at.start_line.get()), Some(2));
    let caught = run(
        "local raising = setmetatable({}, { __tostring = function() error({ code = 7 }) end })\n\
         local _, failure = pcall(string.format, '%s', raising)\n\
         return tostring(failure.code)",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(caught.returned.as_deref(), Some("7"));
}

#[test]
fn the_replacements_answer_as_stock_lua_does() {
    // Each case runs in a section VM and in a stock Lua, and the two must
    // give the same result or the same failure text. Two things are cut
    // before the comparison. One is the address a stock Lua prints after
    // the name of a table or a function. The other is the position of the
    // case's own chunk in front of a failure: a stock function, which is
    // C, puts none on a failure Lua raises inside it. Any other position
    // stays, so a failure that names the Engine's own chunk is a mismatch.
    for case in CASES {
        assert_eq!(
            in_a_section(case).as_deref(),
            Some(in_stock_lua(case).as_str()),
            "{case}"
        );
    }
}

#[test]
fn an_argument_error_cannot_follow_a_tail_call_or_a_call_metamethod() {
    // The calls that a replacement names otherwise than stock Lua does,
    // where every other call is a case of the comparison with stock Lua.
    // A replacement is a Lua function. A tail call of one leaves nothing
    // of the call to read, so the name is the one of the library's
    // field, and the guard of `_G` tail-calls a handler that is a
    // function, as the Engine's `xpcall` tail-calls its handler. And Lua
    // counts the value whose `__call` is the function among the
    // arguments, which stock Lua leaves out.
    for (case, ours, stock) in [
        (
            "local order = table.sort return order(5)",
            "bad argument #1 to 'sort' (table expected, got number)",
            "bad argument #1 to 'order' (table expected, got number)",
        ),
        (
            "setmetatable(_G, { __index = string.format }) return undefined_name",
            "bad argument #1 to 'format' (string expected, got table)",
            "bad argument #1 to 'index' (string expected, got table)",
        ),
        (
            "local _, failure = xpcall(function() error({}) end, string.format) error(failure, 0)",
            "bad argument #1 to 'format' (string expected, got table)",
            "bad argument #1 to 'string.format' (string expected, got table)",
        ),
        (
            "local callee = setmetatable({ 2, 1 }, { __call = table.sort }) callee(5)",
            "bad argument #2 to 'callee' (function expected, got number)",
            "bad argument #1 to 'callee' (function expected, got number)",
        ),
    ] {
        assert_eq!(
            in_a_section(case),
            Some(format!("failed string {ours}")),
            "{case}"
        );
        assert_eq!(
            in_stock_lua(case),
            format!("failed string {stock}"),
            "{case}"
        );
    }
}

#[test]
fn pcall_and_xpcall_refuse_their_arguments_on_the_line_of_the_call() {
    // The case is the second line of the section's chunk and the third of
    // the stock Lua's, which has one line more in front.
    for (call, refusal) in [
        ("pcall()", "bad argument #1 to 'pcall' (value expected)"),
        (
            "xpcall(type, 5)",
            "bad argument #2 to 'xpcall' (function expected, got number)",
        ),
    ] {
        let case =
            format!("return (select(2, pcall(function() local taken = {call} return taken end)))");
        assert_eq!(
            in_a_section(&case),
            Some(format!("returned string [string \"line 1\"]:2: {refusal}")),
            "{call}"
        );
        assert_eq!(
            in_stock_lua(&case),
            format!("returned string stock:3: {refusal}"),
            "{call}"
        );
    }
}

#[test]
fn a_replacement_stands_under_no_protected_call_while_the_authors_code_runs() {
    // The handler of the author's `xpcall` runs where the failure is
    // raised, so the variable the metamethod holds is still open, as it is
    // under the stock function. A protected call in the replacement would
    // close it first and raise the failure again from its own frame.
    for (event, call) in [
        ("__tostring", "tostring(value)"),
        ("__tostring", "string.format('%d %s', 1, value)"),
        ("__lt", "table.sort({ value, value })"),
        ("__len", "table.sort(value)"),
        ("__index", "table.sort(value)"),
        (
            "__newindex",
            "table.sort(value, function() return true end)",
        ),
        ("__len", "table.concat(value)"),
        ("__index", "table.concat(value)"),
    ] {
        let case = held_open(event, call);
        let ours = in_a_section(&case);
        assert_eq!(
            ours.as_deref(),
            Some("returned string open"),
            "{event} {call}"
        );
        assert_eq!(
            ours.as_deref(),
            Some(in_stock_lua(&case).as_str()),
            "{event} {call}"
        );
    }
    // Stock `table.concat` takes no table, so these have no stock answer.
    // The second fails on the line of the call: the position of the
    // Engine's own chunk would stay in front of the text.
    assert_eq!(
        in_a_section(&held_open("__tostring", "table.concat({ value })")).as_deref(),
        Some("returned string open")
    );
    assert_eq!(
        in_a_section(
            "return (table.concat({ setmetatable({}, { __tostring = function() return {} end }) }))"
        )
        .as_deref(),
        Some("failed string '__tostring' must return a string")
    );
    // The `__tostring` of an element is read as Lua finds it, so a
    // `__metatable` field does not cover it.
    assert_eq!(
        in_a_section(
            "return (table.concat({ 'a', setmetatable({}, { \
               __metatable = 'sealed', __tostring = function() return 'b' end }) }, '-'))"
        )
        .as_deref(),
        Some("returned string a-b")
    );
    // A `__tostring` of an element gives the list a metatable, so the
    // read of the next element is one that Lua can refuse.
    assert_eq!(
        in_a_section(
            "local list = {} \
             list[1] = setmetatable({}, { __tostring = function() \
               setmetatable(list, { __index = 5 }) \
               return 'first' \
             end }) \
             return (table.concat(list, ' ', 1, 2))"
        )
        .as_deref(),
        Some("failed string attempt to index a number value")
    );
    // The Engine reads the failing line where the failure is raised, so a
    // value with no position in it is placed on the metamethod's line.
    for (event, call) in [
        ("__tostring", "tostring(value)"),
        ("__tostring", "string.format('%s', value)"),
        ("__tostring", "table.concat({ value })"),
        ("__lt", "table.sort({ value, value })"),
        ("__len", "table.sort(value)"),
        ("__index", "table.sort(value)"),
    ] {
        for (raised, reason) in [
            ("error('boom', 0)", "boom"),
            ("error('boom', 2)", "boom"),
            ("error({})", "error object is a table value"),
        ] {
            let block = format!(
                "local meta = {{ __len = function() return 2 end }}\n\
                 meta.{event} = function()\n\
                   {raised}\n\
                 end\n\
                 local value = setmetatable({{}}, meta)\n\
                 return {call}"
            );
            let error = run(&block, &json!({})).expect_err("the metamethod fails");
            assert_eq!(error.prose, refused(reason), "{event} {call} {raised}");
            assert_eq!(
                error.at.map(|at| at.start_line.get()),
                Some(3),
                "{event} {call} {raised}"
            );
        }
    }
}

#[test]
fn a_metamethod_waits_where_a_replacement_calls_it_from_lua() {
    // A call that waits yields the block's coroutine. The replacements
    // call a `__tostring`, a `__len`, and a `__lt` from Lua, and
    // `table.concat` and `table.sort` read a list that has a metatable
    // from Lua, and the sort writes it from Lua, where stock Lua does
    // each from a C function, which a yield cannot cross.
    let waited = run(
        "store.write('text.txt', 'stored')\n\
         local value = setmetatable({ 'b', 'a' }, {\n\
           __tostring = function() return store.read('text.txt') end,\n\
           __len = function() return #store.read('text.txt') - 4 end,\n\
           __lt = function() return store.read('text.txt') ~= 'stored' end,\n\
         })\n\
         local read = setmetatable({}, {\n\
           __len = function() return 2 end,\n\
           __index = function() return store.read('text.txt') end,\n\
         })\n\
         local texts = { tostring(value), string.format('%s', value), table.concat({ value, value }, ','),\n\
           table.concat(read, '+') }\n\
         table.sort(value)\n\
         table.sort({ value, value })\n\
         local held = { 'd', 'c' }\n\
         table.sort(setmetatable({}, {\n\
           __len = function() return 2 end,\n\
           __index = function(_, key) return store.read('text.txt') and held[key] end,\n\
           __newindex = function(_, key, item) held[key] = store.read('text.txt') and item end,\n\
         }))\n\
         return table.concat(texts, '|') .. '|' .. table.concat(value) .. '|' .. table.concat(held)",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(
        waited.returned.as_deref(),
        Some("stored|stored|stored,stored|stored+stored|ab|cd")
    );
}

#[test]
fn a_replacement_is_one_frame_with_no_position_to_the_authors_code() {
    // `error(text, level)` counts frames from the code that calls it, and
    // puts the position of the frame at `level` in front of the text.
    // Under a stock function, which is C, level 1 is the metamethod,
    // level 2 the stock function, which has no position, and level 3 the
    // author's call. Level 4 is the `pcall` around that call, which is C.
    let expected = [
        r#"[string "line 1"]:2: boom"#,
        "boom",
        r#"[string "line 1"]:5: boom"#,
        "boom",
    ];
    for (event, call, stock_takes_it) in [
        ("__tostring", "tostring(value)", true),
        ("__tostring", "string.format('%d %s', 1, value)", true),
        ("__tostring", "table.concat({ value })", false),
        ("__len", "table.concat(value)", true),
        ("__index", "table.concat(value)", true),
        ("__len", "table.sort(value)", true),
        ("__index", "table.sort(value)", true),
        ("__newindex", "table.sort(value)", true),
        ("__lt", "table.sort({ value, value })", true),
        ("comparison", "table.sort({ 2, 1 }, meta.comparison)", true),
    ] {
        for (level, expected) in (1..).zip(expected) {
            // The list reads as 2, 1, so the stock sort, which writes
            // only what it moves, writes it.
            let block = format!(
                "local meta = {{ __len = function() return 2 end, __index = function(_, key) return 3 - key end }}\n\
                 meta.{event} = function() error('boom', {level}) end\n\
                 local value = setmetatable({{}}, meta)\n\
                 local _, failure = pcall(function()\n\
                   local result = {call}\n\
                   return result\n\
                 end)\n\
                 return failure"
            );
            let ours = run(&block, &json!({})).expect("the block runs").returned;
            assert_eq!(ours.as_deref(), Some(expected), "{event} {call} {level}");
            if stock_takes_it {
                let stock = Lua::new()
                    .load(&block)
                    .set_name("line 1")
                    .eval::<String>()
                    .expect("the stock Lua runs");
                assert_eq!(stock, expected, "stock {event} {call} {level}");
            }
        }
    }
    // A list that gets its metatable from the comparison function is
    // written one item at a time too, so level 3 of its `__newindex` is
    // the author's call: the sort looks at the list again before it
    // writes.
    let late = "local list = { 2, 1 }\n\
                local _, failure = pcall(function()\n\
                  table.sort(list, function(a, b)\n\
                    list[1], list[2] = nil, nil\n\
                    setmetatable(list, { __newindex = function() error('boom', 3) end })\n\
                    return a < b\n\
                  end)\n\
                end)\n\
                return failure";
    let ours = run(late, &json!({})).expect("the block runs").returned;
    let stock = Lua::new()
        .load(late)
        .set_name("line 1")
        .eval::<String>()
        .expect("the stock Lua runs");
    assert_eq!(
        (ours.as_deref(), stock.as_str()),
        (
            Some(r#"[string "line 1"]:3: boom"#),
            r#"[string "line 1"]:3: boom"#
        )
    );
}

#[test]
fn a_close_handler_reads_a_failure_under_a_replacement_as_lua_raised_it() {
    // A `__close` handler runs before the `pcall` around it returns, so it
    // reads a failure before the Engine does. A replacement has no debug
    // info, so level 2 of an `error` in a metamethod puts nothing in front
    // of the text there either. A failure that Lua raises in the
    // replacement starts with `?:?:` there, and nowhere else: under an
    // `xpcall` the handler reads what the message handler returned, which
    // got the text without that start.
    for (meta, call, raised, caught) in [
        (
            "__tostring = function() error('boom', 2) end",
            "tostring(value)",
            "boom",
            "boom",
        ),
        (
            "__len = function() return 1 end, __index = 5",
            "table.concat(value)",
            "?:?: attempt to index a number value",
            "attempt to index a number value",
        ),
    ] {
        for (protected, handled, closing) in [
            ("pcall(function()", "end)", raised),
            (
                "xpcall(function()",
                "end, function(text) return text end)",
                caught,
            ),
        ] {
            let seen = run(
                &format!(
                    "local value = setmetatable({{}}, {{ {meta} }})\n\
                     local seen\n\
                     local _, failure = {protected}\n\
                       local closing <close> = setmetatable({{}}, {{\n\
                         __close = function(_, raised) seen = raised end }})\n\
                       return {call}\n\
                     {handled}\n\
                     return seen .. '|' .. failure"
                ),
                &json!({}),
            )
            .expect("the block runs");
            assert_eq!(
                seen.returned,
                Some(format!("{closing}|{caught}")),
                "{protected} {call}"
            );
        }
    }
}

#[test]
fn a_raised_value_with_no_text_fails_the_block_without_its_address() {
    // A number in the shape this platform prints an address in: the C
    // runtime of Windows pads uppercase digits to the pointer's width and
    // puts no `0x` in front.
    let number = if cfg!(windows) {
        format!("{:0width$X}", 0x1f, width = 2 * size_of::<usize>())
    } else {
        "0x1f".to_owned()
    };
    let address_like = format!("error('size: {number}')");
    for (block, reason) in [
        ("error({})", "error object is a table value"),
        ("error(log)", "error object is a function value"),
        (
            "error(setmetatable({}, { __name = 'My Thing' }))",
            "error object is a My Thing value",
        ),
        (
            "error(setmetatable({}, { __tostring = function() return 'rendered' end }))",
            "rendered",
        ),
        // Rust runs each metamethod below, so `mlua` has rendered the
        // raised table, address and all, before the Engine sees it. The
        // address is a real one, in the shape this platform prints.
        (
            "error(setmetatable({}, { __tostring = function() error({}) end }))",
            "error object is a table value",
        ),
        (
            "pairs(setmetatable({}, { __pairs = function() error({}) end }))",
            "error object is a table value",
        ),
        (
            "tools.offer(setmetatable({}, { __len = function() error({}) end }))",
            "error object is a table value",
        ),
        // A `__name` is any string. One that is empty or holds a `: ` stays
        // as it is, and only the address goes.
        ("error(setmetatable({}, { __name = '' }))", ": (address)"),
        (
            "pairs(setmetatable({}, { __pairs = function()\n\
               error(setmetatable({}, { __name = 'a: b' }))\n\
             end }))",
            "a: b: (address)",
        ),
        // A name that starts as `mlua`'s label for a failure of Lua does is
        // a name with a `: ` like any other.
        (
            "error(setmetatable({}, { __name = 'runtime error: Thing' }))",
            "runtime error: Thing: (address)",
        ),
        // A string that ends as such a value does reads the same to the
        // Engine, and loses its number.
        (address_like.as_str(), "size: (address)"),
    ] {
        let error = run(block, &json!({})).expect_err("the block fails");
        assert_eq!(error.prose, refused(reason), "{block}");
    }
    let caught = run(
        "local ok, failure = pcall(pairs, setmetatable({}, { __pairs = function() error({}) end }))\n\
         return failure.message",
        &json!({}),
    )
    .expect("the block catches the failure")
    .returned;
    assert_eq!(
        caught,
        Some(format!(
            "prompt:1: {}",
            refused("error object is a table value")
        ))
    );
    // A `__close` handler gets the failure as `mlua` raised it, before the
    // `pcall` around it returns. It prints as the message `pcall` gives.
    let closed = run(
        "local seen\n\
         local _, failure = pcall(function()\n\
           local closing <close> = setmetatable({}, { __close = function(_, raised)\n\
             seen = { tostring(raised), string.format('%s', raised), table.concat({ raised }) }\n\
           end })\n\
           tools.offer(setmetatable({}, { __len = function() error({}) end }))\n\
         end)\n\
         return table.concat({ failure.message, seen[1], seen[2], seen[3] }, '\\n')",
        &json!({}),
    )
    .expect("the block catches the failure")
    .returned;
    let message = format!("prompt:6: {}", refused("error object is a table value"));
    assert_eq!(closed, Some([message.as_str(); 4].join("\n")));
}

#[test]
fn an_installed_require_is_runvils_and_loads_no_module() {
    let vm = TestVm::new().shims().build();
    vm.install_require(
        |path| {
            Err::<Library, _>(crate::make_lua_runtime_error(format!(
                "section `{path}` not found"
            )))
        },
        |template, _, _| Ok(template.to_owned()),
        &null_emitter(),
        "Section",
    )
    .expect("require must install");
    let step = vm
        .start_block_coro(&program(
            "local ok, err = pcall(require, 'os')\n\
             return type(require) .. '|' .. tostring(ok) .. '|' .. tostring(err) .. '|' \
               .. tostring(os) .. '|' .. tostring(package)",
        ))
        .expect("the refused require is caught");
    let CoroStep::Done(LuaBlockResult::Returned(Some(returned))) = step else {
        panic!("the block must return a string, got {step:?}");
    };
    assert_eq!(
        returned,
        format!(
            "function|false|prompt:1: section `os` not found. {}|nil|nil",
            ErrorCode::LuaRuntime.prose()
        )
    );
}

#[test]
fn setmetatable_refuses_finalizers_and_weak_globals_but_keeps_g_handlers() {
    let step = TestVm::new()
        .shims()
        .build()
        .start_block_coro(&program(
            "local _, gc = pcall(function() setmetatable({}, { __gc = function() end }) end)\n\
             local _, mode = pcall(function() setmetatable(_G, { __mode = 'v' }) end)\n\
             setmetatable(_G, { __index = function(_, key) return 'fallback ' .. key end })\n\
             return tostring(gc) .. '|' .. tostring(mode) .. '|' .. missing_name",
        ))
        .expect("the refusals are caught and the handler installs");
    let CoroStep::Done(LuaBlockResult::Returned(Some(returned))) = step else {
        panic!("the block must return a string, got {step:?}");
    };
    let parts: Vec<&str> = returned.split('|').collect();
    let [gc, mode, fallback] = parts.as_slice() else {
        panic!("expected three parts, got {returned}");
    };
    assert!(
        gc.ends_with("setmetatable: finalizers (__gc) are not available in the sandbox"),
        "a __gc metatable must be refused: {gc}"
    );
    assert!(
        mode.ends_with("setmetatable: weak tables (__mode) are not available for _G"),
        "a weak _G must be refused: {mode}"
    );
    assert_eq!(*fallback, "fallback missing_name");
}

#[test]
fn untrusted_global_escapes_and_envelopes_any_string() {
    let outcome = run("return untrusted('a < b')", &json!({})).expect("untrusted must run");
    let wrapped = outcome.returned.expect("untrusted returns a string");
    assert!(
        wrapped.starts_with("The text inside the untrusted_input_"),
        "the envelope opens with the preface, got:\n{wrapped}"
    );
    assert!(
        wrapped.contains("\na &lt; b\n"),
        "every literal '<' is escaped in the body, got:\n{wrapped}"
    );
    assert_eq!(
        wrapped.matches("<untrusted_input_").count(),
        1,
        "exactly one live open tag, got:\n{wrapped}"
    );
    assert_eq!(
        wrapped.matches("</untrusted_input_").count(),
        1,
        "exactly one live close tag, got:\n{wrapped}"
    );
}

#[test]
fn untrusted_global_wraps_every_call_under_the_run_nonce() {
    // One nonce per run: both calls from the same VM wrap under the same
    // nonce, so identical content produces a byte-identical envelope.
    let outcome = run(
        "return untrusted('same') .. '\\n@@SPLIT@@\\n' .. untrusted('same')",
        &json!({}),
    )
    .expect("untrusted must run");
    let wrapped = outcome.returned.expect("the chunk returns both envelopes");
    let (first, second) = wrapped
        .split_once("\n@@SPLIT@@\n")
        .expect("the separator splits the two envelopes");
    assert_eq!(first, second, "every call in a run shares the run nonce");
}

#[test]
fn untrusted_global_rejects_a_non_string_argument() {
    let error = run("return untrusted({})", &json!({})).expect_err("a table is not a string");
    assert!(
        error.code == ErrorCode::LuaRuntime,
        "a non-string argument must surface as a Lua error, got {error:?}"
    );
}

#[test]
fn a_stack_overflow_has_no_position_of_an_engine_chunk_wherever_lua_meets_it() {
    // Lua raises a stack overflow in the function that makes the call it
    // has no room for: the author's `__tostring`, or a function of the
    // Engine's under it. The locals move the frame the stack runs out in.
    for pad in 0..PADS {
        let locals = "local p = 0\n".repeat(pad);
        let nest = format!(
            "local t = setmetatable({{}}, {{ __tostring = function(self)\n{locals}\
             local s = string.format('%d<%s>%d', 1, self, 2) return s end }})\n"
        );
        let raised = run(&format!("{nest}return string.format('%s', t)"), &json!({}))
            .expect_err("the nest overflows");
        assert_eq!(
            raised.prose,
            author_prose("stack overflow", ErrorCode::LuaRuntime),
            "pad {pad}"
        );
        // The caught failure is the string that Lua raised. It starts
        // with nothing, or with the line of the author's that made the
        // call.
        let caught = run(
            &format!(
                "{nest}local ok, failure = pcall(string.format, '%s', t)\n\
                 return type(failure) .. '|' .. tostring(failure)"
            ),
            &json!({}),
        )
        .expect("the block runs")
        .returned
        .expect("the block returns a text");
        let position = format!("[string \"line 1\"]:{}: ", pad + 2);
        assert_eq!(
            caught.replace(&position, ""),
            "string|stack overflow",
            "pad {pad}"
        );
    }
}

#[test]
fn a_failure_that_is_not_the_stock_functions_own_passes_a_replacement_as_it_is() {
    // The `pcall` of the stock function is the call that meets Lua's
    // limit of nested C calls, so the failure it catches is Lua's and no
    // refusal of the stock function. The replacement raises it again
    // with no position put in front.
    for call in ["string.format('%d', 1)", "table.concat({ 'a', 'b' })"] {
        let caught = run(
            &format!(
                "local list list = setmetatable({{}}, {{ __len = function()\n\
                 local text = {call}\n\
                 return #list end }})\n\
                 local ok, failure = pcall(function() return #list end)\n\
                 return type(failure) .. '|' .. tostring(failure)"
            ),
            &json!({}),
        )
        .expect("the block runs")
        .returned;
        assert_eq!(caught.as_deref(), Some("string|C stack overflow"), "{call}");
    }
}

#[test]
fn a_failure_under_the_concat_of_an_error_value_has_no_position_of_an_engine_chunk() {
    // The `__concat` of an error value is a Lua function of the Engine's.
    // Level 3 of the `error` names it, and so does a stack overflow that
    // Lua raises in it.
    let leveled = run(
        "local _, refused = pcall(tools.offer, 5)\n\
         local bad = setmetatable({}, { __tostring = function() error('boom', 3) end })\n\
         local _, failure = pcall(function() return refused .. bad end)\n\
         return failure",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(leveled.returned.as_deref(), Some("boom"));
    for pad in CONCAT_PADS {
        let locals = "local p = 0\n".repeat(pad);
        let raised = run(
            &format!(
                "local _, refused = pcall(tools.offer, 5)\n\
                 local function nest(n)\n{locals}\
                 local text = refused .. 'x'\n\
                 return nest(n + 1) .. text end\n\
                 return nest(1)"
            ),
            &json!({}),
        )
        .expect_err("the nest overflows");
        assert_eq!(
            raised.prose,
            author_prose("stack overflow", ErrorCode::LuaRuntime),
            "pad {pad}"
        );
    }
}

#[test]
fn the_concat_of_an_error_value_calls_no_tostring_that_a_block_replaced() {
    // The chunk of the `__concat` takes `tostring` when the VM is made.
    // At the VM's first failure, which comes here after the block put its
    // own function there, it would take that function.
    let joined = run(
        "tostring = function() return 'replaced' end\n\
         local _, refused = pcall(tools.offer, 5)\n\
         local joined = refused .. '!'\n\
         return joined == refused.message .. '!' and 'the message' or joined",
        &json!({}),
    )
    .expect("the block runs");
    assert_eq!(joined.returned.as_deref(), Some("the message"));
}

#[test]
fn a_memory_error_under_a_replacement_is_caught_and_the_block_goes_on() {
    // The loop fills the heap, and each call asks a Rust function of the
    // replacement about a value: `tostring` about the `__tostring` and
    // the `__name` of a table, and the sort about the `__lt` of a
    // string. So the allocation that passes the limit is one that such a
    // function makes, when it makes any, or one of the block's with such
    // a function next. The filler moves which one.
    for pad in 0..MEMORY_PADS {
        for call in ["tostring(hold)", "pcall(table.sort, { label, 1 })"] {
            let vm = TestVm::new()
                .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
                .shims()
                .build();
            let block = program(&format!(
                "local filler = string.rep('x', {pad})\n\
                 local hold = table.create(80000)\n\
                 local label = 'row'\n\
                 local ok, failure = pcall(function()\n\
                   local i = 0\n\
                   while true do\n\
                     i = i + 1\n\
                     local text = {call}\n\
                     hold[i] = {{}}\n\
                   end\n\
                 end)\n\
                 hold = nil\n\
                 store.write('after.txt', 'the block went on')\n\
                 return tostring(ok) .. ' | ' .. tostring(failure) .. ' | ' .. store.read('after.txt')"
            ));
            let went_on = "false | not enough memory | the block went on".to_owned();
            assert_eq!(
                vm.drive(&block).map_err(|error| error.message),
                Ok(LuaBlockResult::Returned(Some(went_on))),
                "pad {pad}, {call}"
            );
        }
    }
}

/// How many frame sizes `a_stack_overflow_has_no_position_of_an_engine_chunk_wherever_lua_meets_it`
/// tries.
const PADS: usize = 8;

/// The frame sizes
/// `a_failure_under_the_concat_of_an_error_value_has_no_position_of_an_engine_chunk`
/// tries, among which the stack runs out in the `__concat`.
const CONCAT_PADS: std::ops::Range<usize> = 26..36;

/// How many lengths of filler
/// `a_memory_error_under_a_replacement_is_caught_and_the_block_goes_on`
/// tries.
const MEMORY_PADS: usize = 16;

/// The cases of `the_replacements_answer_as_stock_lua_does`.
const CASES: &[&str] = &[
    r#"return (string.format('%5.2f|%-5d|%+d|%05d|%x|%c|%g|%i|%q', 1.5, 3, 4, 6, 255, 65, 1.5, 7, 'a"b'))"#,
    "return (string.format('%s %s %s %10s|%-6s|%.2s', nil, true, 1.5, 'abc', 'abc', 'abc'))",
    "return (string.format('%s|%.3s|%s|%d', {}, {}, function() end, 3))",
    "return (string.format('%s %s', setmetatable({}, { __name = 'Named' }), setmetatable({}, { __name = 5 })))",
    "return (string.format('100%% %%s %s', setmetatable({}, { __tostring = function() return 'T' end })))",
    "return (string.format('%s', setmetatable({}, { __tostring = function() return {} end })))",
    "return (string.format('%s', setmetatable({}, { __tostring = function() error('boom', 0) end })))",
    // A `__tostring` that changes what a later argument prints as: the
    // argument's own metatable, then another argument's.
    "local value = setmetatable({}, { __tostring = function(self) setmetatable(self, nil) return 'first' end }) \
     return (string.format('%s|%s', value, value))",
    "local later = {} \
     local first = setmetatable({}, { __tostring = function() \
       setmetatable(later, { __tostring = function() return 'later' end }) return 'first' end }) \
     return (string.format('%s|%s', first, later))",
    // The stock function refuses an argument before it prints a later one.
    "local calls = 0 \
     local counted = setmetatable({}, { __tostring = function() calls = calls + 1 return 'x' end }) \
     pcall(string.format, '%s %d %s', counted, 'x', counted) return calls",
    "getmetatable('').__tostring = function(text) return '<' .. text .. '>' end \
     return (string.format('%s %s %s', 'x', {}, setmetatable({}, { __tostring = function() return 't' end })))",
    // A `__name` is printed up to its first zero byte, as a C string.
    "local named = setmetatable({}, { __name = 'a\\0b' }) \
     return (string.format('%s|', named) .. tostring(named))",
    // A `__tostring` of the string metatable that returns no string. The
    // case takes it out again, because the comparison prints strings.
    "local strings = getmetatable('') \
     strings.__tostring = function() return {} end \
     local _, failure = pcall(function() return tostring('x') end) \
     strings.__tostring = nil \
     error(failure, 0)",
    // The parts of a format on the two sides of a `__tostring`: the
    // conversions keep their flags, and a refused argument its number.
    "local value = setmetatable({}, { __tostring = function() return 'v' end }) \
     return (string.format('%d|%5s|%-5s|%.1s|%%|%s|%x', 7, value, value, value, value, 255))",
    "local value = setmetatable({}, { __tostring = function() return 'v' end }) \
     return (string.format('%s %s %d', value, value, 'x'))",
    "local value = setmetatable({}, { __tostring = function() return 'v' end }) \
     return (string.format('%s %d', value))",
    "return (string.format('%s %5s', 1, setmetatable({}, { __tostring = function() return 'a\\0b' end })))",
    "return (string.format('%d %s', 1, setmetatable({}, { __tostring = function() return {} end })))",
    "return (string.format('%s|%5s', setmetatable({}, { __tostring = function() return 1.0 end }), \
       setmetatable({}, { __tostring = function() return 7 end })))",
    // What the stock function refuses before it asks the argument for
    // its text, and what it refuses after: a conversion too long by one
    // byte, the longest one it reads, a flag `%s` does not take, and a
    // `%` that is no conversion.
    "local calls = 0 \
     local counted = setmetatable({}, { __tostring = function() calls = calls + 1 return 'x' end }) \
     local _, failure = pcall(string.format, '%s %000000000000000000000s', counted, counted) \
     return calls .. ' ' .. failure",
    "local calls = 0 \
     local counted = setmetatable({}, { __tostring = function() calls = calls + 1 return 'x' end }) \
     local _, failure = pcall(string.format, '%s %00000000000000000000s', counted, counted) \
     return calls .. ' ' .. failure",
    "local calls = 0 \
     local counted = setmetatable({}, { __tostring = function() calls = calls + 1 return 'x' end }) \
     local _, failure = pcall(string.format, '%s %#s %s', counted, counted, counted) \
     return calls .. ' ' .. failure",
    "local calls = 0 \
     local counted = setmetatable({}, { __tostring = function() calls = calls + 1 return 'x' end }) \
     local _, failure = pcall(string.format, '%s %5% %s', counted, counted, counted) \
     return calls .. ' ' .. failure",
    // A `__tostring` that is no function: Lua calls it through
    // `__call`, at most 15 deep, or refuses it by the type of the first
    // value it cannot call.
    "local callee = setmetatable({}, { __call = function(_, value) return type(value) end }) \
     local value = setmetatable({}, { __tostring = callee }) \
     return (tostring(value) .. string.format('%s', value))",
    "return (tostring(setmetatable({}, { __tostring = setmetatable({}, { __call = function() return {} end }) })))",
    "return (string.format('%s', setmetatable({}, { __tostring = setmetatable({}, { __name = 'Named', __call = 5 }) })))",
    "local callee = function() return 'called' end \
     for _ = 1, 15 do callee = setmetatable({}, { __call = callee }) end \
     return (tostring(setmetatable({}, { __tostring = callee })))",
    "local callee = function() return 'called' end \
     for _ = 1, 16 do callee = setmetatable({}, { __call = callee }) end \
     return (tostring(setmetatable({}, { __tostring = callee })))",
    "return (tostring(setmetatable({}, { __tostring = function() return 2^53 end })))",
    "return (string.format('%s %s', 1))",
    "return (string.format('%d', 'x'))",
    "return (string.format('%y', 1))",
    "return (string.format())",
    "return (string.format(nil))",
    "return (string.format(5, {}))",
    "return (tostring())",
    "return (tostring(nil) .. tostring(2^63) .. tostring(-0.0) .. tostring(3) .. tostring(true))",
    "return (tostring({}) .. '|' .. tostring(function() end) .. '|' .. tostring(setmetatable({}, { __name = 'Named' })))",
    "return (tostring(setmetatable({}, { __tostring = function() return {} end })))",
    "return (tostring(setmetatable({}, { __tostring = 5 })))",
    "math.randomseed(42) local t = {} for i = 1, 500 do t[i] = math.random(1, 1000000) end \
     table.sort(t) return table.concat(t, ' ')",
    "local t = { 'pear', 'fig', 'apple', 'Apple' } table.sort(t, function(a, b) return a > b end) \
     return table.concat(t, ' ')",
    "table.sort({ 1, 'x', 2 })",
    // The default order of values that are no two numbers and no two
    // strings: by `__lt`, of the left value first, or refused by the
    // names of the two types.
    "local meta = { __lt = function(a, b) return a.key < b.key end } \
     local list = {} \
     for i, key in ipairs({ 3, 1, 2 }) do list[i] = setmetatable({ key = key }, meta) end \
     table.sort(list) \
     return list[1].key .. list[2].key .. list[3].key",
    "local strings = getmetatable('') \
     strings.__lt = function(a, b) return tonumber(a) < tonumber(b) end \
     local list = { 3, '1', 2 } \
     local ok, failure = pcall(table.sort, list) \
     strings.__lt = nil \
     return tostring(failure) .. list[1] .. list[2] .. list[3]",
    "table.sort({ {}, 1 })",
    "table.sort({ setmetatable({}, { __name = 'Named' }), setmetatable({}, { __name = 'Named' }) })",
    // A `__len` that cannot be called is named by its `__name`.
    "table.sort(setmetatable({}, { __len = setmetatable({}, { __name = 'Named' }) }))",
    "table.sort({ setmetatable({}, { __lt = false }), 1 })",
    "table.sort({ 1, setmetatable({}, { __lt = setmetatable({}, { __call = function() error('called', 0) end }) }) })",
    "table.sort(setmetatable({}, { __len = false }))",
    "table.sort(setmetatable({ 2, 1 }, { __len = setmetatable({}, { __call = function() return 2 end }) }))",
    // A `__len` behind 15 `__call` metamethods is called, and a `__lt`
    // behind 16 is refused for their number before the last is asked for
    // its own.
    "local callee = function() return 0 end \
     for _ = 1, 15 do callee = setmetatable({}, { __call = callee }) end \
     table.sort(setmetatable({}, { __len = callee }))",
    "local callee = {} \
     for _ = 1, 16 do callee = setmetatable({}, { __call = callee }) end \
     local value = setmetatable({}, { __lt = callee }) \
     table.sort({ value, value })",
    "table.sort(setmetatable({}, { __len = function() return 0x7fffffff end }))",
    "table.sort()",
    "table.sort({ 3, 1, 2 }, 5)",
    "table.sort({ 1 }, 5)",
    "table.sort({ 3, 1, 2 }, function() error('boom', 0) end)",
    "local t = setmetatable({ 3, 1, 2 }, { __len = function() return 3.0 end }) table.sort(t) \
     return table.concat(t, ' ', 1, 3)",
    "table.sort(setmetatable({}, { __len = function() return 2.5 end }))",
    "table.sort(setmetatable({}, { __len = function() return 3 end, __index = 5 }))",
    // The write of an element follows a chain of `__newindex` tables, to
    // a table that takes it, to a value that cannot, or past the count
    // Lua follows. The list reads as 2, 1, so the stock sort writes it.
    "local taken = {} \
     table.sort(setmetatable({}, { __len = function() return 2 end, \
       __index = function(_, key) return 3 - key end, \
       __newindex = setmetatable({}, { __newindex = taken }) })) \
     return taken[1] .. taken[2]",
    "table.sort(setmetatable({}, { __len = function() return 2 end, \
       __index = function(_, key) return 3 - key end, __newindex = 5 }))",
    "local list = setmetatable({}, { __len = function() return 2 end, \
       __index = function(_, key) return 3 - key end }) \
     getmetatable(list).__newindex = list \
     table.sort(list)",
    "local a, b = math.randomseed(7, 9) return a .. ' ' .. b .. ' ' .. math.random(0)",
    "math.randomseed('5') return math.random(0)",
    "math.randomseed(1.5)",
    "math.randomseed(nil)",
    "math.randomseed(1, 'x')",
    "return (table.concat({ 1, 2, 3 }, ', ', 2))",
    "return (table.concat({}, {}))",
    "return (table.concat({ 1, {}, 3 }))",
    "return (table.concat({ 1, 2 }, '', 2, 1) .. table.concat({}, 'x') .. table.concat({ 1, nil, 3 }, '', 3, 3))",
    "return (table.concat({ 1, 2, 3 }, 5, '2', 3.0))",
    "return (table.concat({ 1, 2, true }))",
    "return (table.concat({ 1, 2 }, ', ', 1, 3))",
    "return (table.concat())",
    "return (table.concat(nil))",
    "return (table.concat('abc'))",
    "return (table.concat({ 1, 2 }, ', ', 'x'))",
    "return (table.concat({ 1, 2 }, ', ', 1.5))",
    "return (table.concat({ 1, 2 }, ', ', 1, {}))",
    "return (table.concat(setmetatable({}, { __len = false })))",
    "return (table.concat(setmetatable({}, { __len = function() return 'x' end })))",
    "return (table.concat(setmetatable({}, { __len = function() return 2 end, __index = 5 })))",
    "return (table.concat(setmetatable({}, { __len = function() return 2 end, \
       __index = function(_, key) return key end }), '-'))",
    // The read of an element follows a chain of `__index` tables, to
    // a value, to no value, or past the count Lua follows.
    "local last = { 'a', 'b' } \
     local list = setmetatable({}, { __len = function() return 2 end, \
       __index = setmetatable({}, { __index = setmetatable({}, { __index = last }) }) }) \
     return (table.concat(list, '-'))",
    "return (table.concat(setmetatable({}, { __len = function() return 1 end, __index = 'text' })))",
    "local list = setmetatable({}, { __len = function() return 1 end }) \
     getmetatable(list).__index = list \
     return (table.concat(list))",
    // The stock function takes the length, then refuses a separator or
    // a bound, and only then reads an element.
    "local seen = {} \
     local list = setmetatable({}, { \
       __len = function() seen[#seen + 1] = 'length' return 1 end, \
       __index = function() seen[#seen + 1] = 'read' return 'x' end }) \
     pcall(table.concat, list, {}) \
     pcall(table.concat, list, '', 'x') \
     pcall(table.concat, list, '', 1, 1) \
     return table.concat(seen, ' ')",
    // A refused list is named by the `__name` of its metatable.
    "getmetatable('').__name = 'Text' return (table.concat('abc'))",
    "getmetatable('').__name = 'Text' table.sort('abc')",
    "table.sort({ 2, 1 }, setmetatable({}, { __name = 'Comparator' }))",
    "table.sort({ 2, 1 }, setmetatable({}, { __call = function(_, a, b) return a < b end }))",
    "local strings = getmetatable('') \
     strings.__newindex = function() end \
     strings.__len = function() return 5 end \
     table.sort('ba')",
    // Lua takes the length of a string with no look at its `__len`.
    "local strings = getmetatable('') \
     strings.__len = false \
     strings.__index = function(text, at) \
       return type(at) == 'number' and string.sub(text, at, at) or string[at] \
     end \
     return (table.concat('abc', '-'))",
    // Lua finds the `__tostring` of a string by a raw read of the string
    // metatable, so an `__index` of a metatable of that metatable gives
    // none.
    "setmetatable(getmetatable(''), { __index = function(_, key) \
       if key == '__tostring' then return function() return 'indexed' end end \
     end }) \
     local shown = tostring('abc') .. '|' .. string.format('%s|%5s', 'abc', 'abc') \
     setmetatable(getmetatable(''), nil) \
     return shown",
    // Every other value between two renderings goes to the stock
    // function in one part.
    "local value = setmetatable({}, { __tostring = function() return 'v' end }) \
     local arguments = {} \
     for at = 1, 240 do arguments[at] = at % 80 == 0 and value or at end \
     return (string.format(string.rep('%s', 240, ' '), table.unpack(arguments)))",
    // The Engine's `pcall` and `xpcall` refuse their own arguments as the
    // stock ones do, and a handler that is missing as no value.
    "pcall()",
    "xpcall()",
    "xpcall(function() end)",
    "xpcall(function() end, nil)",
    "xpcall(function() end, 5)",
    "xpcall(function() end, setmetatable({}, { __name = 'Handler', __call = function() end }))",
    "xpcall(function() end, setmetatable({}, { __name = 'Hidden', __metatable = false }))",
    "return (select(2, pcall(pcall)))",
    // An argument error names the function as the call is written: by a
    // variable or a field, by its place in the library under a caller
    // that is no Lua code, and with the value left out of the count for a
    // call written `value:name(...)`.
    "local order = table.sort order(5)",
    "local library = { join = table.concat } library.join({}, {})",
    "local guarded = xpcall guarded(type, 5)",
    "return (select(2, pcall(table.sort, 5)))",
    "return (select(2, pcall(table.concat, {}, {})))",
    "return (select(2, pcall(string.format, '%d', 'x')))",
    "return (select(2, pcall(math.randomseed, 'x')))",
    "return (select(2, pcall(tostring)))",
    "return (select(2, pcall(xpcall, type, 5)))",
    "return (('%d'):format('x'))",
    "local value = setmetatable({}, { __tostring = function() return 'v' end }) \
     return (('%s %d'):format(value, 'x'))",
    "local text = setmetatable({}, { __index = string }) return (text:format())",
    "for _ in table.sort, 5 do end",
    // A light userdata has a type name of its own in an argument error.
    "xpcall(type, json.null)",
    "local set = setmetatable set({}, 5)",
    "local read = getmetatable read()",
    // A handler of `_G` that Lua cannot index fails in Lua's words, with
    // no variable of the Engine's chunk after them.
    "setmetatable(_G, { __index = 5 }) return undefined_name",
    "setmetatable(_G, { __newindex = true }) undefined_name = 1",
    // A stock function that a handler table of `_G` hands the write to is
    // named by the event, under the guard's `__newindex` as under no guard.
    "setmetatable(_G, { __newindex = setmetatable({}, { __newindex = string.format }) }) \
     undefined_name = 1",
    // A metamethod that calls the replacement again reaches Lua's limit
    // of nested C calls. Lua raises that failure, a string, and no Rust
    // function of the replacement does.
    "local list list = setmetatable({}, { __len = function() table.sort(list) return 0 end }) \
     table.sort(list)",
    "local low low = setmetatable({}, { __lt = function() table.sort({ low, low }) end }) \
     table.sort({ low, low })",
    "local list list = setmetatable({}, { __len = function() table.concat(list) return 0 end }) \
     table.concat(list)",
    // A replacement that is the metamethod another replacement runs is
    // named as under a C function, by its name in the library. One that
    // the guard of `_G` runs for a handler that is a table is named by
    // the event, as the author's read is.
    "return table.concat(setmetatable({}, { __index = string.format, __len = function() return 1 end }))",
    "local low = { __lt = string.format } \
     table.sort({ setmetatable({}, low), setmetatable({}, low) })",
    "setmetatable(_G, { __index = setmetatable({}, { __index = string.format }) }) \
     return undefined_name",
];

/// A case of the comparison with stock Lua, as one chunk: the case's
/// result or failure as a text, without the position of the case's own
/// chunk, which is named `chunk` in a Lua pattern.
fn outcome_chunk(case: &str, chunk: &str) -> String {
    format!(
        "local function body()\n{case}\nend\n\
         local ok, value = pcall(body)\n\
         local outcome = (ok and 'returned ' or 'failed ') .. type(value) .. ' ' .. tostring(value)\n\
         return (outcome:gsub('^(failed string ){chunk}:%d+: ', '%1'))"
    )
}

/// What `case` gives as a block of a section VM.
fn in_a_section(case: &str) -> Option<String> {
    run(&outcome_chunk(case, r#"%[string "line 1"%]"#), &json!({}))
        .expect("the block runs")
        .returned
}

/// What `case` gives in a stock Lua, without the address that stock Lua
/// prints after the name of a table or a function. The stock Lua has a
/// `json.null` that is a light userdata, as the Engine's is.
fn in_stock_lua(case: &str) -> String {
    let lua = Lua::new();
    let json = lua.create_table().expect("the table is made");
    json.set("null", Value::NULL).expect("the field is set");
    lua.globals().set("json", json).expect("the global is set");
    lua.load(format!(
        "local outcome = (function()\n{}\nend)()\nreturn (outcome:gsub('(%a): %w+', '%1'))",
        outcome_chunk(case, "stock")
    ))
    .set_name("=stock")
    .eval::<String>()
    .expect("the stock Lua runs")
}

/// A case in which the metamethod `event` of `value` holds a variable to
/// close and fails, and the handler of an `xpcall` around `call` reports
/// whether the variable is still open.
fn held_open(event: &str, call: &str) -> String {
    format!(
        "local closed = false \
         local meta = {{ __len = function() return 2 end }} \
         meta.{event} = function() \
           local held <close> = setmetatable({{}}, {{ __close = function() closed = true end }}) \
           error('boom', 0) \
         end \
         local value = setmetatable({{}}, meta) \
         local _, seen = xpcall(function() return {call} end, function() return closed and 'closed' or 'open' end) \
         return seen"
    )
}
