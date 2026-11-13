//! Control flow out of the preamble: the top-level sections are its
//! siblings, so `call`, `jump`, and `tasks.fanout` reach them by a `../`
//! path, a child path misses because the preamble has no children, and a
//! sibling path that names no section fails on its line; the child ids
//! `call` takes across the preamble and the next section; the
//! decision-tool idiom; and a level-1 heading above level-2 headings,
//! whose section runs its children only when it names them.

use super::*;

#[test]
fn call_from_the_preamble_runs_a_top_level_section_as_a_contained_chain() {
    // The top-level sections are the preamble's siblings, so `call`
    // reaches one by a `../` path, runs it as a contained chain, and
    // returns its text. The walk then goes on at the section after the
    // preamble.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.answer = call('../Answer')\n```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.answer\n```\n\n\
        ## Answer\n\n\
        ```lua\nreturn 'called from the preamble'\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "called from the preamble");
}

#[test]
fn a_call_from_the_preamble_and_a_call_from_the_next_section_take_consecutive_child_ids() {
    // The preamble and the section after it are entries of one root chain,
    // and the child counter is the chain's: the `call` the preamble makes
    // is child `0.0`, and the next section's first `call` is child `0.1`,
    // not a second `0.0`. The chain's entry counter runs on the same way:
    // the next section is entry `0.1`.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.first = call('../Answer')\n```\n\n\
        ## Result\n\n\
        ```lua\n\
        assert(sys.id == '0.1', 'the section after the preamble takes root entry 1')\n\
        return var.first .. ',' .. call('../Answer')\n\
        ```\n\n\
        ## Answer\n\n\
        ```lua\nreturn sys.id\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(
        trace.text(),
        "0.0.0,0.1.0",
        "the preamble's call is root child 0 and the next section's call is root child 1"
    );
}

#[test]
fn a_call_from_the_preamble_by_a_child_path_misses_and_pcall_catches_it() {
    // The preamble has no children, so a child path names nothing there,
    // even when a top-level section has that name. The miss is the call's
    // answer: the shim raises it at the call site, where an author `pcall`
    // catches it.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local ok, err = pcall(call, 'Answer')\n\
        return tostring(ok) .. ':' .. tostring(err)\n\
        ```\n\n\
        ## Answer\n\n\
        ```lua\nreturn 'a child path does not reach this section'\n```\n";
    let trace = preamble_scenario(md).run();

    let out = trace.text();
    assert!(
        out.starts_with("false:")
            && out.contains("section `Answer` not found: the preamble has no child `Answer`"),
        "the caught error says where it looked: {out}"
    );
}

#[test]
fn a_jump_from_the_preamble_moves_the_walk_to_a_top_level_section() {
    // A jump from the preamble is an ordinary sibling jump: the same chain
    // moves to the top-level target, which takes its next entry id, and
    // the sections between do not run.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\njump('../Target')\n```\n\n\
        ## Skipped\n\n\
        ```lua\nerror('the jump must skip this section')\n```\n\n\
        ## Target\n\n\
        ```lua\nreturn 'jumped:' .. sys.id\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "jumped:0.1");
}

#[test]
fn a_jump_from_the_preamble_to_a_missing_sibling_fails_the_run() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\njump('../Missing')\n```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert!(
        error
            .to_string()
            .contains("section `../Missing` not found: the preamble has no sibling `Missing`"),
        "the failure says where it looked: {error}"
    );
}

#[test]
fn a_jump_from_the_preamble_to_a_missing_sibling_fails_on_the_jump_line() {
    // The jump's target resolves after the jumper's frame closes, and a
    // miss is a Lua error placed on the `jump` call's line.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        var.before = true\n\
        jump('../Missing')\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn 'unexpected'\n```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error:?}");
    assert_eq!(
        error.at,
        at_line(9),
        "the `jump` call's line, not the block's first: {error:?}"
    );
}

#[test]
fn fanout_from_the_preamble_runs_a_top_level_worker_over_the_collection() {
    // `tasks.fanout` works in the preamble as in any section: the worker
    // resolves by a `../` path against the top-level sections and the arms
    // join in collection order.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local r = tasks.fanout('../Worker', {'a', 'b'})\n\
        var.answer = r[1].text .. '|' .. r[2].text\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.answer\n```\n\n\
        ## Worker\n\n\
        ```lua\nreturn 'item:' .. item\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "item:a|item:b");
}

#[test]
fn the_decision_tool_idiom_runs_in_the_preamble() {
    // The decision-tool idiom in the preamble: an adhoc tool with an enum
    // parameter is the verdict channel - the model's loop call lands in the
    // Lua handler, and the captured verdict drives the run's shape in the
    // sections after it. This needs `tools.adhoc`, `tools.offer`, and
    // `models.loop` in the preamble, which installs them on the one path
    // every section takes.
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "decide", "{\"choice\":\"use_mcp\"}")]),
        text("decided"),
    ]);
    let md = "---\nname: preamble-decision\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        tools.offer(tools.adhoc({ name = 'decide', description = 'Record the verdict', parameters = { choice = 'string' } }, function(args)\n\
          var.verdict = args.choice\n\
          return 'recorded'\n\
        end))\n\
        messages:user('interpret the guidance')\n\
        models.loop()\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.verdict\n```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "use_mcp");
    assert_eq!(
        gateway.call_count(),
        2,
        "the loop runs the tool-call round and the terminal text round"
    );
}

#[test]
fn a_level_one_section_above_level_two_sections_ends_the_run_without_running_them() {
    // With `# A` above `## B`, the top level is level 1 and `B` is a child
    // of `A`. The walk runs the top level and never descends, so the run
    // enters `A`, runs its blocks, and ends with the generic completion.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # A\n\n\
        ```lua\nstore.append('order.txt', 'A\\n')\n```\n\n\
        ## B\n\n\
        ```lua\nerror('a child must not run by fall-through')\n```\n";
    let trace = scheduler_scenario(md).run();

    assert_eq!(trace.text(), "done");
    assert_eq!(trace.read("order.txt").expect("order log"), "A\n");
}

#[test]
fn a_level_one_section_runs_its_level_two_child_when_it_calls_or_jumps_to_it() {
    // `B` is a child of `A`, so `A` names it by a child path: `call('B')`
    // runs it as a contained chain and returns its text, and `jump('B')`
    // moves the walk into it.
    for (lua, expected) in [
        ("return 'called:' .. call('B')", "called:b-ran"),
        ("jump('B')", "b-ran"),
    ] {
        let md = format!(
            "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
             # A\n\n\
             ```lua\n{lua}\n```\n\n\
             ## B\n\n\
             ```lua\nreturn 'b-ran'\n```\n"
        );
        let trace = scheduler_scenario(md).run();

        assert_eq!(trace.text(), expected, "{lua}");
    }
}
