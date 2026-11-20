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
