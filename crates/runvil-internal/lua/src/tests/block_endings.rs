//! How a block coroutine ends, by result: a block that runs off its end, a
//! `return` that gives text, a `return` that gives nil or no value, a
//! `return` inside a function, and the same endings for a block that
//! suspended first.

use super::*;

/// How `source` ends as a block coroutine on a fresh shim VM.
fn ending(source: &str) -> Result<LuaBlockResult> {
    let vm = TestVm::new().shims().build();
    match vm.start_block_coro(&program(source))? {
        CoroStep::Done(result) => Ok(result),
        CoroStep::Yielded(..) => panic!("{source:?} must not suspend"),
    }
}

/// How `source` ends as a block coroutine that suspends once, on a
/// `call`, and resumes with the text `answer`.
fn ending_after_a_wait(source: &str) -> LuaBlockResult {
    let vm = TestVm::new().shims().build();
    let block = program(source);
    let CoroStep::Yielded(thread, _) = vm.start_block_coro(&block).expect("the block starts")
    else {
        panic!("{source:?} must suspend on its call");
    };
    match vm
        .resume_block_coro(&block, &thread, (true, "answer"))
        .expect("the resumed block ends")
    {
        CoroStep::Done(result) => result,
        CoroStep::Yielded(..) => panic!("{source:?} must suspend once"),
    }
}

/// The result of a `return` that gave `text`.
fn text(text: &str) -> LuaBlockResult {
    LuaBlockResult::Returned(Some(text.to_owned()))
}

#[test]
fn a_block_that_executes_no_return_runs_off_its_end() {
    for source in [
        "",
        "-- only a comment",
        "local x = 1",
        "x = 1 -- a comment on the last line",
        "if false then return end",
        "if false then return 'never' end",
        "for i = 1, 3 do if i > 5 then return end end",
        "goto done\nlocal x = 1\n::done::",
    ] {
        assert_eq!(
            ending(source).expect("the block must run"),
            LuaBlockResult::RanOff,
            "{source:?}"
        );
    }
}

#[test]
fn a_return_inside_a_function_is_not_the_blocks_return() {
    for source in [
        "local function early() return 'inside' end\nearly()",
        "local function bare() return end\nbare()",
        "local function none() return nil end\nlocal x = none()",
        "pcall(function() return 'caught' end)",
    ] {
        assert_eq!(
            ending(source).expect("the block must run"),
            LuaBlockResult::RanOff,
            "{source:?}"
        );
    }
}

#[test]
fn a_return_that_gives_nil_or_no_value_is_a_return_with_no_text() {
    for source in [
        "return",
        "return;",
        "return nil",
        "return nil, 'text'",
        "do return end",
        "if true then return end",
        "local ready = false\nif not ready then return end\nreturn 'late'",
        "for _ = 1, 3 do return end",
        "local x\nreturn x",
        "local function nothing() end\nreturn nothing()",
        "local function none() return nil end\nreturn none()",
    ] {
        assert_eq!(
            ending(source).expect("the block must run"),
            LuaBlockResult::Returned(None),
            "{source:?}"
        );
    }
}

#[test]
fn a_return_of_a_scalar_is_its_text_and_any_other_value_is_refused() {
    for (source, expected) in [
        ("return 'text'", "text"),
        ("return ''", ""),
        ("return 42", "42"),
        ("return 1.5", "1.5"),
        ("return true", "true"),
        ("return false", "false"),
        ("return 'a', 'b'", "a"),
        ("if true then return 'early' end\nreturn 'late'", "early"),
        (
            "local function f() return 'from f' end\nreturn f()",
            "from f",
        ),
    ] {
        assert_eq!(
            ending(source).expect("the block must run"),
            text(expected),
            "{source:?}"
        );
    }
    for (source, kind) in [
        ("return {}", "table"),
        ("return function() end", "function"),
    ] {
        let error = ending(source).expect_err("the value has no text form");
        assert!(
            error
                .to_string()
                .contains(&format!("cannot return a {kind} as a result")),
            "{source:?}: {error}"
        );
    }
}

#[test]
fn a_block_that_suspended_first_ends_the_same_ways() {
    for (source, expected) in [
        ("local reply = call('../worker')", LuaBlockResult::RanOff),
        (
            "local reply = call('../worker')\nif reply == 'other' then return end",
            LuaBlockResult::RanOff,
        ),
        (
            "local reply = call('../worker')\nreturn",
            LuaBlockResult::Returned(None),
        ),
        (
            "local reply = call('../worker')\nreturn nil",
            LuaBlockResult::Returned(None),
        ),
        (
            "local reply = call('../worker')\nif reply then return end\nreturn 'late'",
            LuaBlockResult::Returned(None),
        ),
        (
            "local reply = call('../worker')\nreturn reply",
            text("answer"),
        ),
        ("return call('../worker')", text("answer")),
    ] {
        assert_eq!(ending_after_a_wait(source), expected, "{source:?}");
    }
}
