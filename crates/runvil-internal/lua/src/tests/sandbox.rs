//! The sandboxed global environment: no direct output, the safe standard
//! library without the dangerous globals, `setmetatable`'s refusals, and
//! the `untrusted` global.

use super::*;

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
fn table_sort_fails_on_the_callers_line_in_stock_luas_words() {
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
        // A `__len` that cannot be called fails where the sort takes the
        // list's length.
        (
            "table.sort(setmetatable({}, { __len = false }))",
            "attempt to call a boolean value (metamethod 'len')",
        ),
        // A failure Lua raises in the sort's own read of the list, which a
        // comparison function does not put under a protected call.
        (
            "table.sort(setmetatable({}, { __len = function() return 3 end, __index = 5 }), \
             function(a, b) return a < b end)",
            "attempt to index a number value",
        ),
    ] {
        let error =
            run(&format!("local first_line\n{call}"), &json!({})).expect_err("the sort fails");
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
        "table.sort()",
        "table.sort({ 3, 1, 2 }, 5)",
        "table.sort({ 1 }, 5)",
        "table.sort({ 3, 1, 2 }, function() error('boom', 0) end)",
        "local t = setmetatable({ 3, 1, 2 }, { __len = function() return 3.0 end }) table.sort(t) \
         return table.concat(t, ' ', 1, 3)",
        "table.sort(setmetatable({}, { __len = function() return 2.5 end }))",
        "table.sort(setmetatable({}, { __len = function() return 3 end, __index = 5 }))",
        "local a, b = math.randomseed(7, 9) return a .. ' ' .. b .. ' ' .. math.random(0)",
        "math.randomseed('5') return math.random(0)",
        "math.randomseed(1.5)",
        "math.randomseed(nil)",
        "math.randomseed(1, 'x')",
        "return (table.concat({ 1, 2, 3 }, ', ', 2))",
        "return (table.concat({}, {}))",
        "return (table.concat({ 1, {}, 3 }))",
    ];
    let outcome = |case: &str, chunk: &str| {
        format!(
            "local function body()\n{case}\nend\n\
             local ok, value = pcall(body)\n\
             local outcome = (ok and 'returned ' or 'failed ') .. type(value) .. ' ' .. tostring(value)\n\
             return (outcome:gsub('^(failed string ){chunk}:%d+: ', '%1'))"
        )
    };
    for case in CASES {
        let ours = run(&outcome(case, r#"%[string "line 1"%]"#), &json!({}))
            .expect("the block runs")
            .returned;
        let stock = Lua::new()
            .load(format!(
                "local outcome = (function()\n{}\nend)()\nreturn (outcome:gsub('(%a): %w+', '%1'))",
                outcome(case, "stock")
            ))
            .set_name("=stock")
            .eval::<String>()
            .expect("the stock Lua runs");
        assert_eq!(ours.as_deref(), Some(stock.as_str()), "{case}");
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
