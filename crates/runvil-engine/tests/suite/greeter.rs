//! The greeter, one tour at a time, up to the complete program in
//! `examples/greeter.rs`.

use runvil_engine::Prompt;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::{Effect, EffectAnswer};
use runvil_engine::event::Event;

use super::support::{Scenario, TestResult, note, preamble, prompt, shout, text};

#[path = "../../examples/greeter.rs"]
#[expect(
    unreachable_pub,
    reason = "the example's items are pub at its own crate root, and the suite reaches them through this private module"
)]
#[expect(dead_code, reason = "only the example binary calls its main")]
mod example;

use example::{GREETER, answer};

const REPLY: &str = "
    store.write('note.md', 'hello')
    return models.infer(store.read('note.md'))
";

#[test]
fn the_smallest_greeter_parses_and_a_caller_reads_its_file_and_frontmatter_name() -> TestResult {
    let source = note("'hello'");
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt")?;
    assert_eq!(parsed.file(), "prompt");
    assert_eq!(parsed.frontmatter().name(), "greeter");
    Ok(())
}

#[test]
fn a_body_of_one_lua_fence_and_no_heading_runs_and_returns_its_value() -> TestResult {
    let trace = Scenario::default().run(&preamble("return 'hello'", &[]))?;
    assert_eq!(trace.text()?, "hello");
    Ok(())
}

#[test]
fn a_preamble_runs_first_and_the_run_falls_through_to_the_section_below_it() -> TestResult {
    let greet = "return var.order .. ', then Greet'";
    let source = preamble("var.order = 'the preamble'", &[("Greet", greet)]);
    let trace = Scenario::default().run(&source)?;
    assert_eq!(trace.text()?, "the preamble, then Greet");

    let started: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            Event::SectionStarted { section, .. } => Some(section.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        started,
        ["", "Greet"],
        "the preamble's entry reports the empty section name"
    );
    Ok(())
}

#[test]
fn answering_each_store_effect_from_the_runs_store_returns_the_note() -> TestResult {
    let trace = Scenario::default().run(&note("'hello'"))?;
    assert_eq!(trace.text()?, "hello");
    Ok(())
}

#[test]
fn the_example_answers_a_timer_as_fired_at_once() {
    assert!(matches!(
        answer(Effect::Timer {
            duration: std::time::Duration::from_secs(3_600)
        }),
        EffectAnswer::Timer
    ));
}

#[test]
fn a_prompt_declaring_no_models_runs_on_the_default_model_and_the_canned_reply_becomes_the_result()
-> TestResult {
    let source = prompt("", &[("Greet", REPLY)]);
    let trace = Scenario::replies([text("hi there")]).run(&source)?;
    assert_eq!(trace.text()?, "hi there");
    Ok(())
}

#[test]
fn trusted_tool_output_reaches_the_calling_lua_unchanged() -> TestResult {
    let trace = Scenario::tool(shout(), [text("hi there")]).run(GREETER)?;
    assert_eq!(trace.text()?, "HI THERE");
    Ok(())
}

#[test]
fn a_cancel_from_another_thread_ends_the_run_canceled_once_held_effects_drop() -> TestResult {
    let cancel = CancelHandle::new();
    let handed = cancel.clone();
    let mut driver = Scenario::tool(shout(), [])
        .hold(|effect| matches!(effect, Effect::Chat { .. }))
        .params(move |ctx| ctx.with_cancel(handed.clone()))
        .start(GREETER)?;
    driver.pump_until_held()?;

    std::thread::spawn(move || cancel.cancel())
        .join()
        .map_err(|_| "the canceling thread panicked")?;

    assert!(
        driver.pump()? == 0 && driver.held() == 0,
        "the step that decides the run withdraws the held chat effect"
    );
    assert!(driver.finish()?.error()?.is_canceled());
    Ok(())
}

#[test]
fn the_complete_greeter_returns_hi_there_and_its_log_opens_with_the_runs_start() -> TestResult {
    assert_eq!(example::greet()?, "HI THERE");
    Ok(())
}
