//! The `args` global: a table in every section, read through the guard,
//! measured and walked as the object it holds, and read-only at any depth.

use super::*;

/// Runs one chunk on an args VM, returning the block's failure.
fn run_args(vm: &SectionVm, source: &str) -> Result<Option<String>> {
    run_scalar(vm, &program(source), &null_emitter(), "Args")
}

#[test]
fn args_reads_through_the_guard() {
    let args = json!({ "query": "papers", "nested": { "hits": 2 } });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "assert(args.query == 'papers', 'a field reads through')\n\
         assert(args.nested.hits == 2, 'a nested field reads through')\n\
         assert(args.absent == nil, 'an absent field reads nil')\n\
         return args.query",
    )
    .expect("args must read");
    assert_eq!(out.as_deref(), Some("papers"));
    vm.teardown();
}

#[test]
fn args_rejects_reassignment() {
    let args = json!({ "query": "papers" });
    let vm = TestVm::new().args(&args).build();
    let error =
        run_args(&vm, "args = { query = 'hijacked' }").expect_err("reassigning args must fail");
    assert!(
        error.to_string().contains("args is read-only"),
        "the error names the rule: {error}"
    );
    vm.teardown();
}

#[test]
fn args_rejects_writes_at_any_depth() {
    let args = json!({ "query": "papers", "nested": { "hits": 2 } });
    let vm = TestVm::new().args(&args).build();
    let error =
        run_args(&vm, "args.query = 'hijacked'").expect_err("a field write on args must fail");
    assert!(
        error.to_string().contains("args is read-only"),
        "the error names the rule: {error}"
    );
    let error =
        run_args(&vm, "args.nested.hits = 3").expect_err("a nested field write on args must fail");
    assert!(
        error.to_string().contains("args is read-only"),
        "a nested table rejects writes too: {error}"
    );
    vm.teardown();
}

#[test]
fn an_args_write_raises_the_whole_refusal_for_a_string_integer_or_other_key() {
    let args = json!({ "query": "papers" });
    let vm = TestVm::new().args(&args).build();
    for (target, refusal) in [
        ("args.query", "args is read-only: cannot set field 'query'"),
        ("args[1]", "args is read-only: cannot set field 1"),
        ("args[2.5]", "args is read-only: cannot set field number"),
    ] {
        let source =
            format!("local ok, err = pcall(function() {target} = 1 end)\nreturn tostring(err)");
        let out = run_args(&vm, &source).expect("the pcall catches the refusal");
        let caught = out.expect("the chunk returns the caught error");
        assert_eq!(refusal_line(&caught), Some(refusal), "writing {target}");
    }
    let label =
        run_args(&vm, "return getmetatable(args)").expect("getmetatable reads the args label");
    assert_eq!(label.as_deref(), Some("args is read-only"));
    vm.teardown();
}

#[test]
fn an_args_assignment_raises_the_whole_refusal() {
    let args = json!({ "query": "papers" });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "local ok, err = pcall(function() args = {} end)\nreturn tostring(err)",
    )
    .expect("the pcall catches the refusal");
    let caught = out.expect("the chunk returns the caught error");
    assert_eq!(
        refusal_line(&caught),
        Some("args is read-only: assign to `var` or a section global instead")
    );
    vm.teardown();
}

#[test]
fn args_holding_an_empty_list_encodes_it_as_an_empty_array() {
    let args = json!({ "items": [], "nested": { "more": [] } });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "return json.encode(args.items) .. json.encode(args.nested.more)",
    )
    .expect("args encodes");
    assert_eq!(out.as_deref(), Some("[][]"));
    vm.teardown();
}

#[test]
fn args_reads_nested_nulls_as_json_null() {
    let args = json!({ "a": null, "ids": [1, null, 3] });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "assert(args.a == json.null, 'a null field reads json.null')\n\
         assert(args.ids[2] == json.null, 'a null list slot reads json.null')\n\
         return 'ok'",
    )
    .expect("nested nulls read as json.null");
    assert_eq!(out.as_deref(), Some("ok"));
    vm.teardown();
}

#[test]
fn args_is_a_table_even_when_empty() {
    let vm = TestVm::new().build();
    let out = run_args(
        &vm,
        "assert(type(args) == 'table', 'args is a table')\n\
         assert(next(args) == nil, 'empty args has no first key')\n\
         assert(#args == 0, 'empty args has no items')\n\
         assert(args.absent == nil, 'an absent field reads nil')\n\
         return json.encode(args)",
    )
    .expect("empty args must read");
    assert_eq!(out.as_deref(), Some("{}"));
    let error = run_args(&vm, "args = {}").expect_err("assigning empty args must fail");
    assert!(
        error.to_string().contains("args is read-only"),
        "the error names the rule: {error}"
    );
    let still_empty = run_args(&vm, "return tostring(next(args) == nil)")
        .expect("args reads after the refused assignment");
    assert_eq!(
        still_empty.as_deref(),
        Some("true"),
        "the refused assignment wrote nothing"
    );
    vm.teardown();
}

#[test]
fn the_length_of_a_list_in_args_is_its_item_count() {
    let args = json!({ "list": ["a", "b", "c"], "empty": [], "nested": { "list": [1, 2] } });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "return #args.list .. ',' .. #args.empty .. ',' .. #args.nested.list .. ',' .. #args",
    )
    .expect("the lengths must read");
    assert_eq!(out.as_deref(), Some("3,0,2,0"));
    vm.teardown();
}

#[test]
fn pairs_walks_args_and_a_list_inside_it_in_key_order() {
    let args = json!({ "query": "papers", "limit": 3, "tags": ["x", "y"] });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "local seen = {}\n\
         for key, value in pairs(args) do seen[#seen + 1] = key .. '=' .. type(value) end\n\
         for index, tag in pairs(args.tags) do seen[#seen + 1] = index .. ':' .. tag end\n\
         return table.concat(seen, ' ')",
    )
    .expect("the walk must run");
    assert_eq!(
        out.as_deref(),
        Some("limit=number query=string tags=table 1:x 2:y")
    );
    vm.teardown();
}

#[test]
fn an_injection_of_args_that_are_not_an_object_is_refused_as_internal() {
    let refused = |args: &Json| {
        SectionVm::new(&test_nonce(), &null_emitter(), "Args")
            .expect("VM must build")
            .inject_test_values(args, &json!({}), &fresh_access().access)
            .expect_err("args that are not an object are refused")
            .teardown(&null_emitter(), "Args")
    };
    for args in [json!(5), json!(["papers"]), json!(null)] {
        let error = refused(&args);
        assert_eq!(error.code, ErrorCode::Internal, "{args}");
        assert!(
            error.message.contains("args must be a JSON object"),
            "the refusal of {args} names the shape args must have: {}",
            error.message
        );
    }
    let error = refused(&json!("papers"));
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        "internal invariant violated: args must be a JSON object, got string"
    );
}

#[test]
fn the_args_guard_leaves_other_globals_alone() {
    let args = json!({ "query": "papers" });
    let vm = TestVm::new().args(&args).build();
    let out = run_args(
        &vm,
        "scratch = 42\n\
         assert(scratch == 42, 'a bare global still assigns')\n\
         assert(absent_global == nil, 'an absent global still reads nil')\n\
         return 'ok'",
    )
    .expect("ordinary globals must be untouched by the args guard");
    assert_eq!(out.as_deref(), Some("ok"));
    vm.teardown();
}
