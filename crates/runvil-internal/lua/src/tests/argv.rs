//! The `argv` global: read through the guard, and read-only at any depth.

use super::*;

/// Runs one chunk on an argv VM, returning the block's failure.
fn run_argv(vm: &SectionVm, source: &str) -> Result<Option<String>> {
    run_scalar(vm, &program(source), &null_emitter(), "Argv")
}

#[test]
fn argv_reads_through_the_guard() {
    let argv = json!({ "query": "papers", "nested": { "hits": 2 } });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(
        &vm,
        "assert(argv.query == 'papers', 'a field reads through')\n\
         assert(argv.nested.hits == 2, 'a nested field reads through')\n\
         assert(argv.absent == nil, 'an absent field reads nil')\n\
         return argv.query",
    )
    .expect("argv must read");
    assert_eq!(out.as_deref(), Some("papers"));
    vm.teardown();
}

#[test]
fn argv_rejects_reassignment() {
    let argv = json!({ "query": "papers" });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let error =
        run_argv(&vm, "argv = { query = 'hijacked' }").expect_err("reassigning argv must fail");
    assert!(
        error.to_string().contains("argv is read-only"),
        "the error names the rule: {error}"
    );
    vm.teardown();
}

#[test]
fn argv_rejects_writes_at_any_depth() {
    let argv = json!({ "query": "papers", "nested": { "hits": 2 } });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let error =
        run_argv(&vm, "argv.query = 'hijacked'").expect_err("a field write on argv must fail");
    assert!(
        error.to_string().contains("argv is read-only"),
        "the error names the rule: {error}"
    );
    let error =
        run_argv(&vm, "argv.nested.hits = 3").expect_err("a nested field write on argv must fail");
    assert!(
        error.to_string().contains("argv is read-only"),
        "a nested table rejects writes too: {error}"
    );
    vm.teardown();
}

#[test]
fn an_argv_write_raises_the_whole_refusal_for_a_string_integer_or_other_key() {
    let argv = json!({ "query": "papers" });
    let vm = TestVm::new().argv(Some(&argv)).build();
    for (target, refusal) in [
        ("argv.query", "argv is read-only: cannot set field 'query'"),
        ("argv[1]", "argv is read-only: cannot set field 1"),
        ("argv[2.5]", "argv is read-only: cannot set field number"),
    ] {
        let source =
            format!("local ok, err = pcall(function() {target} = 1 end)\nreturn tostring(err)");
        let out = run_argv(&vm, &source).expect("the pcall catches the refusal");
        let caught = out.expect("the chunk returns the caught error");
        assert_eq!(refusal_line(&caught), Some(refusal), "writing {target}");
    }
    let label =
        run_argv(&vm, "return getmetatable(argv)").expect("getmetatable reads the argv label");
    assert_eq!(label.as_deref(), Some("argv is read-only"));
    vm.teardown();
}

#[test]
fn an_argv_assignment_raises_the_whole_refusal() {
    let argv = json!({ "query": "papers" });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(
        &vm,
        "local ok, err = pcall(function() argv = {} end)\nreturn tostring(err)",
    )
    .expect("the pcall catches the refusal");
    let caught = out.expect("the chunk returns the caught error");
    assert_eq!(
        refusal_line(&caught),
        Some("argv is read-only: assign to `var` or a section global instead")
    );
    vm.teardown();
}

#[test]
fn argv_holding_an_empty_list_encodes_it_as_an_empty_array() {
    let argv = json!({ "items": [], "nested": { "more": [] } });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(
        &vm,
        "return json.encode(argv.items) .. json.encode(argv.nested.more)",
    )
    .expect("argv encodes");
    assert_eq!(out.as_deref(), Some("[][]"));
    vm.teardown();
}

#[test]
fn argv_reads_nested_nulls_as_json_null() {
    let argv = json!({ "a": null, "ids": [1, null, 3] });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(
        &vm,
        "assert(argv.a == json.null, 'a null field reads json.null')\n\
         assert(argv.ids[2] == json.null, 'a null list slot reads json.null')\n\
         return 'ok'",
    )
    .expect("nested nulls read as json.null");
    assert_eq!(out.as_deref(), Some("ok"));
    vm.teardown();
}

#[test]
fn a_nil_argv_reads_nil_and_rejects_assignment() {
    let vm = TestVm::new().argv(None).build();
    let out = run_argv(&vm, "assert(argv == nil, 'no argv reads nil') return 'ok'")
        .expect("a nil argv reads nil");
    assert_eq!(out.as_deref(), Some("ok"));
    let error = run_argv(&vm, "argv = {}").expect_err("assigning a nil argv must still fail");
    assert!(
        error.to_string().contains("argv is read-only"),
        "the error names the rule: {error}"
    );
    let still_nil = run_argv(&vm, "return tostring(argv == nil)")
        .expect("argv reads after the refused assignment");
    assert_eq!(
        still_nil.as_deref(),
        Some("true"),
        "the refused assignment wrote nothing"
    );
    vm.teardown();
}

#[test]
fn a_json_null_argv_reads_nil() {
    let argv = json!(null);
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(&vm, "return tostring(argv == nil)").expect("a null argv reads");
    assert_eq!(out.as_deref(), Some("true"));
    vm.teardown();
}

#[test]
fn a_scalar_argv_reads_and_rejects_assignment() {
    let argv = json!(5);
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out =
        run_argv(&vm, "assert(argv == 5) return tostring(argv)").expect("a scalar argv reads");
    assert_eq!(out.as_deref(), Some("5"));
    let error = run_argv(&vm, "argv = 6").expect_err("reassigning a scalar argv must fail");
    assert!(
        error.to_string().contains("argv is read-only"),
        "the error names the rule: {error}"
    );
    vm.teardown();
}

#[test]
fn the_argv_guard_leaves_other_globals_alone() {
    let argv = json!({ "query": "papers" });
    let vm = TestVm::new().argv(Some(&argv)).build();
    let out = run_argv(
        &vm,
        "scratch = 42\n\
         assert(scratch == 42, 'a bare global still assigns')\n\
         assert(absent_global == nil, 'an absent global still reads nil')\n\
         return 'ok'",
    )
    .expect("ordinary globals must be untouched by the argv guard");
    assert_eq!(out.as_deref(), Some("ok"));
    vm.teardown();
}
