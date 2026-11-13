//! The control globals from the preamble: `call`, `jump`, and `fanout`
//! reach a top-level section by a sibling path, and a path that names no
//! section fails the run with the resolution error.

use super::*;

/// A prompt whose preamble is `lua`, followed by the top-level sections
/// `Skipped`, which fails when it runs, and `Real`.
fn preamble_source(lua: &str) -> String {
    PromptSource::new()
        .preamble(lua)
        .section(
            "Skipped",
            "error('the preamble leaves before the walk falls through')",
        )
        .section("Real", "return 'real-ran'")
        .build()
}

/// Runs the prompt whose preamble is `lua` and returns the run failure's
/// message.
#[track_caller]
fn preamble_failure(lua: &str) -> String {
    let trace = Scenario::new(preamble_source(lua)).run();
    trace.error().to_string()
}

/// What the preamble searched for a child path: it has no children, so the
/// name of a top-level section is no child's name there.
const NO_SUCH_CHILD: &str = "section `Real` not found: the preamble has no child `Real`";

/// What the preamble searched for a sibling path no top-level section has.
const NO_SUCH_SIBLING: &str = "section `../Nope` not found: the preamble has no sibling `Nope`";

/// The top-level sections are the preamble's siblings: `call`, `jump`, and
/// `fanout` each reach `Real` by `../Real`, past the section between them.
#[test]
fn the_control_globals_reach_a_top_level_section_from_the_preamble_by_a_sibling_path() {
    let cases = [
        ("return 'called:' .. call('../Real')", "called:real-ran"),
        ("jump('../Real')", "real-ran"),
        (
            "return 'arm:' .. tasks.fanout('../Real', {'a'})[1].text",
            "arm:real-ran",
        ),
    ];
    for (lua, expected) in cases {
        let trace = Scenario::new(preamble_source(lua)).run();
        assert_eq!(trace.text(), expected, "{lua}");
    }
}

/// The control globals work in the preamble, which has no children: a
/// `call` that names a top-level section as a child fails the run with the
/// resolution error.
#[test]
fn call_from_the_preamble_by_a_child_path_fails_the_run() {
    let rendered = preamble_failure("call('Real')");
    assert!(
        rendered.contains(NO_SUCH_CHILD),
        "the resolution error says where it looked: {rendered}"
    );
}

/// `jump` out of the preamble takes a sibling path; a child path fails the
/// run with the resolution error.
#[test]
fn jump_from_the_preamble_by_a_child_path_fails_the_run() {
    let rendered = preamble_failure("jump('Real')");
    assert!(
        rendered.contains(NO_SUCH_CHILD),
        "the resolution error says where it looked: {rendered}"
    );
}

/// `fanout` from the preamble resolves its worker the way `call` resolves
/// its target; a child path fails the run with the resolution error.
#[test]
fn fanout_from_the_preamble_by_a_child_path_fails_the_run() {
    let rendered = preamble_failure("tasks.fanout('Real', {'a'})");
    assert!(
        rendered.contains(NO_SUCH_CHILD),
        "the resolution error says where it looked: {rendered}"
    );
}

/// A sibling path from the preamble that names no top-level section fails
/// the run with the resolution error, from each control global.
#[test]
fn a_sibling_path_from_the_preamble_to_an_unknown_section_fails_the_run() {
    let controls = [
        "call('../Nope')",
        "jump('../Nope')",
        "tasks.fanout('../Nope', {'a'})",
    ];
    for lua in controls {
        let rendered = preamble_failure(lua);
        assert!(
            rendered.contains(NO_SUCH_SIBLING),
            "{lua}: the resolution error says where it looked: {rendered}"
        );
    }
}
