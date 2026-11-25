//! Each prompt shipped in `prompts/` runs end to end on a scripted model:
//! it returns what it says it returns and sends the model what it says it
//! sends. The research prompt runs over its web tools, and the prose its
//! model reads names no tool by its id.

use std::fs;
use std::path::Path;

use runvil_engine::ToolCatalog;
use runvil_engine::effect::{ToolCallOrigin, ToolCaller};
use runvil_engine::model::{ModelRole, ThinkingMode};
use serde_json::{Value, json};

use super::prepare::refusal;
use super::support::{Scenario, TestResult, Tool, Trace, calls, model, text, tool_info};

/// The text of the shipped prompt `name`, read from the repository's
/// `prompts` folder.
fn shipped(name: &str) -> TestResult<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../prompts")
        .join(name);
    Ok(fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?)
}

/// The run's model rounds, in order: the role each ran under and the
/// messages it sent.
fn rounds(trace: &Trace) -> Vec<(&Value, &Value)> {
    trace
        .effects
        .iter()
        .filter_map(|logged| logged.effect_of("Chat"))
        .map(|chat| (&chat["alias"], &chat["messages"]))
        .collect()
}

/// Checks that the run made exactly one model round, under `role`, whose
/// only message was `prose`, and that the run returned `reply`.
fn assert_one_round(trace: &Trace, role: ModelRole, prose: &str, reply: &str) -> TestResult {
    let alias = json!(role.as_str());
    let messages = json!([{ "role": "user", "content": prose }]);
    assert_eq!(rounds(trace), [(&alias, &messages)]);
    assert_eq!(trace.text()?, reply);
    Ok(())
}

/// Runs the shipped research prompt for "Ada Lovelace" over a catalog
/// holding `web/fetch` and `web/search`, with a model that searches once
/// and then answers.
fn research_ada_lovelace() -> TestResult<Trace> {
    let query = json!({ "type": "object", "properties": { "query": { "type": "string" } } });
    let catalog = ToolCatalog::new([
        tool_info("web/fetch", query.clone())?,
        tool_info("web/search", query)?,
    ])?;
    let search = calls("web_search", json!({ "query": "Ada Lovelace" }))?;
    Scenario::tool(
        Tool::untrusted("web/search", "search results"),
        [search, text("a factual summary")],
    )
    .args(r#"{"text":"Ada Lovelace"}"#)
    .params(move |ctx| ctx.with_tools(catalog.clone()))
    .run(&shipped("research-person.md")?)
}

#[test]
fn the_shipped_echo_prompt_returns_its_text_and_asks_no_model() -> TestResult {
    let trace = Scenario::replies([])
        .args(r#"{"text":"ping"}"#)
        .run(&shipped("echo.md")?)?;
    assert_eq!(trace.text()?, "ping");
    assert!(
        rounds(&trace).is_empty(),
        "the prompt answers in Lua alone, with no model round"
    );
    Ok(())
}

#[test]
fn the_shipped_greet_prompt_sends_the_model_the_greeting_its_lua_computed() -> TestResult {
    let trace = Scenario::replies([text("the model's reply")])
        .args(r#"{"name":"Ada"}"#)
        .run(&shipped("greet.md")?)?;
    assert_one_round(
        &trace,
        ModelRole::Default,
        "Repeat exactly, with no extra words: Hello, Ada!",
        "the model's reply",
    )
}

#[test]
fn the_shipped_hello_prompt_sends_its_prose_and_returns_the_reply() -> TestResult {
    let trace = Scenario::replies([text("the model's reply")]).run(&shipped("hello.md")?)?;
    assert_one_round(
        &trace,
        ModelRole::Default,
        "Say \"Hello, world!\"",
        "the model's reply",
    )
}

#[test]
fn the_shipped_analyst_prompt_runs_its_round_on_the_strong_role() -> TestResult {
    let trace = Scenario::replies([text("a short summary")])
        .roles([
            (
                ModelRole::Default,
                model("everyday", 8_192, ThinkingMode::Never)?,
            ),
            (
                ModelRole::Strong,
                model("careful", 40_000, ThinkingMode::Never)?,
            ),
        ])
        .args(r#"{"text":"the input"}"#)
        .run(&shipped("analyst-example.md")?)?;
    assert_one_round(
        &trace,
        ModelRole::Strong,
        "Analyze the following input carefully and return a short factual summary with no preamble:\n\nthe input",
        "a short summary",
    )
}

#[test]
fn the_shipped_analyst_prompt_is_refused_when_its_strong_model_is_one_token_short() -> TestResult {
    let scenario = Scenario::replies([])
        .model(model("nearly", 39_999, ThinkingMode::Never)?)
        .args(r#"{"text":"the input"}"#);
    let message = refusal(scenario, &shipped("analyst-example.md")?)?;
    assert!(
        message.contains(
            "role 'strong': requires a context of at least 40000 tokens; the model filling it provides 39999"
        ),
        "{message}"
    );
    Ok(())
}

#[test]
fn the_shipped_research_prompt_offers_its_web_tools_by_id_and_runs_end_to_end() -> TestResult {
    let trace = research_ada_lovelace()?;
    assert_eq!(trace.text()?, "a factual summary");

    let (mut advertised, mut tool_calls) = (Vec::new(), Vec::new());
    for logged in &trace.effects {
        if let Some(chat) = logged.effect_of("Chat") {
            advertised.push(serde_json::from_value::<Vec<String>>(
                chat["tools"].clone(),
            )?);
        }
        if let Some(call) = logged.effect_of("ToolCall") {
            let origin: ToolCallOrigin = serde_json::from_value(call["origin"].clone())?;
            tool_calls.push((call["tool"].clone(), call["alias"].clone(), origin.caller));
        }
    }
    let wire_names = vec!["web_search".to_owned(), "web_fetch".to_owned()];
    assert_eq!(
        advertised,
        [wire_names.clone(), wire_names],
        "both rounds advertise the offered tools under their wire names, in offer order"
    );
    assert_eq!(
        tool_calls,
        [(json!("web/search"), json!("web_search"), ToolCaller::Model)],
        "the model's call by wire name reaches the tool its id names"
    );
    Ok(())
}

#[test]
fn the_shipped_research_prompt_names_no_tool_by_its_id_in_what_the_model_reads() -> TestResult {
    let trace = research_ada_lovelace()?;
    let mut checked = 0;
    for logged in &trace.effects {
        let Some(chat) = logged.effect_of("Chat") else {
            continue;
        };
        let messages = chat["messages"]
            .as_array()
            .ok_or("a chat record holds its messages as a list")?;
        let user = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"));
        for message in user {
            let wire = serde_json::to_string(message)?;
            for id in ["web/search", "web/fetch"] {
                assert!(
                    !wire.contains(id),
                    "the model only knows tools by wire name, but a user message names the id `{id}`: {wire}"
                );
            }
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the run sends the model at least one user message"
    );
    Ok(())
}
