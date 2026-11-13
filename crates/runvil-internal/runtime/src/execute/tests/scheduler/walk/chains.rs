//! Contained chains: a `return` or `jump` inside a `call` or child walk
//! stays inside its chain, a `return` that gives no value ends its chain
//! with no text, the outer walk never moves meanwhile, a jump
//! descent consumes no call depth, and a chain started past index 0 names
//! its start section before its first entry.

use super::*;
use crate::test_support::rig::Rig;

#[test]
fn a_return_inside_a_child_walk_ends_the_whole_chain() {
    // A return inside a jump-started child-level walk ends the whole
    // chain, not just the child level - the parent walk never resumes.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\njump('X')\n```\n\n\
        ### X\n\n\
        ```lua\nreturn 'x-value'\n```\n\n\
        ## B\n\n\
        ```lua\nerror('the return must end the chain before the parent resumes')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "x-value",
        "a return in the child walk ends the whole chain"
    );
}

#[test]
fn a_return_with_no_value_inside_a_child_walk_ends_the_whole_chain() {
    // A `return` that gives no value ends the chain as one that gives a
    // scalar does: neither the child level's next section nor the parent
    // walk runs, and the root chain ends with the generic completion.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\njump('X')\n```\n\n\
        ### X\n\n\
        ```lua\n\
        store.append('order.txt', 'X\\n')\n\
        return\n\
        ```\n\n\
        ### Y\n\n\
        ```lua\nerror('the return must end the chain before the child level goes on')\n```\n\n\
        ## B\n\n\
        ```lua\nerror('the return must end the chain before the parent resumes')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "done");
    assert_eq!(rig.trace().read("order.txt").expect("order log"), "X\n");
}

#[test]
fn a_return_with_no_value_inside_a_chain_ends_the_chain_and_its_call_returns_nil() {
    // The called chain ends at its `return`: the rest of `Sub` and the
    // section after it do not run, the chain has no text, so the caller's
    // `call` returns nil, and the outer walk continues.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\n\
        local r = call('../Sub')\n\
        store.append('order.txt', 'A:' .. tostring(r) .. '\\n')\n\
        ```\n\n\
        ## B\n\n\
        ```lua\n\
        store.append('order.txt', 'B\\n')\n\
        return store.read('order.txt')\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\n\
        store.append('order.txt', 'Sub\\n')\n\
        return nil\n\
        ```\n\n\
        ```lua\nerror('a return must end the chain before the next block')\n```\n\n\
        ## After\n\n\
        ```lua\nerror('a return must end the chain before fall-through')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "Sub\nA:nil\nB\n");
}

#[test]
fn a_return_with_no_value_inside_a_called_child_walk_returns_to_the_caller() {
    // The call chain descended into `### S1`. Its `return` ends the whole
    // call chain, not the child level, so `### S2` and `## After` do not
    // run and the caller goes on with nil.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\nreturn 'main:' .. tostring(call('../Sub'))\n```\n\n\
        ## Sub\n\n\
        ```lua\njump('S1')\n```\n\n\
        ### S1\n\n\
        ```lua\nif true then return end\n```\n\n\
        ### S2\n\n\
        ```lua\nerror('the return must end the chain before the child level goes on')\n```\n\n\
        ## After\n\n\
        ```lua\nerror('the return must end the chain before the parent resumes')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "main:nil");
}

#[test]
fn a_called_chain_with_no_text_gives_its_call_nil_and_an_empty_text_gives_a_string() {
    // `Quiet` runs off the chain's last section, `Bare` ends at a `return`
    // with no value, and `Empty` returns the empty string on purpose. The
    // first two chains have no text, so `call` returns nil, and the third
    // has one, so `call` returns it.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local quiet, bare, empty = call('Quiet'), call('Bare'), call('Empty')\n\
        return type(quiet) .. '|' .. type(bare) .. '|' .. type(empty) .. ':[' .. empty .. ']'\n\
        ```\n\n\
        ### Empty\n\n\
        ```lua\nreturn ''\n```\n\n\
        ### Bare\n\n\
        ```lua\nreturn\n```\n\n\
        ### Quiet\n\n\
        ```lua\nvar.ran = true\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "nil|nil|string:[]");
}

#[test]
fn a_call_with_no_text_reads_as_false_and_takes_a_fallback() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local seen = 'no text'\n\
        if call('Quiet') then seen = 'text' end\n\
        return seen .. '|' .. (call('Quiet') or 'fallback') .. '|' .. (call('Loud') or 'fallback')\n\
        ```\n\n\
        ### Loud\n\n\
        ```lua\nreturn 'loud'\n```\n\n\
        ### Quiet\n\n\
        ```lua\nvar.ran = true\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "no text|fallback|loud");
}

#[test]
fn returning_a_call_with_no_text_ends_the_chain_with_no_text() {
    // `return call(...)` ends the chain it runs in whatever the called
    // chain gave. With no text from `Quiet`, `Middle` ends with no text
    // too, so its own caller gets nil, and the main walk, which returns
    // that call in turn, ends the run with the generic completion.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        store.write('seen.txt', tostring(call('Middle')))\n\
        return call('Middle')\n\
        ```\n\n\
        ```lua\nerror('a return must end the chain before the next block')\n```\n\n\
        ### Middle\n\n\
        ```lua\nreturn call('../Quiet')\n```\n\n\
        ```lua\nerror('a return must end the chain before the next block')\n```\n\n\
        ### Quiet\n\n\
        ```lua\nvar.ran = true\n```\n\n\
        ## After\n\n\
        ```lua\nerror('a return must end the chain before fall-through')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "done");
    assert_eq!(rig.trace().read("seen.txt").expect("the note"), "nil");
}

#[test]
fn concatenating_a_call_with_no_text_fails_on_the_callers_line() {
    // The call's value is nil, so the author's concatenation is a Lua
    // error where the call is written, and it names the nil.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local prefix = 'got: '\n\
        return prefix .. call('Quiet')\n\
        ```\n\n\
        ### Quiet\n\n\
        ```lua\nvar.ran = true\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    let error = rig.trace().error();
    assert_eq!(
        error.code,
        runvil_types::error::ErrorCode::LuaRuntime,
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("attempt to concatenate a nil value"),
        "the failure names the nil: {error}"
    );
    assert_eq!(error.at, crate::test_support::at_line(11), "{error:?}");
}

#[test]
fn a_task_and_an_arm_that_return_a_call_with_no_text_have_empty_text() {
    // Inside a worker the call returns nil, and the worker's `return` of
    // it ends the worker with no text, which a task and an arm still
    // report as the empty string.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local results = tasks.join({ tasks.spawn('Worker') })\n\
        assert(results[1].ok == true, 'the task succeeded')\n\
        assert(results[1].result == '', 'a task with no text has the empty string')\n\
        local r = tasks.fanout('Worker', {'a', 'b'})\n\
        assert(r[1].ok and r[2].ok, 'both arms succeeded')\n\
        assert(r[1].text == '' and r[2].text == '', 'an arm with no text has the empty string')\n\
        return 'three workers returned'\n\
        ```\n\n\
        ### Worker\n\n\
        ```lua\n\
        assert(call('../Quiet') == nil, 'the call returns nil inside a worker')\n\
        return call('../Quiet')\n\
        ```\n\n\
        ### Quiet\n\n\
        ```lua\nvar.ran = true\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "three workers returned");
}

#[test]
fn a_task_and_an_arm_that_return_no_value_succeed_with_empty_text() {
    // A task chain and a fanout arm end at the worker's `return` too: each
    // succeeds with the empty string, and nothing after the `return` runs.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local results = tasks.join({ tasks.spawn('../Worker') })\n\
        assert(results[1].ok == true, 'the task succeeded')\n\
        assert(results[1].result == '', 'a task that returns no value has empty text')\n\
        local r = tasks.fanout('../Worker', {'a', 'b'})\n\
        assert(r[1].ok and r[2].ok, 'both arms succeeded')\n\
        assert(r[1].text == '' and r[2].text == '', 'an arm that returns no value has empty text')\n\
        return 'three workers returned'\n\
        ```\n\n\
        ## Worker\n\n\
        ```lua\n\
        local ready = false\n\
        if not ready then return end\n\
        ```\n\n\
        ```lua\nerror('a return must end the chain before the next block')\n```\n\n\
        ## After\n\n\
        ```lua\nerror('a return must end the chain before fall-through')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(rig.trace().text(), "three workers returned");
}

#[test]
fn jump_inside_call_is_contained_in_the_chain() {
    // A jump inside `call()` is contained by the chain - followed, not
    // rejected. The chain's index moves to the target, the sections between
    // the jumper and the target do not run, and the target's reply returns
    // to the caller.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local r = call('../Sub')\n\
        return 'main:' .. r\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\njump('../Peer')\n```\n\n\
        ## Skipped\n\n\
        ```lua\nerror('the chain jump must move past me')\n```\n\n\
        ## Peer\n\n\
        ```lua\nreturn 'peer-ran'\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "main:peer-ran",
        "a jump inside call must be followed within the chain"
    );
}

#[test]
fn jump_inside_a_call_chain_moves_within_the_chain() {
    // A jump inside a `call()` chain to a sibling moves within the
    // contained chain - the walk continues from the jump target under the
    // normal rules, and the chain's final reply is the call's return value.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\n\
        local r = call('../Sub')\n\
        return 'A:' .. r\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\njump('../Peer')\n```\n\n\
        ## Peer\n\n\
        ```lua\nstore.append('order.txt', 'Peer\\n')\n```\n\n\
        ## Tail\n\n\
        ```lua\n\
        store.append('order.txt', 'Tail\\n')\n\
        return 'tail-reply'\n\
        ```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "A:tail-reply",
        "a jump inside the chain must move within the chain"
    );
    assert_eq!(
        rig.trace().read("order.txt").expect("order log"),
        "Peer\nTail\n"
    );
}

#[test]
fn call_chain_jumps_to_a_child_and_returns_the_chain_result() {
    // The canonical contained chain: A calls Sub; Sub jumps to its child S1,
    // starting a child-level walk that falls through to S2; S2's return is
    // the chain's final text back to A, and the outer walk continues at B,
    // never having moved.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\n\
        store.append('order.txt', 'A1\\n')\n\
        local r = call('../Sub')\n\
        assert(r == 's2-result', 'the chain final text returns to A')\n\
        store.append('order.txt', 'A2\\n')\n\
        ```\n\n\
        ## B\n\n\
        ```lua\n\
        store.append('order.txt', 'B\\n')\n\
        return store.read('order.txt')\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\n\
        store.append('order.txt', 'Sub\\n')\n\
        jump('S1')\n\
        ```\n\n\
        ### S1\n\n\
        ```lua\nstore.append('order.txt', 'S1\\n')\n```\n\n\
        ### S2\n\n\
        ```lua\n\
        store.append('order.txt', 'S2\\n')\n\
        return 's2-result'\n\
        ```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "A1\nSub\nS1\nS2\nA2\nB\n",
        "the call chain must jump, fall through, and return its final text"
    );
}

#[test]
fn the_outer_walk_never_moves_during_a_contained_chain() {
    // The outer walk never moves while a contained chain runs - wherever
    // the chain ends, the outer walk resumes at the section after the
    // caller.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\n\
        call('../Sub')\n\
        store.append('order.txt', 'A-done\\n')\n\
        ```\n\n\
        ## B\n\n\
        ```lua\n\
        store.append('order.txt', 'B\\n')\n\
        return store.read('order.txt')\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\njump('../Peer')\n```\n\n\
        ## Peer\n\n\
        ```lua\n\
        store.append('order.txt', 'Peer\\n')\n\
        return 'p'\n\
        ```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "Peer\nA-done\nB\n",
        "the outer walk must resume at the section after the caller"
    );
}

#[test]
fn a_return_inside_a_chain_ends_the_chain_not_the_run() {
    // A return inside a contained chain ends the chain, not the run - the
    // returned value is the call's return, the chain's remaining sections
    // do not run, and the outer walk continues.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\n\
        local r = call('../Sub')\n\
        store.append('order.txt', 'A:' .. r .. '\\n')\n\
        ```\n\n\
        ## B\n\n\
        ```lua\n\
        store.append('order.txt', 'B\\n')\n\
        return store.read('order.txt')\n\
        ```\n\n\
        ## Sub\n\n\
        ```lua\nreturn 'sub-reply'\n```\n\n\
        ## After\n\n\
        ```lua\nerror('a return must end the chain before fall-through')\n```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "A:sub-reply\nB\n",
        "a return must end the chain, not the run"
    );
}

#[test]
fn call_to_a_child_starts_a_contained_chain() {
    // `call` to a child starts a contained chain at the target - the chain
    // falls through to the target's following siblings under the same rules
    // as any walk, and the chain's final reply is the call's return value.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\n\
        local r = call('Sub')\n\
        return 'got:' .. r\n\
        ```\n\n\
        ### Sub\n\n\
        ```lua\nstore.append('order.txt', 'Sub\\n')\n```\n\n\
        ### After\n\n\
        ```lua\n\
        store.append('order.txt', 'After\\n')\n\
        return 'after-reply'\n\
        ```\n";
    let rig = Rig::run(scheduler_scenario(md));

    assert_eq!(
        rig.trace().text(),
        "got:after-reply",
        "call to a child must start a contained chain"
    );
    assert_eq!(
        rig.trace().read("order.txt").expect("order log"),
        "Sub\nAfter\n"
    );
}

#[test]
fn a_jump_descent_does_not_consume_call_depth() {
    // The depth-cap interaction: a jump descent is not a call, so the child
    // level shares the chain's call-depth field. X and Y ping-pong calls
    // from inside a jump-started child walk; each entry appends once. The
    // cap trips when the ninth nested call would run (depth 9 > 8), after
    // exactly nine section entries - a descent that wrongly consumed depth
    // would trip the cap one entry earlier.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## Main\n\n\
        ```lua\njump('X')\n```\n\n\
        ### X\n\n\
        ```lua\n\
        store.append('depth.txt', 'x\\n')\n\
        return call('../Y')\n\
        ```\n\n\
        ### Y\n\n\
        ```lua\n\
        store.append('depth.txt', 'y\\n')\n\
        return call('../X')\n\
        ```\n";
    let rig = Rig::run(scheduler_scenario(md));

    let error = rig.trace().error();
    assert!(
        error
            == &crate::test_support::with_author_prose(
                crate::error::make_call_depth_exceeded_error(8),
                17
            ),
        "the depth cap must fail the run on X's call line, got {error:?}"
    );
    assert_eq!(
        rig.trace().read("depth.txt").expect("depth log"),
        "x\ny\nx\ny\nx\ny\nx\ny\nx\n",
        "the descent shares the chain's call depth: nine entries, then the cap"
    );
}

#[test]
fn a_chain_started_past_index_0_names_its_start_section_before_its_first_entry() {
    // A `call` and a spawn start their chain at the target's index, so the
    // chain's reports before its first entry name the target, not the
    // slice's first section. An index past the slice names the prompt by
    // its frontmatter `name`.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## A\n\n\
        ```lua\nreturn 'a'\n```\n\n\
        ## B\n\n\
        ```lua\nreturn 'b'\n```\n";
    let mut rig = Rig::start(scheduler_scenario(md));
    let scheduler = rig.scheduler_for_test();

    assert_eq!(
        scheduler
            .name_before_first_entry_for_test(1)
            .expect("the chain starts"),
        "B",
        "a chain started at index 1 names the section at index 1"
    );
    assert_eq!(
        scheduler
            .name_before_first_entry_for_test(2)
            .expect("the chain starts"),
        "t",
        "a chain started past the slice names the prompt's frontmatter `name`"
    );
}
