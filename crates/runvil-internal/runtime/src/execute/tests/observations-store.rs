//! Tests for the events a run reports for its store operations: each
//! operation's succeeded event and its failed event, where an event lands
//! among its block's other reports, the block that reports it, and the
//! fields it holds.

use std::fmt::Write as _;

use super::*;

/// The prompt whose one section, `Gather`, writes a file in its first
/// block and reads it back in its second.
const TWO_STORE_BLOCKS: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
    ## Gather\n\n\
    ```lua\nstore.write('state.txt', 'kept')\n```\n\n\
    ```lua\nreturn store.read('state.txt')\n```\n";

/// `records` in the shape [`sequence`] gives a run's: each section beside
/// its event's detail.
fn records<const N: usize>(records: [(&str, Observation); N]) -> Vec<(String, String)> {
    records
        .into_iter()
        .map(|(section, event)| (section.to_owned(), event.to_string()))
        .collect()
}

/// The author's `log(message)` checkpoint.
fn logged(message: &str) -> Observation {
    Observation::Lua(message.to_owned())
}

/// The records of the run's store operations, in order.
fn store_records(trace: &Trace) -> Vec<(String, String)> {
    sequence(trace)
        .into_iter()
        .filter(|(_, detail)| detail.starts_with("Vfs "))
        .collect()
}

/// The events of the run's store operations, each serialized as the caller
/// receives it, in order.
fn serialized_store_events(trace: &Trace) -> Vec<Value> {
    trace
        .events()
        .iter()
        .map(|event| serde_json::to_value(event).expect("an event serializes"))
        .filter(|event| {
            event["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("vfs_"))
        })
        .collect()
}

#[test]
fn every_store_operation_reports_its_succeeded_event_and_its_failed_event() {
    /// One store operation: a call the store performs, a call it refuses,
    /// and the event each one reports.
    struct Operation {
        performed: &'static str,
        refused: &'static str,
        succeeded: Observation,
        failed: Observation,
    }

    // In an order one block runs top to bottom: the write makes the file
    // the reads and the replace need, and the delete comes last. The store
    // refuses a path with a traversal segment, a file that is not there,
    // and a pattern whose wildcard grammar is invalid.
    let operations = [
        Operation {
            performed: "store.write('a.txt', 'old')",
            refused: "store.write('../x', 'new')",
            succeeded: detail::VFS_WRITE_SUCCEEDED,
            failed: detail::VFS_WRITE_FAILED,
        },
        Operation {
            performed: "store.append('a.txt', ' text')",
            refused: "store.append('../x', 'new')",
            succeeded: detail::VFS_APPEND_SUCCEEDED,
            failed: detail::VFS_APPEND_FAILED,
        },
        Operation {
            performed: "store.read('a.txt')",
            refused: "store.read('missing.txt')",
            succeeded: detail::VFS_READ_SUCCEEDED,
            failed: detail::VFS_READ_FAILED,
        },
        Operation {
            performed: "store.read_numbered('a.txt')",
            refused: "store.read_numbered('missing.txt')",
            succeeded: detail::VFS_READ_NUMBERED_SUCCEEDED,
            failed: detail::VFS_READ_NUMBERED_FAILED,
        },
        Operation {
            performed: "store.str_replace('a.txt', 'old', 'new')",
            refused: "store.str_replace('missing.txt', 'old', 'new')",
            succeeded: detail::VFS_REPLACE_SUCCEEDED,
            failed: detail::VFS_REPLACE_FAILED,
        },
        Operation {
            performed: "store.glob('*.txt')",
            refused: "store.glob('a/***/b')",
            succeeded: detail::VFS_GLOB_SUCCEEDED,
            failed: detail::VFS_GLOB_FAILED,
        },
        Operation {
            performed: "store.exists('a.txt')",
            refused: "store.exists('../x')",
            succeeded: detail::VFS_EXISTS_SUCCEEDED,
            failed: detail::VFS_EXISTS_FAILED,
        },
        Operation {
            performed: "store.delete('a.txt')",
            refused: "store.delete('../x')",
            succeeded: detail::VFS_DELETE_SUCCEEDED,
            failed: detail::VFS_DELETE_FAILED,
        },
    ];

    // The block catches each refusal, so it runs on to the next operation.
    let mut block = String::new();
    for Operation {
        performed, refused, ..
    } in &operations
    {
        // Writing to a String is infallible, so the result is discarded.
        let _ = writeln!(
            block,
            "{performed}\n\
             local ok, err = pcall(function() {refused} end)\n\
             assert(not ok and err.kind == 'store', [[the store must refuse {refused}]])"
        );
    }
    block.push_str("return 'done'");
    let trace = Scenario::new(one_section(&block)).run();
    assert_eq!(trace.text(), "done");

    let expected: Vec<(String, String)> = operations
        .iter()
        .flat_map(|operation| [&operation.succeeded, &operation.failed])
        .map(|event| ("Only".to_owned(), event.to_string()))
        .collect();
    assert_eq!(
        store_records(&trace),
        expected,
        "each operation reports its succeeded event, then its failed event for the refused call"
    );
}

#[test]
fn an_existence_check_that_finds_no_file_reports_its_succeeded_event() {
    // The check answers `false`, which is its outcome and no refusal.
    let trace = Scenario::new(one_section("return tostring(store.exists('missing.txt'))")).run();
    assert_eq!(trace.text(), "false");
    assert_eq!(
        store_records(&trace),
        records([("Only", detail::VFS_EXISTS_SUCCEEDED)])
    );
}

#[test]
fn a_store_operations_event_precedes_the_side_effects_that_follow_it_in_its_block() {
    // An operation's event is reported when its answer is applied, before
    // the block resumes: the `log` between the two writes lands between
    // their events. Events held back to the block's end would both follow
    // it.
    let source = one_section(
        "
        log('before the first write')
        store.write('first.txt', 'one')
        log('between the writes')
        store.write('second.txt', 'two')
        log('after the second write')
        return 'done'",
    );
    let trace = Scenario::new(source).run();
    assert_eq!(trace.text(), "done");
    assert_eq!(
        sequence(&trace),
        records([
            ("t", detail::RUN_STARTED),
            ("Only", detail::SECTION_STARTED),
            ("Only", detail::LUA_CHUNK_STARTED),
            ("Only", logged("before the first write")),
            ("Only", detail::VFS_WRITE_SUCCEEDED),
            ("Only", logged("between the writes")),
            ("Only", detail::VFS_WRITE_SUCCEEDED),
            ("Only", logged("after the second write")),
            ("Only", detail::LUA_CHUNK_SUCCEEDED),
            ("Only", detail::LUA_TEARDOWN_STARTED),
            ("Only", detail::LUA_TEARDOWN_SUCCEEDED),
            ("Only", detail::SECTION_FINISHED),
            ("t", detail::RUN_SUCCEEDED),
        ])
    );
    for (path, contents) in [("first.txt", "one"), ("second.txt", "two")] {
        assert_eq!(
            trace.read(path).as_deref(),
            Ok(contents),
            "the write of {path} landed"
        );
    }
}

#[test]
fn each_block_of_a_section_reports_its_own_store_operations() {
    // A block's store events sit between its own two boundaries: the
    // first block's write inside the first pair, the second block's read
    // inside the second.
    let trace = Scenario::new(TWO_STORE_BLOCKS).run();
    assert_eq!(trace.text(), "kept");
    assert_eq!(
        sequence(&trace),
        records([
            ("t", detail::RUN_STARTED),
            ("Gather", detail::SECTION_STARTED),
            ("Gather", detail::LUA_CHUNK_STARTED),
            ("Gather", detail::VFS_WRITE_SUCCEEDED),
            ("Gather", detail::LUA_CHUNK_SUCCEEDED),
            ("Gather", detail::LUA_CHUNK_STARTED),
            ("Gather", detail::VFS_READ_SUCCEEDED),
            ("Gather", detail::LUA_CHUNK_SUCCEEDED),
            ("Gather", detail::LUA_TEARDOWN_STARTED),
            ("Gather", detail::LUA_TEARDOWN_SUCCEEDED),
            ("Gather", detail::SECTION_FINISHED),
            ("t", detail::RUN_SUCCEEDED),
        ])
    );
}

#[test]
fn a_store_event_holds_only_its_identity_fields() {
    // The paths, the contents, the pattern, and the anchor are what each
    // operation's effect holds. None of them reaches the operation's
    // event, whether the store performs the operation or refuses it, as
    // it refuses the replace that ends this run.
    let source = one_section(
        "
        store.write('secret/path.txt', 'private contents')
        store.read('secret/path.txt')
        store.glob('secret/*.txt')
        store.str_replace('secret/path.txt', 'missing secret', 'replacement')",
    );
    let trace = Scenario::new(source).run();
    assert_eq!(
        trace.error().code,
        ErrorCode::Anchor,
        "the refused replace ends the run: {:?}",
        trace.error()
    );

    let events = serialized_store_events(&trace);
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "vfs_write_succeeded",
            "vfs_read_succeeded",
            "vfs_glob_succeeded",
            "vfs_replace_failed",
        ]
    );
    for event in &events {
        let Some(object) = event.as_object() else {
            panic!("required an event serialized as a JSON object, actual {event}");
        };
        let mut fields: Vec<&str> = object.keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            ["kind", "provenance", "section"],
            "required an event holding only identity fields, actual {event}"
        );
    }
}
