//! Execution budgets: no instruction ceiling, the configured log budget,
//! and the memory ceiling.

use std::collections::BTreeSet;
use std::convert::Infallible;

use super::*;

#[test]
fn a_long_running_loop_completes_with_no_instruction_ceiling() {
    // No instruction ceiling applies: a long-running block completes
    // instead of tripping a quota error. The hook fires throughout, polling
    // the cancel flag.
    let out = run(
        "local n = 0\nfor i = 1, 8000000 do n = n + 1 end\nreturn n",
        &json!({}),
    )
    .expect("a loop of 8000000 steps must complete");
    assert_eq!(out.returned.as_deref(), Some("8000000"));
}

#[test]
fn the_first_chunk_consumes_the_configured_log_budget() {
    // `apply_lua_limits` lands before any chunk runs, so the first chunk
    // spends the configured log budget rather than the construction
    // defaults.
    let vm = TestVm::new().limits(DEFAULT_LUA_MEMORY_BYTES, 1).build();
    let error = run_scalar(
        &vm,
        &program("log('one')\nlog('two')"),
        &null_emitter(),
        "Budget",
    )
    .expect_err("the second log must exhaust the configured budget");
    let quota = crate::make_lua_quota_error(crate::QuotaResource::LogEvent);
    assert!(
        (error.code, &error.message, &error.facts) == (quota.code, &quota.message, &quota.facts),
        "log-budget exhaustion must surface as a typed LuaQuota: {error:?}"
    );
    vm.teardown();
}

#[test]
fn an_author_raise_of_a_quota_refusals_text_is_an_ordinary_runtime_error() {
    // Only the Engine's own refusal is a quota: an author who raises the
    // same words fails as any other runtime error.
    let vm = TestVm::new().build();
    for refusal in [
        "lua log event budget exceeded",
        "lua log cumulative byte budget exceeded",
    ] {
        let error = run_scalar(
            &vm,
            &program(&format!("error('{refusal}')")),
            &null_emitter(),
            "Budget",
        )
        .expect_err("the author's raise must fail the block");
        assert!(
            error.code == ErrorCode::LuaRuntime,
            "an author raise of {refusal:?} must stay a runtime error: {error:?}"
        );
    }
    vm.teardown();
}

#[test]
fn the_memory_budget_error_stays_reachable() {
    // The heap ceiling refuses a block that allocates past the memory budget
    // `apply_lua_limits` set.
    let vm = TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .build();
    let error = run_scalar(
        &vm,
        &program(
            "local t = {}\nlocal i = 1\nwhile true do t[i] = string.rep('x', 16384) i = i + 1 end",
        ),
        &null_emitter(),
        "Budget",
    )
    .expect_err("allocation past the heap ceiling must fail");
    assert!(
        lua_error_message(&error).contains("memory"),
        "the memory ceiling must surface its refusal: {error:?}"
    );
    vm.teardown();
}

#[test]
fn a_vm_that_does_not_fit_its_memory_limit_fails_to_build_with_the_memory_error() {
    // `mlua` panics when the stack of the Lua values that Rust holds must
    // grow and the memory limit refuses the growth, and Lua ends the
    // process when a push that `mlua` makes with no protected call cannot
    // allocate. A build that meets either under one of these limits does
    // not return, and the test fails there.
    for tables in TABLES_IN_ARGS {
        let injected = injected_under(DEFAULT_LUA_MEMORY_BYTES, tables)
            .expect("the Engine values inject under the default limit")
            .lua()
            .used_memory();
        let built = built_under(DEFAULT_LUA_MEMORY_BYTES, tables)
            .expect("a VM builds under the default limit")
            .lua()
            .used_memory();
        let outcomes = (injected - SHORT_OF_INJECTION..=built)
            .step_by(LIMIT_STEP)
            .map(|limit| {
                built_under(limit, tables)
                    .map(drop)
                    .map_err(|error| error.message)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            outcomes,
            BTreeSet::from([Ok(()), Err("memory error: not enough memory".to_owned())]),
            "with {tables} tables inside `args`"
        );
    }
}

/// How far below the memory in use after Engine injection the limits of
/// the build test start, in bytes: Lua collects before it refuses an
/// allocation, so the injection passes under less than it leaves in use.
const SHORT_OF_INJECTION: usize = 8192;

/// The distance between two limits of the build test, in bytes. As many
/// limits refuse one allocation as it has bytes, and the smallest that
/// the test is for is the empty string that `Lua::type_metatable` pushes,
/// 25 bytes, so a limit falls among those that refuse it.
const LIMIT_STEP: usize = 16;

/// How many tables the `args` of the build test hold, one count for each
/// pass over the limits. Each table inside `args` keeps one of the Lua
/// values that Rust holds for the life of the VM. With the first count,
/// the install of the coroutine shims grows the stack of those values
/// unless `SectionVm::new` grew it before. The second count is the most
/// that this build, which has no Pack prelude, takes with the stack at
/// the size `SectionVm::new` gives it.
const TABLES_IN_ARGS: [usize; 2] = [8, 36];

/// A section VM through Engine injection under a limit of `memory_bytes`,
/// with `tables` tables inside its `args`.
fn injected_under(memory_bytes: usize, tables: usize) -> Result<SectionVm> {
    let emitter = null_emitter();
    let fresh = SectionVm::new(&test_nonce(), &emitter, TEST_SECTION)?;
    fresh.apply_lua_limits(memory_bytes, DEFAULT_LUA_LOG_EVENTS)?;
    let args = (0..tables)
        .map(|at| (format!("k{at}"), json!({ "at": at })))
        .collect::<serde_json::Map<_, _>>();
    fresh
        .inject_test_values(&Json::Object(args), &json!({}))
        .map_err(|failure| failure.teardown(&emitter, TEST_SECTION))
}

/// A section VM with every setup step done under a limit of
/// `memory_bytes`, with `tables` tables inside its `args`.
fn built_under(memory_bytes: usize, tables: usize) -> Result<SectionVm> {
    let mut vm = injected_under(memory_bytes, tables)?;
    vm.install_engine_globals(&null_emitter(), TEST_SECTION)?;
    vm.install_control_globals(|path: String, _| {
        Ok::<SectionRead, Infallible>(SectionRead::Missing(crate::make_lua_runtime_error(
            format!("section `{path}` not found"),
        )))
    })?;
    vm.install_coro_shims(1)?;
    Ok(vm)
}
