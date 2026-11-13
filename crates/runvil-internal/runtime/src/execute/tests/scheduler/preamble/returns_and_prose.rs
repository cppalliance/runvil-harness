//! Preamble returns, fall-through, and prose: a preamble-only prompt's
//! return or fall-through, a preamble return that ends the run whether or
//! not it gives a value, a preamble that runs off its end and chains into
//! the next section with no jump, preamble prose read only through an
//! explicit infer, a prose-only preamble, which is no section, and a
//! prompt with no section at all.

use super::*;
use crate::execute::tests::context::sequence;

#[test]
fn a_preamble_only_prompt_returns_its_value() {
    // A preamble-only prompt's scalar return is the run's result.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nreturn \"hello\"\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "hello");
}

#[test]
fn a_preamble_only_prompt_that_returns_nothing_ends_with_done() {
    // A preamble-only prompt that produces nothing ends in the shared
    // generic completion.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nlocal x = 1\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "done");
}

#[test]
fn a_preamble_return_ends_the_run_with_its_value_and_no_later_section_runs() {
    // A `return` in the preamble ends the root chain, so it ends the run
    // with its value and no later section ever runs - the erroring
    // section is the tripwire.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nreturn 'early'\n```\n\n\
        ## Never\n\n\
        ```lua\nerror('no section runs after a preamble return')\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "early");
}

#[test]
fn a_preamble_return_with_no_value_ends_the_run_with_done() {
    // Any `return` in the preamble ends the root chain, so neither a later
    // preamble block nor a section runs, and a `return` that gives no
    // value ends the run with the shared generic completion.
    for ending in [
        "return",
        "return nil",
        "if true then return end",
        "return store.write('preamble.txt', 'kept')",
    ] {
        let md = format!(
            "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
             ```lua\n{ending}\n```\n\n\
             ```lua\nerror('a later preamble block must not run')\n```\n\n\
             ## Never\n\n\
             ```lua\nerror('no section runs after a preamble return')\n```\n"
        );
        let trace = preamble_scenario(&md).run();

        assert_eq!(trace.text(), "done", "{ending:?}");
    }
}

#[test]
fn a_preamble_that_runs_off_its_end_falls_through_to_the_next_section() {
    // A `return` the preamble never executes ends nothing: the preamble
    // runs off its last block and the walk falls through to the next
    // section, in the same chain.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nif false then return end\nvar.trail = 'first'\n```\n\n\
        ```lua\nvar.trail = var.trail .. ',second'\n```\n\n\
        ## Result\n\n\
        ```lua\nreturn var.trail .. ',section'\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "first,second,section");
}

#[test]
fn a_preamble_chains_into_the_next_section_with_no_jump() {
    // The preamble is the first top-level section and `Research` is its
    // next sibling, so the walk falls through from one to the other: the
    // preamble's block runs, then `Research` runs, and the run's text is
    // the return of `Research`.
    let md = "---\nname: research\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nlog(\"test\")\n```\n\n\
        ## Research\n\n\
        ```lua\nreturn \"chained with no jump\"\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "chained with no jump");
    let observed = sequence(&trace);
    let logged = (
        String::new(),
        Observation::Lua("test".to_owned()).to_string(),
    );
    let entered = ("Research".to_owned(), detail::SECTION_STARTED.to_string());
    let logged_at = observed.iter().position(|record| *record == logged);
    let entered_at = observed.iter().position(|record| *record == entered);
    assert!(
        logged_at.is_some() && logged_at < entered_at,
        "the preamble's block runs before `Research` starts: {observed:?}"
    );
}

#[test]
fn preamble_prose_inferred_explicitly_is_the_run_result() {
    // Prose in a preamble with Lua is live prose for the next Lua block. A
    // preamble-only prompt whose Lua reads it into an explicit infer ends
    // the run with the inferred text: the scalar return ends the root
    // chain.
    let gateway = ScriptedChat::new(vec![text("preamble reply")]);
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        say something\n\n\
        ```lua\n\
        return models.infer(prose)\n\
        ```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "preamble reply");
    assert_eq!(request_prompts(&gateway), ["say something"]);
}

#[test]
fn preamble_prose_and_section_prose_each_infer_explicitly_in_source_order() {
    // The preamble and the section after it each read their own pending
    // prose into an explicit infer - two completions, in source order.
    let gateway = ScriptedChat::new(vec![text("preamble reply"), text("section reply")]);
    let md = "---\nname: shared-loop\ndescription: d\nrunvil: 0\n---\n\n\
        preamble prose turn\n\n\
        ```lua\n\
        var.first = models.infer(prose)\n\
        ```\n\n\
        ## Section Two\n\n\
        section prose turn\n\n\
        ```lua\n\
        return models.infer(prose)\n\
        ```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "section reply");
    assert_eq!(
        gateway.call_count(),
        2,
        "the preamble prose and the section prose each drive exactly one completion"
    );
    let requests = gateway.requests();
    let first_prose = requests[0].messages[0].content();
    let second_prose = requests[1].messages[0].content();
    assert!(
        first_prose.contains("preamble prose turn"),
        "the first completion is the preamble prose: {first_prose}"
    );
    assert!(
        second_prose.contains("section prose turn"),
        "the second completion is the section prose: {second_prose}"
    );
}

#[test]
fn unread_preamble_prose_stays_inert_and_explicit_infer_requires_a_model() {
    // Preamble prose does not drive inference, so an unread buffer - even
    // one whose substitution would fail or stay empty - discards at the
    // preamble's end without requiring a model. Only an explicit
    // `models.infer` of the prose requires a binding.
    let unread = "---\nname: unread-prose\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\nvar.omit = ''\n```\n\n\
        {{ var.omit }}\n\n\
        ## Result\n\n\
        ```lua\nreturn 'ok'\n```\n";
    assert_eq!(Scenario::new(unread).run().text(), "ok");

    let reading = "---\nname: read-prose\ndescription: d\nrunvil: 0\n---\n\n\
        ask\n\n\
        ```lua\nreturn models.infer(prose)\n```\n";
    let trace = Scenario::new(reading).run();
    let error = trace.error();
    assert!(
        error.code == ErrorCode::ModelRequired,
        "expected ModelRequired, got {error}"
    );
}

#[test]
fn preamble_prose_infers_explicitly_and_var_accumulates_into_the_next_section() {
    // The preamble reads its pending prose only through an explicit infer,
    // and `var` writes accumulate across its blocks into the next section.
    let gateway = ScriptedChat::new(vec![text("final answer")]);
    let md = "---\nname: live-preamble-prose\ndescription: d\nrunvil: 0\n---\n\n\
        ```lua\n\
        var.executions = (var.executions or 0) + 1\n\
        ```\n\n\
        Ask for one round.\n\n\
        ```lua\n\
        var.first = models.infer(prose)\n\
        var.executions = var.executions + 1\n\
        ```\n\n\
        ## Result\n\n\
        ```lua\n\
        return var.first .. ':' .. var.executions\n\
        ```\n";
    let trace = preamble_scenario(md).chat(&gateway).run();

    assert_eq!(trace.text(), "final answer:2");
}

#[test]
fn a_prose_only_preamble_is_no_section_and_the_first_heading_takes_entry_0_0() {
    // Text before the first heading with no `lua` fence is commentary: the
    // prompt has no preamble, the run starts at the first heading's
    // section as entry `0.0`, and nothing reports under the empty name.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        Notes for whoever maintains this prompt.\n\n\
        ## First\n\n\
        ```lua\nreturn sys.section_name .. ':' .. sys.id\n```\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "First:0.0");
    let observed = sequence(&trace);
    assert!(
        !observed.iter().any(|(section, _)| section.is_empty()),
        "no section reports under the empty name: {observed:?}"
    );
}

#[test]
fn a_prompt_with_no_section_ends_with_done() {
    // Frontmatter and prose alone are a valid prompt with no section: the
    // root chain has no section to enter, so the run ends with the shared
    // generic completion and reports its two boundaries alone.
    let md = "---\nname: no-section\ndescription: d\nrunvil: 0\n---\n\n\
        Only commentary: no heading and no Lua.\n";
    let trace = preamble_scenario(md).run();

    assert_eq!(trace.text(), "done");
    assert_eq!(
        sequence(&trace),
        vec![
            ("no-section".to_owned(), detail::RUN_STARTED.to_string()),
            ("no-section".to_owned(), detail::RUN_SUCCEEDED.to_string()),
        ]
    );
}
