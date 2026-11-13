//! The preamble as the root chain's first section entry: its infer and
//! default-model selection, its entry id and empty section name, the
//! section boundaries it reports and the run boundaries around them, the
//! `var`, concurrency limit, and task that last into the next section,
//! read-only `argv`, a Lua error that ends the run as `LuaRuntime` on its
//! line, `var` read back at a return and at the fall-through, a failed
//! substitution that keeps its own code, and a failing library.

use super::*;
use crate::execute::tests::context::sequence;

#[test]
fn a_preamble_infer_on_a_model_handle_runs_one_round() {
    // The preamble selects the default model by label, `models.infer` on
    // that handle yields through the shim, and the `var` it writes reads in
    // the next section.
    let gateway = ScriptedChat::new(vec![text("preamble answer")]);
    let md = "---\nname: live-preamble\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local writer = models.get('default')\n\
        var.answer = models.infer(writer, 'answer once')\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.answer\n```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "preamble answer");
    assert_eq!(gateway.call_count(), 1);
}

#[test]
fn a_preamble_models_infer_resolves_the_default_model_without_touching_sys() {
    // `models.infer` with no handle resolves the preamble's current model
    // from the shared set and runs the one infer shape - a single
    // tool-free round on a fresh conversation that leaves `sys` untouched.
    let gateway = ScriptedChat::new(vec![text("preamble answer")]);
    let md = "---\nname: live-preamble-models-infer\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        var.answer = models.infer('answer once')\n\
        var.sys_untouched = not pcall(function() return sys.reply_finish_reason end)\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.answer .. ':' .. tostring(var.sys_untouched)\n```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "preamble answer:true");
    assert_eq!(gateway.call_count(), 1);
    let body = gateway
        .last_request()
        .expect("infer must reach the gateway");
    assert_eq!(
        body.options.model(),
        "claude-sonnet-4-6",
        "models.infer must use the section's current model"
    );
    assert!(
        body.tools.is_empty(),
        "models.infer advertises no tools: {body:?}"
    );
    assert_eq!(
        body.messages.len(),
        1,
        "models.infer runs on a fresh context: {body:?}"
    );
}

#[test]
fn sys_id_is_0_0_in_the_preamble_and_0_1_in_the_next_section() {
    // The preamble is the first section the root chain enters, so it takes
    // entry 0, and the section after it takes entry 1 of the same chain.
    let md = "---\nname: live-preamble-sys-id\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.preamble_id = sys.id\n```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.preamble_id .. ',' .. sys.id\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "0.0,0.1");
}

#[test]
fn with_no_preamble_the_first_section_takes_sys_id_0_0() {
    // Entry ids count the sections the root chain enters, so with no
    // preamble the first heading's section is entry 0.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ## First\n\n\
        ```lua\nvar.first_id = sys.id\n```\n\n\
        ## Second\n\n\
        ```lua\nreturn var.first_id .. ',' .. sys.id\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "0.0,0.1");
}

#[test]
fn sys_section_name_is_the_empty_string_in_the_preamble() {
    // The preamble's name is the empty string, and the section after it
    // reads its own heading.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.preamble_name = sys.section_name\n```\n\n\
        ## Result\n\n\
        ```lua\nreturn '[' .. var.preamble_name .. ']|' .. sys.section_name\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "[]|Result");
}

#[test]
fn the_preamble_reports_its_section_boundaries_under_the_empty_section_name() {
    // The preamble is a section entry like any other: it reports
    // SECTION_STARTED and, at the fall-through, SECTION_FINISHED, both
    // under the empty section name and both before the next section's
    // SECTION_STARTED.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.x = 1\n```\n\n\
        ## Only\n\n\
        ```lua\nreturn 'done-now'\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "done-now");
    let started = detail::SECTION_STARTED.to_string();
    let finished = detail::SECTION_FINISHED.to_string();
    let boundaries: Vec<(String, String)> = sequence(&trace)
        .into_iter()
        .filter(|(_, event)| event == &started || event == &finished)
        .collect();
    assert_eq!(
        boundaries,
        vec![
            (String::new(), started.clone()),
            (String::new(), finished.clone()),
            ("Only".to_owned(), started.clone()),
            ("Only".to_owned(), finished.clone()),
        ]
    );
}

#[test]
fn the_run_boundaries_report_under_the_frontmatter_name() {
    // The run's own boundaries report under the prompt's frontmatter
    // `name`, and every section's reports sit between them under the
    // section's name.
    let md = "---\nname: boundaries\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.x = 1\n```\n\n\
        ## Only\n\n\
        ```lua\nreturn 'done-now'\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "done-now");
    let observed = sequence(&trace);
    assert_eq!(
        observed.first(),
        Some(&("boundaries".to_owned(), detail::RUN_STARTED.to_string())),
        "{observed:?}"
    );
    assert_eq!(
        observed.last(),
        Some(&("boundaries".to_owned(), detail::RUN_SUCCEEDED.to_string())),
        "{observed:?}"
    );
    let under_the_name = observed
        .iter()
        .filter(|(section, _)| section == "boundaries")
        .count();
    assert_eq!(
        under_the_name, 2,
        "only the two run boundaries report under the frontmatter name: {observed:?}"
    );
}

#[test]
fn var_set_in_the_preamble_reads_in_the_next_section() {
    // The preamble and the section after it are entries of one chain, and
    // `var` is the chain's, so a write in the preamble reads in the next
    // section.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.note = 'set in the preamble'\n```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.note\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "set in the preamble");
}

#[test]
fn a_limit_lowered_in_the_preamble_is_the_next_sections_limit() {
    // The concurrency limit is the chain's, so a `tasks.concurrency` call
    // in the preamble still bounds the tasks the next section spawns. Were
    // the limit read from the run's ceiling at each section entry, the next
    // section would read back 8 here.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local n = tasks.concurrency(2)\n\
        assert(n == 2, 'the preamble sets its own limit, got ' .. n)\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\nreturn 'limit: ' .. tasks.concurrency()\n```\n";
    let trace = preamble_scenario(md).limits(ceiling(8)).run();

    assert_eq!(
        trace.text(),
        "limit: 2",
        "the next section keeps the limit the preamble lowered, not the ceiling"
    );
}

#[test]
fn a_task_spawned_in_the_preamble_is_joined_in_the_next_section() {
    // A task belongs to the chain that spawned it, and the preamble and
    // the section after it are one chain, so the next section joins the
    // task as its owner. A wait on a task the caller does not own raises,
    // so the join's result is the proof. The handle crosses in `var`.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.worker = tasks.spawn('../Worker')\n```\n\n\
        ## Result\n\n\
        ```lua\n\
        local results = tasks.join({ var.worker })\n\
        assert(results[1].ok, 'the task succeeded')\n\
        return var.worker.task .. ':' .. results[1].result\n\
        ```\n\n\
        ## Worker\n\n\
        ```lua\nreturn 'worked'\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "0.0:worked");
}

#[test]
fn argv_refuses_a_write_in_the_preamble_and_in_a_later_section() {
    // `argv` is read-only in every section, the preamble included: an
    // assignment raises the refusal, which `pcall` catches.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local ok, err = pcall(function() argv = 'rewritten' end)\n\
        assert(not ok, 'the preamble write is refused')\n\
        var.refusal = tostring(err)\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\n\
        local ok, err = pcall(function() argv = 'rewritten' end)\n\
        assert(not ok, 'the section write is refused')\n\
        return var.refusal .. '|' .. tostring(err)\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let out = trace.text();
    assert_eq!(
        out.matches("argv is read-only: assign to `var` or a section global instead")
            .count(),
        2,
        "both writes are refused with the rule: {out}"
    );
}

#[test]
fn a_preamble_lua_error_ends_the_run_as_lua_runtime() {
    // A Lua error in the preamble is a Lua error like any section's: the
    // run ends with the `LuaRuntime` code and the error's own text, and
    // neither a later preamble block nor the next section runs.
    // `RequirementsUnmet` is the code of the run's start checks alone.
    let md = "---\nname: preamble-error\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        assert(false, 'the input is not ready')\n\
        ```\n\n\
        ```lua\nstore.write('later.txt', 'ran')\n```\n\n\
        ## Result\n\n\
        ```lua\nstore.write('section.txt', 'ran')\nreturn 'unexpected'\n```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::LuaRuntime,
        "a preamble error has the Lua code: {error:?}"
    );
    assert!(
        error.message.contains("the input is not ready"),
        "the message holds the error's own text: {error:?}"
    );
    assert!(
        trace.read("later.txt").is_err(),
        "the later preamble block must not run after the error"
    );
    assert!(
        trace.read("section.txt").is_err(),
        "the next section must not run after the error"
    );
}

#[test]
fn a_preamble_lua_error_is_placed_on_its_failing_line() {
    let md = "---\nname: preamble-error\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local ready = false\n\
        assert(ready, 'the input is not ready')\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error:?}");
    assert_eq!(
        error.at,
        at_line(9),
        "the failure is on the line that raised it, not the block's first line: {error:?}"
    );
    let message = error.located_message();
    assert!(message.ends_with(" (at prompt:9)"), "{message}");
}

#[test]
fn a_failing_preamble_chunk_reports_failed_under_the_empty_section_name() {
    // The observation side of a preamble error: the chunk reports
    // LUA_CHUNK_FAILED under the preamble's empty name, the preamble
    // reports started and never finished, and the run reports its failure
    // under the frontmatter `name`.
    let md = "---\nname: failing-preamble\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        assert(false, 'the input is not ready')\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error}");
    let observed = sequence(&trace);
    assert!(
        observed.contains(&(String::new(), detail::SECTION_STARTED.to_string())),
        "the preamble reports started: {observed:?}"
    );
    assert!(
        observed.contains(&(String::new(), detail::LUA_CHUNK_FAILED.to_string())),
        "the failing chunk reports failed: {observed:?}"
    );
    assert!(
        !observed
            .iter()
            .any(|(_, event)| event == &detail::SECTION_FINISHED.to_string()),
        "the failing preamble never reports finished: {observed:?}"
    );
    assert_eq!(
        observed.last(),
        Some(&(
            "failing-preamble".to_owned(),
            detail::RUN_FAILED.to_string()
        )),
        "{observed:?}"
    );
}

#[test]
fn a_preamble_scalar_return_still_reads_var_back() {
    // The read-back half of the return rule: a section's final `var` is
    // read back on every exit, so a reassigned `var` global fails the run
    // even when the block's scalar return would end it.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        var = 5\n\
        return 'early'\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::LuaRuntime,
        "the read-back failure has the Lua code: {error}"
    );
    assert!(
        error.to_string().contains("global was reassigned"),
        "the read-back failure must name the cause: {error}"
    );
    assert_eq!(
        error.at,
        at_line(8),
        "the read-back fails on the returning block's line: {error:?}"
    );
}

#[test]
fn a_preamble_return_with_no_value_still_reads_var_back() {
    // The read-back runs for a `return` that gives no value as it does for
    // one that gives a scalar, so the reassigned `var` fails the run
    // instead of ending it with the generic completion.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        var = 5\n\
        return\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error}");
    assert!(
        error.to_string().contains("global was reassigned"),
        "the read-back failure must name the cause: {error}"
    );
    assert_eq!(
        error.at,
        at_line(8),
        "the read-back fails on the returning block's line: {error:?}"
    );
}

#[test]
fn a_preamble_var_read_back_failure_at_the_fall_through_is_placed_on_its_last_lua_block() {
    // The preamble falls through, so its block index is past its last
    // block: the failure lands on the last Lua block's first line.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        local first = true\n\
        ```\n\n\
        ```lua\n\
        local broken = true\n\
        var = 5\n\
        ```\n\n\
        Trailing commentary.\n\n\
        ## Result\n\n\
        ```lua\nreturn 'unexpected'\n```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error:?}");
    assert_eq!(error.at, at_line(12), "{error:?}");
}

#[test]
fn a_failed_substitution_in_preamble_prose_keeps_its_code_on_its_template_line() {
    // A `{{ }}` failure in the preamble's prose arrives placed on its
    // template's line with its own code, as in any section.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        Needs {{ var.missing }}.\n\n\
        ```lua\n\
        return prose\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(error.code, ErrorCode::Substitution, "{error:?}");
    assert_eq!(error.at, at_line(7), "{error:?}");
}

#[test]
fn a_failing_library_required_from_the_preamble_ends_the_run_as_lua_runtime() {
    // A library runs inside the chunk that requires it, so its uncaught
    // raise is that chunk's own Lua error: the run ends as `LuaRuntime`
    // with the library's raise as the message, placed on the library's
    // line.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        require '/broken'\n\
        var.ok = true\n\
        ```\n\n\
        ## broken\n\n\
        ```lua\n\
        error('library boom')\n\
        ```\n";
    let trace = preamble_scenario(md).run();

    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::LuaRuntime,
        "the library's raise is the preamble chunk's Lua error: {error}"
    );
    assert!(
        error.message.contains("library boom"),
        "the message is the library's raise: {error}"
    );
    assert_eq!(error.at, at_line(15), "{error:?}");
}
