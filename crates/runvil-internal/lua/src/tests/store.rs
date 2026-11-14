//! The `store` table as a block calls it: each operation's result, the
//! line bounds of `read` and `read_numbered`, the store failure an
//! operation the block does not catch ends it with, and writes seen
//! through the access the block driver answers through.

use super::*;

#[test]
fn store_exists_returns_boolean() {
    let access = fresh_access();
    let store = store_view(&access);
    assert_eq!(
        run_with("return tostring(store.exists('missing.txt'))", &access)
            .unwrap()
            .returned
            .as_deref(),
        Some("false")
    );
    store
        .write("a.txt", b"hi")
        .expect("the memory store can seed a file");
    assert_eq!(
        run_with("return tostring(store.exists('a.txt'))", &access)
            .unwrap()
            .returned
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        run_with(
            "store.delete('a.txt')\nreturn tostring(store.exists('a.txt'))",
            &access,
        )
        .unwrap()
        .returned
        .as_deref(),
        Some("false")
    );
}

#[test]
fn store_write_then_read_numbered_returns_numbered_content() {
    let out = run(
        "store.write('a.txt', 'first\\nsecond')\nreturn store.read_numbered('a.txt')",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("1| first\n2| second"));
}

#[test]
fn store_append_extends_the_file() {
    let out = run(
            "store.append('log.txt', 'one\\n')\nstore.append('log.txt', 'two')\nreturn store.read_numbered('log.txt')",
            &json!({}),
        )
        .unwrap();
    assert_eq!(out.returned.as_deref(), Some("1| one\n2| two"));
}

#[test]
fn store_str_replace_edits_in_place() {
    let out = run(
            "store.write('a.txt', 'the quick brown fox')\nstore.str_replace('a.txt', 'quick', 'slow')\nreturn store.read_numbered('a.txt')",
            &json!({}),
        )
        .unwrap();
    assert_eq!(out.returned.as_deref(), Some("1| the slow brown fox"));
}

#[test]
fn store_delete_then_read_raises() {
    let err = run(
        "store.write('a.txt', 'gone soon')\nstore.delete('a.txt')\nreturn store.read('a.txt')",
        &json!({}),
    )
    .expect_err("reading a deleted file must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::NotFound),
        "file not found in store: a.txt"
    );
}

#[test]
fn store_read_with_start_only_reads_to_eof() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read('a.txt', 2)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("two\nthree"));
}

#[test]
fn store_read_with_start_and_end_slices_inclusively() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read('a.txt', 2, 2)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("two"));
}

#[test]
fn store_read_clamps_end_to_the_last_line() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read('a.txt', 2, 99)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("two\nthree"));
}

#[test]
fn store_read_beyond_eof_returns_empty() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read('a.txt', 99)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some(""));
}

#[test]
fn store_read_start_below_one_raises() {
    for source in [
        "store.write('a.txt', 'one')\nreturn store.read('a.txt', 0)",
        "store.write('a.txt', 'one')\nreturn store.read('a.txt', -1)",
    ] {
        let err = run(source, &json!({})).expect_err("a start below 1 must raise");
        assert_eq!(
            store_failure_message(&err, ErrorCode::InvalidRange),
            "invalid line range for a.txt: start must be at least 1",
            "{source}"
        );
    }
}

#[test]
fn store_read_end_before_start_raises() {
    let err = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read('a.txt', 3, 2)",
        &json!({}),
    )
    .expect_err("an end before start must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: end must not be before start"
    );
}

#[test]
fn store_read_end_without_start_raises() {
    let err = run(
        "store.write('a.txt', 'one')\nreturn store.read('a.txt', nil, 1)",
        &json!({}),
    )
    .expect_err("an end without a start must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: start is required when end is given"
    );
}

#[test]
fn store_read_numbered_without_bounds_numbers_from_one() {
    let access = fresh_access();
    let out = run_with(
        "store.write('a.txt', 'first\\nsecond')\nreturn store.read_numbered('a.txt')",
        &access,
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("1| first\n2| second"));
}

#[test]
fn store_read_numbered_numbers_a_slice_absolutely() {
    let access = fresh_access();
    let store = store_view(&access);
    let mut body = String::new();
    for n in 1..=85 {
        use std::fmt::Write as _;
        let _ = writeln!(body, "line{n}");
    }
    store
        .write("a.txt", body.as_bytes())
        .expect("the memory store can seed a file");
    let out = run_with("return store.read_numbered('a.txt', 84, 85)", &access).unwrap();
    assert_eq!(out.returned.as_deref(), Some("84| line84\n85| line85"));
}

#[test]
fn store_read_numbered_pads_across_the_hundred_boundary() {
    let access = fresh_access();
    let store = store_view(&access);
    let mut body = String::new();
    for n in 1..=100 {
        use std::fmt::Write as _;
        let _ = writeln!(body, "line{n}");
    }
    store
        .write("a.txt", body.as_bytes())
        .expect("the memory store can seed a file");
    let out = run_with("return store.read_numbered('a.txt', 99, 100)", &access).unwrap();
    assert_eq!(out.returned.as_deref(), Some(" 99| line99\n100| line100"));
}

#[test]
fn store_read_numbered_clamps_end_to_the_last_line() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read_numbered('a.txt', 2, 99)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some("2| two\n3| three"));
}

#[test]
fn store_read_numbered_beyond_eof_returns_empty() {
    let out = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read_numbered('a.txt', 99)",
        &json!({}),
    )
    .unwrap();
    assert_eq!(out.returned.as_deref(), Some(""));
}

#[test]
fn store_read_numbered_start_below_one_raises() {
    for source in [
        "store.write('a.txt', 'one')\nreturn store.read_numbered('a.txt', 0)",
        "store.write('a.txt', 'one')\nreturn store.read_numbered('a.txt', -1)",
    ] {
        let err = run(source, &json!({})).expect_err("a start below 1 must raise");
        assert_eq!(
            store_failure_message(&err, ErrorCode::InvalidRange),
            "invalid line range for a.txt: start must be at least 1",
            "{source}"
        );
    }
}

#[test]
fn store_read_numbered_end_before_start_raises() {
    let err = run(
        "store.write('a.txt', 'one\\ntwo\\nthree')\nreturn store.read_numbered('a.txt', 3, 2)",
        &json!({}),
    )
    .expect_err("an end before start must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: end must not be before start"
    );
}

#[test]
fn store_read_numbered_end_without_start_raises() {
    let err = run(
        "store.write('a.txt', 'one')\nreturn store.read_numbered('a.txt', nil, 1)",
        &json!({}),
    )
    .expect_err("an end without a start must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: start is required when end is given"
    );
}

#[test]
fn installed_store_read_honors_line_bounds() {
    let vm = TestVm::new().shims().build();
    store_view(&vm.access)
        .write("a.txt", b"one\ntwo\nthree\n")
        .expect("the memory store can prepare a file");

    let sliced =
        drive_scalar(&vm, "return store.read('a.txt', 2, 2)").expect("a bounded read must run");
    assert_eq!(sliced.as_deref(), Some("two"));

    let err =
        drive_scalar(&vm, "return store.read('a.txt', 0)").expect_err("a start below 1 must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: start must be at least 1"
    );
}

#[test]
fn installed_store_read_numbered_honors_line_bounds() {
    let vm = TestVm::new().shims().build();
    store_view(&vm.access)
        .write("a.txt", b"one\ntwo\nthree\n")
        .expect("the memory store can prepare a file");

    let numbered = drive_scalar(&vm, "return store.read_numbered('a.txt', 2, 3)")
        .expect("a bounded numbered read must run");
    assert_eq!(numbered.as_deref(), Some("2| two\n3| three"));

    let whole = drive_scalar(&vm, "return store.read_numbered('a.txt')")
        .expect("an unbounded numbered read must run");
    assert_eq!(whole.as_deref(), Some("1| one\n2| two\n3| three"));

    let err = drive_scalar(&vm, "return store.read_numbered('a.txt', 0)")
        .expect_err("a start below 1 must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::InvalidRange),
        "invalid line range for a.txt: start must be at least 1"
    );
}

#[test]
fn store_glob_returns_a_sorted_array() {
    let out = run(
            "store.write('src/b.rs', '')\nstore.write('src/a.rs', '')\nlocal g = store.glob('src/*.rs')\nreturn g[1] .. ',' .. g[2]",
            &json!({}),
        )
        .unwrap();
    assert_eq!(out.returned.as_deref(), Some("src/a.rs,src/b.rs"));
}

#[test]
fn an_uncaught_store_failure_ends_the_block_as_that_store_failure() {
    // An ambiguous `str_replace` anchor is a `VfsError::Anchor`: the block
    // that does not catch it ends with the store failure itself, under its
    // own code and the model-facing message, on the line of the call.
    let err = run(
        "store.write('a.txt', 'na na na')\nstore.str_replace('a.txt', 'na', 'la')",
        &json!({}),
    )
    .expect_err("an ambiguous anchor must raise");
    assert_eq!(
        store_failure_message(&err, ErrorCode::Anchor),
        "anchor \"na\" occurs 3 times in a.txt, expected exactly one; \
         include more surrounding text so it matches once"
    );
    let call_line = NonZeroU32::new(2).expect("2 is non-zero");
    assert_eq!(
        err.at,
        Some(runvil_types::error::SourceLoc::line("prompt", call_line)),
        "the failure is placed on the line of the store call"
    );
}

#[test]
fn store_writes_are_visible_on_the_shared_handle() {
    // The block driver performs the write through the test's access, so a
    // write from Lua reads back through that access's store view once the
    // block ends.
    let access = fresh_access();
    let store = store_view(&access);
    run_with("store.write('shared.txt', 'from lua')", &access).unwrap();
    assert_eq!(
        store
            .read_string("shared.txt")
            .expect("the file the chunk wrote reads back"),
        "from lua",
        "a Lua write must land in the shared store"
    );
}
