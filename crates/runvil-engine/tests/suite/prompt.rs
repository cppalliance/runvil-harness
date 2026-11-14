//! Reading what a prompt declares before it runs.

use runvil_engine::ErrorCode;
use runvil_engine::Prompt;
use runvil_engine::model::{ModelRole, ThinkingMode};
use serde_json::json;

use super::prepare::refusal;
use super::support::{Built, Scenario, TestResult, model, prompt};

const CONTRACT: &str = "
    uses: [text]
    args: { type: object, properties: { times: { type: integer, default: 1 }, name: { type: string } }, required: [name] }
    models: { fast: {} }
";

#[test]
fn the_frontmatter_lists_every_declaration_and_gives_its_arguments_as_json_schema() -> TestResult {
    let source = prompt(CONTRACT, &[("Greet", "return 'done'")]);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt")?;
    let frontmatter = parsed.frontmatter();
    assert_eq!(frontmatter.name(), "greeter");
    assert_eq!(
        frontmatter.description(),
        "A prompt the facade suite builds."
    );

    let packs: Vec<_> = frontmatter
        .uses()
        .iter()
        .map(|entry| (entry.vendor(), entry.id().to_string()))
        .collect();
    assert_eq!(packs, [("runvil", "text".to_owned())]);
    let roles: Vec<_> = ModelRole::ALL
        .into_iter()
        .filter_map(|role| {
            frontmatter
                .role_requirements(role)
                .map(|declared| (role, declared.min_context()))
        })
        .collect();
    assert_eq!(roles, [(ModelRole::Fast, None)]);

    assert_eq!(
        frontmatter.args().as_json(),
        &json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "times": { "type": "integer", "default": 1 },
            },
            "required": ["name"],
            "additionalProperties": false,
        }),
        "the declaration reads back as plain JSON Schema, each closed object saying so"
    );

    let misspelled = source.replace("uses:", "use:");
    let parsed = Prompt::parse(&|_: &str| Ok(misspelled.clone()), "prompt");
    assert_eq!(
        parsed.err().map(|error| error.code),
        Some(ErrorCode::FrontmatterInvalid)
    );
    Ok(())
}

#[test]
fn the_prompt_is_named_by_the_name_its_finder_was_asked_for() -> TestResult {
    let source = prompt(CONTRACT, &[("Greet", "return 'done'")]);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "greeter.md")?;
    assert_eq!(parsed.file(), "greeter.md");
    Ok(())
}

#[test]
fn a_role_check_by_hand_finds_the_same_shortfall_the_run_reports() -> TestResult {
    let source = prompt(
        "models: { fast: {}, small: { thinking: false, min_context: 100000 } }",
        &[("Greet", "return 'done'")],
    );
    let model = model("canned", 200_000, ThinkingMode::Always)?;
    let scenario = Scenario::default().model(model.clone());
    let Built { prompt, .. } = scenario.build(&source)?;

    let mut misses = Vec::new();
    for role in ModelRole::ALL {
        let Some(declared) = prompt.frontmatter().role_requirements(role) else {
            continue;
        };
        let thinking_miss = match declared.thinking() {
            Some(true) => model.thinking() == ThinkingMode::Never,
            Some(false) => model.thinking() == ThinkingMode::Always,
            None => false,
        };
        let context_miss = declared
            .min_context()
            .is_some_and(|min| min > model.context());
        if thinking_miss || context_miss {
            misses.push((role, thinking_miss, context_miss));
        }
    }
    assert_eq!(misses, [(ModelRole::Small, true, false)]);

    assert_eq!(
        refusal(scenario, &source)?,
        "the environment cannot satisfy this prompt:\n\
         - role 'small': requires 'thinking: false'; \
         the thinking capability of the model filling it is Always (at prompt:5)",
        "only the small role falls short, on its thinking requirement"
    );
    Ok(())
}
