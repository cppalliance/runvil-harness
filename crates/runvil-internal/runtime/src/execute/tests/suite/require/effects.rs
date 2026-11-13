//! Library code issues effects as part of the calling section: `store`,
//! model, and tool calls while it loads, and the state a library the
//! preamble requires sets in `var` for the sections after it.

use crate::execute::tests::fixtures::echo_tool;
use crate::model::ToolSchema;
use crate::test_support::scripted_chat::{ScriptedChat, text};

use super::*;

/// A `require` case over `source` with the echo tool, the test model as
/// the current model, and `chat` answering its model rounds.
fn effect_case(source: &PromptSource, chat: &ScriptedChat) -> Scenario {
    require_case(source.build())
        .tool(echo_tool())
        .chat(chat)
        .bound()
}

/// The fixture tools Pack every effect case declares.
fn effect_source() -> PromptSource {
    PromptSource::new().front("uses: [tools]")
}

#[test]
fn library_code_makes_store_model_and_tool_calls_while_it_loads() {
    let source = effect_source()
        .section("Main", "return require '/loader'")
        .section(
            "loader",
            "
            store.write('lib.md', 'stored')
            local echoed = tools.call('tools/echo', { value = 'lib' })
            local reply = models.infer('ping')
            return table.concat({ store.read('lib.md'), tostring(echoed), tostring(reply) }, '|')",
        );
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let trace = effect_case(&source, &gateway).run();
    assert_eq!(trace.text(), "stored|echoed: lib|pong");
    assert_eq!(trace.read("lib.md").expect("the write landed"), "stored");
    assert_eq!(gateway.requests().len(), 1, "one model round ran");
}

#[test]
fn a_library_the_preamble_requires_sets_state_in_var_that_the_next_section_sees() {
    let source = effect_source()
        .preamble("require '/setup'")
        .section(
            "Main",
            "
            require '/offers'
            messages:user('ping')
            models.loop()
            return var.tag .. ':' .. messages[#messages].content",
        )
        .section("setup", "var.tag = 'ready'")
        .section("offers", "tools.offer('tools/echo')");
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let trace = effect_case(&source, &gateway).run();
    assert_eq!(
        trace.text(),
        "ready:pong",
        "the loop runs after the preamble's library set the state the section reads"
    );
    let round = gateway
        .last_request()
        .expect("the round reaches the gateway");
    let offered: Vec<&str> = round.tools.iter().map(ToolSchema::name).collect();
    assert_eq!(
        offered,
        ["tools_echo"],
        "the offers library's offer reaches the requiring section's round"
    );
}
