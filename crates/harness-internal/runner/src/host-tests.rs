//! Tests for `Host::parse`: a prompt found by its name parses and names
//! itself by that name, a finder or parse failure comes back as given, a
//! `uses:` entry naming a Pack not installed or inserted unavailable is
//! refused naming the Pack and its reason, and one naming a Pack whose
//! factory refuses parses, since parsing makes no Pack. A prompt that
//! links others parses with them, and an entry of a linked prompt is
//! refused the same way, naming that prompt.
//!
//! Tests for the build: a Host built with no Plugin has the two built-in
//! Packs and no report, Rust Plugins run in the order written, so the
//! first one keeps a Pack id that two of them insert, a Rust Plugin that
//! fails is the one failure of its report while the Packs it inserted
//! stay, and `Host::packs` lists a Pack inserted unavailable.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use runvil_engine::{Error, ErrorCode, ErrorFact, Prompt, SourceLoc, make_tool_error};
use runvil_plugin::{BoxFuture, Pack, PackId, PackInfo, ToolContext, ToolInfo, ToolOutput};
use serde_json::Value;

use super::{Host, HostBuilder};
use crate::prompt_finders::PromptSet;

/// A Pack with no tools that answers no call.
struct Idle;

impl Pack for Idle {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async {
            Err(make_tool_error(
                ErrorCode::ToolOther,
                "idle answers no call",
            ))
        })
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn builds(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Idle))
}

fn fails(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Err(make_tool_error(
        ErrorCode::ToolOther,
        "the broken backend is down",
    ))
}

/// A Host holding `acme/kit`, whose factory makes it, `acme/broken`,
/// whose factory refuses, and `acme/offline`, inserted unavailable, that
/// finds each prompt in `sources` by its name.
fn host(sources: &[(&str, String)]) -> Arc<Host> {
    let sources: BTreeMap<String, String> = sources
        .iter()
        .map(|(name, source)| ((*name).to_owned(), source.clone()))
        .collect();
    let finder = move |name: &str| {
        sources
            .get(name)
            .cloned()
            .ok_or_else(|| missing_prompt(name))
    };
    Host::builder(Arc::new(finder))
        .with_rust_plugin(|packs| {
            packs.insert(PackInfo::new("acme/kit"), builds).unwrap();
            packs.insert(PackInfo::new("acme/broken"), fails).unwrap();
            packs
                .insert_unavailable(
                    PackInfo::new("acme/offline"),
                    make_tool_error(ErrorCode::ToolOther, "offline takes no configuration"),
                )
                .unwrap();
            Ok(())
        })
        .build()
}

/// The builder of a Host whose finder serves no prompt.
fn builder() -> HostBuilder {
    Host::builder(Arc::new(PromptSet::new()))
}

/// The name of each of `host`'s Packs, in the order `Host::packs` lists
/// them.
fn fqns(host: &Host) -> Vec<&'static str> {
    host.packs().map(|info| info.fqn).collect()
}

/// The finder's failure for a prompt it has no source for.
fn missing_prompt(name: &str) -> Error {
    let code = ErrorCode::NotFound;
    Error::new(code, format!("no prompt named {name}"), code.prose())
}

/// A prompt declaring the given `uses:` lines.
fn declaring(uses: &str) -> String {
    format!(
        "---\nname: research\ndescription: d\nrunvil: 0\n{uses}---\n\n\
         ## Only\n\nDone.\n"
    )
}

#[test]
fn a_prompt_whose_declared_packs_are_all_installed_parses_under_its_name() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/kit\n"))]);
    let prompt = host
        .parse("research.md")
        .expect("every declared Pack is installed");
    assert_eq!(prompt.file(), "research.md");
    assert_eq!(prompt.frontmatter().name(), "research");
}

#[test]
fn a_uses_entry_naming_an_uninstalled_pack_is_refused_naming_it() {
    let host = host(&[(
        "research.md",
        declaring("uses:\n  - acme/kit\n  - acme/gone\n"),
    )]);
    let error = host
        .parse("research.md")
        .expect_err("acme/gone is not installed");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error:?}");
    assert_eq!(
        error.message,
        "the environment cannot satisfy this prompt:\n\
         - gone is unavailable: acme/gone is not installed"
    );
}

#[test]
fn a_uses_entry_naming_a_pack_whose_factory_refuses_parses_since_parsing_makes_no_pack() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/broken\n"))]);
    let prompt = host
        .parse("research.md")
        .expect("only a run calls acme/broken's factory");
    assert_eq!(prompt.file(), "research.md");
}

#[test]
fn a_uses_entry_naming_a_pack_inserted_unavailable_is_refused_with_its_reason() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/offline\n"))]);
    let error = host
        .parse("research.md")
        .expect_err("acme/offline was inserted unavailable");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error:?}");
    assert_eq!(
        error.message,
        "the environment cannot satisfy this prompt:\n\
         - offline is unavailable: offline takes no configuration"
    );
}

#[test]
fn a_prompt_parses_with_every_prompt_it_links_when_their_packs_are_installed() {
    let host = host(&[
        (
            "research.md",
            declaring("uses:\n  - acme/kit\nprompts: [review.md]\n"),
        ),
        (
            "review.md",
            declaring("uses:\n  - acme/broken\nprompts: [research.md]\n"),
        ),
    ]);
    let prompt = host
        .parse("research.md")
        .expect("every Pack the two prompts declare is installed");
    let linked: Vec<&str> = prompt.linked().map(|linked| linked.file()).collect();
    assert_eq!(linked, ["review.md"]);
}

#[test]
fn a_linked_prompts_uses_entry_the_host_cannot_serve_is_refused_naming_that_prompt() {
    let host = host(&[
        (
            "research.md",
            declaring("uses:\n  - acme/gone\nprompts: [review.md]\n"),
        ),
        (
            "review.md",
            declaring("uses:\n  - acme/kit\n  - acme/offline\n  - acme/gone\n  - web\n"),
        ),
    ]);
    let error = host
        .parse("research.md")
        .expect_err("the linked prompt requires Packs this Host cannot serve");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error:?}");
    assert_eq!(
        error.message,
        "the environment cannot satisfy this prompt:\n\
         - gone is unavailable: acme/gone is not installed\n\
         - review.md: offline is unavailable: offline takes no configuration\n\
         - review.md: web is unavailable: runvil/web is not installed",
        "the parsed prompt's own entry has no prompt name, and the entry both declare is \
         listed once"
    );
}

#[test]
fn a_linked_prompt_the_finder_lacks_fails_the_parse_where_it_is_listed() {
    let host = host(&[("research.md", declaring("prompts: [absent.md]\n"))]);
    let error = host
        .parse("research.md")
        .expect_err("the finder has no absent.md");
    let mut expected = missing_prompt("absent.md");
    expected.at = Some(SourceLoc::line(
        "research.md",
        NonZeroU32::new(5).expect("5 is non-zero"),
    ));
    assert_eq!(
        error, expected,
        "the finder's failure, placed on the `prompts:` line that lists the name"
    );
}

#[test]
fn a_finder_failure_comes_back_as_given() {
    let host = host(&[]);
    let error = host
        .parse("absent.md")
        .expect_err("the finder has no such prompt");
    assert_eq!(error, missing_prompt("absent.md"));
}

#[test]
fn a_parse_failure_comes_back_as_given_before_any_uses_check() {
    let source = declaring("uses:\n  - acme/gone\n").replace("## Only", "##");
    let expected = Prompt::parse(&|_: &str| Ok(source.clone()), "research.md")
        .expect_err("a prompt with an empty heading fails to parse");
    let host = host(&[("research.md", source)]);
    let error = host
        .parse("research.md")
        .expect_err("the parse fails first");
    assert_eq!(error, expected);
}

#[test]
fn a_host_built_with_no_plugin_has_the_two_builtin_packs_and_an_empty_list_of_reports() {
    let host = builder().build();
    assert_eq!(fqns(&host), ["runvil/subagents", "runvil/user"]);
    assert!(
        host.plugin_reports().is_empty(),
        "no step named a Plugin: {:?}",
        host.plugin_reports()
    );
}

#[test]
fn rust_plugins_run_in_the_order_written_and_the_first_keeps_a_pack_id() {
    let host = builder()
        .with_rust_plugin(|packs| packs.insert(PackInfo::new("acme/web"), builds))
        .with_rust_plugin(|packs| packs.insert(PackInfo::new("runvil/web"), builds))
        .build();
    assert_eq!(
        fqns(&host),
        ["acme/web", "runvil/subagents", "runvil/user"],
        "the Plugin written first inserted `web` first"
    );
    let [first, second] = host.plugin_reports() else {
        panic!("one report for each Plugin: {:?}", host.plugin_reports());
    };
    assert!(
        first.plugins().is_empty() && first.failures().is_empty(),
        "the first Plugin's function returned `Ok`: {first:?}"
    );
    assert!(second.plugins().is_empty(), "{second:?}");
    let [refusal] = second.failures() else {
        panic!("the registry's refusal is the second report's one failure: {second:?}");
    };
    assert_eq!(
        refusal.facts.get(&ErrorFact::Reason),
        Some(&Value::from("pack_id_taken"))
    );
    assert_eq!(
        refusal.message,
        "Pack `runvil/web` cannot take the Pack id web, which `acme/web` holds"
    );
}

#[test]
fn a_rust_plugin_that_fails_is_the_failure_of_its_report_and_its_earlier_packs_stay() {
    let failure = make_tool_error(ErrorCode::ToolOther, "the kit's second Pack has no backend");
    let returned = failure.clone();
    let host = builder()
        .with_rust_plugin(move |packs| {
            packs.insert(PackInfo::new("acme/kit"), builds)?;
            Err(returned)
        })
        .build();
    assert_eq!(
        fqns(&host),
        ["acme/kit", "runvil/subagents", "runvil/user"],
        "the Pack the function inserted before it failed stays"
    );
    let [report] = host.plugin_reports() else {
        panic!("one report for the one Plugin: {:?}", host.plugin_reports());
    };
    assert!(
        report.plugins().is_empty(),
        "a Rust Plugin leaves no Plugin in its report: {report:?}"
    );
    assert_eq!(
        report.failures(),
        [failure],
        "the function's error, as it returned it"
    );
}

#[test]
fn a_pack_inserted_as_unavailable_is_among_the_hosts_packs() {
    let host = host(&[]);
    assert_eq!(
        fqns(&host),
        [
            "acme/broken",
            "acme/kit",
            "acme/offline",
            "runvil/subagents",
            "runvil/user",
        ],
        "every Pack in the order of their names: `acme/offline`, which was inserted \
         unavailable, and the two built-ins among them"
    );
}
