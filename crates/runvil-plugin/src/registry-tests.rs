//! Tests for the Pack registry: a new registry is empty and lists every
//! Pack in FQN order, `insert` refuses an FQN that is not a vendor/name
//! pair, an FQN the registry holds already, a reserved Pack id from any
//! vendor, the name and the Pack id of each built-in Pack, and a Pack id
//! the registry holds under another FQN; `insert_unavailable` refuses
//! what `insert` refuses, keeps its reason, and holds its Pack id; the
//! registry hands each Pack's factory on with the Pack id, and its Debug
//! output shows each FQN and Pack id.

use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use runvil_types::tools::{ToolInfo, ToolOutput, make_tool_error};
use serde_json::Value;

use super::PackRegistry;
use crate::{BoxFuture, Pack, PackId, PackInfo, ToolContext};

/// Asserts `result` refuses the insert of `fqn` for `reason`, with its
/// message as its prose, and returns the message.
#[track_caller]
fn refused(result: Result<(), Error>, reason: &str, fqn: &str) -> String {
    let error = result.expect_err("the insert must be refused");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from(reason)),
        "{error:?}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from(fqn)),
        "{error:?}"
    );
    assert_eq!(error.prose, error.message, "{error:?}");
    error.message
}

/// The FQN of the built-in that reserves the subagents name.
const SUBAGENTS: &str = "runvil/subagents";

/// The FQN of the built-in that marks a prompt that needs a human.
const USER: &str = "runvil/user";

/// A Pack with the FQN `fqn`, inserted with the [`idle`] factory.
const fn fixture(fqn: &'static str) -> PackInfo {
    PackInfo::new(fqn)
}

/// A Pack that offers no tool and answers no call.
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

/// The factory of [`Idle`], which a test inserts a Pack with when it
/// never calls the Pack.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn idle(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Idle))
}

fn name(text: &str) -> PackId {
    PackId::parse(text).unwrap()
}

#[test]
fn a_new_registry_is_empty_and_lists_every_pack_in_fqn_order() {
    let mut packs = PackRegistry::new();
    assert_eq!(packs.list().count(), 0, "a new registry holds no Pack");
    packs.insert(fixture("tests/zeta"), idle).unwrap();
    packs.insert(fixture("acme/alpha"), idle).unwrap();
    let listed: Vec<&str> = packs.list().map(|info| info.fqn).collect();
    assert_eq!(listed, ["acme/alpha", "tests/zeta"]);
}

#[test]
fn add_refuses_an_fqn_that_is_not_a_vendor_name_pair() {
    for bad in [
        "web",
        "acme/web/extra",
        "/web",
        "acme/",
        "Acme/web",
        "acme/Web",
    ] {
        refused(
            PackRegistry::new().insert(fixture(bad), idle),
            "invalid_fqn",
            bad,
        );
    }
}

#[test]
fn add_refuses_an_fqn_the_registry_holds_already() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("tests/tools"), idle).unwrap();
    refused(
        packs.insert(fixture("tests/tools"), idle),
        "duplicate_pack",
        "tests/tools",
    );
}

#[test]
fn add_refuses_a_pack_taking_a_builtins_pack_id() {
    let mut packs = PackRegistry::new();
    for (fqn, id, builtin) in [
        ("acme/subagents", "subagents", SUBAGENTS),
        ("acme/user", "user", USER),
        (SUBAGENTS, "subagents", SUBAGENTS),
        (USER, "user", USER),
    ] {
        let message = refused(packs.insert(fixture(fqn), idle), "pack_id_taken", fqn);
        assert_eq!(
            message,
            format!(
                "Pack `{fqn}` cannot take the Pack id {id}, which the built-in Pack `{builtin}` \
                 holds"
            )
        );
        assert!(
            !packs.list().any(|info| info.fqn == fqn),
            "the refused {fqn} is not in the registry"
        );
    }
}

#[test]
fn add_refuses_a_pack_taking_a_reserved_pack_id_from_any_vendor() {
    for (fqn, reserved) in [
        ("acme/adhoc", "adhoc"),
        ("runvil/adhoc", "adhoc"),
        ("acme/dynamic", "dynamic"),
        ("runvil/dynamic", "dynamic"),
    ] {
        let mut packs = PackRegistry::new();
        let message = refused(packs.insert(fixture(fqn), idle), "reserved_pack_id", fqn);
        assert!(
            message.contains(&format!("the Pack id {reserved},")),
            "{message}"
        );
        assert!(
            !packs.list().any(|info| info.fqn == fqn),
            "the refused {fqn} is not in the registry"
        );
    }
}

#[test]
fn add_refuses_a_pack_whose_id_another_vendor_holds_naming_the_holder() {
    for (first, second) in [("runvil/web", "acme/web"), ("acme/web", "runvil/web")] {
        let mut packs = PackRegistry::new();
        packs.insert(fixture(first), idle).unwrap();
        let message = refused(packs.insert(fixture(second), idle), "pack_id_taken", second);
        assert_eq!(
            message,
            format!("Pack `{second}` cannot take the Pack id web, which `{first}` holds")
        );
        let ids: Vec<(&'static str, String)> = packs
            .into_entries()
            .map(|(info, id, _factory)| (info.fqn, id.to_string()))
            .filter(|(_, id)| id == "web")
            .collect();
        assert_eq!(
            ids,
            [(first, "web".to_owned())],
            "the first Pack inserted keeps the Pack id, its FQN's second segment"
        );
    }
}

#[test]
fn a_pack_inserted_unavailable_holds_its_pack_id() {
    let reason = || make_tool_error(ErrorCode::ToolOther, "kit takes no configuration");
    let mut packs = PackRegistry::new();
    packs
        .insert_unavailable(fixture("acme/kit"), reason())
        .unwrap();
    let message = refused(
        packs.insert(fixture("tests/kit"), idle),
        "pack_id_taken",
        "tests/kit",
    );
    assert_eq!(
        message,
        "Pack `tests/kit` cannot take the Pack id kit, which `acme/kit` holds"
    );
    packs.insert(fixture("tests/web"), idle).unwrap();
    refused(
        packs.insert_unavailable(fixture("acme/web"), reason()),
        "pack_id_taken",
        "acme/web",
    );
}

#[test]
fn the_registry_hands_each_packs_factory_on_with_its_pack_id() {
    let depth = 2;
    let mut packs = PackRegistry::new();
    packs
        .insert(
            fixture("acme/kit"),
            move |id: &PackId| -> Result<Arc<dyn Pack>, Error> {
                Err(make_tool_error(
                    ErrorCode::ToolOther,
                    format!("{id} refuses at depth {depth}"),
                ))
            },
        )
        .unwrap();
    packs.insert(fixture("tests/idle"), idle).unwrap();
    let made: Vec<(&'static str, PackId, Option<String>)> = packs
        .into_entries()
        .map(|(info, id, factory)| {
            let made = factory.unwrap().new_pack(&id);
            (info.fqn, id, made.err().map(|error| error.to_string()))
        })
        .collect();
    assert_eq!(
        made,
        [
            (
                "acme/kit",
                name("kit"),
                Some("kit refuses at depth 2".to_owned())
            ),
            ("tests/idle", name("idle"), None),
        ],
        "each factory is the one inserted, and makes its Pack under the Pack id handed on"
    );
}

#[test]
fn the_registrys_debug_output_shows_each_fqn_and_pack_id() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("acme/mcp"), idle).unwrap();
    let shown = format!("{packs:?}");
    assert!(shown.contains("acme/mcp"), "the FQN is shown: {shown}");
    let id = format!("{:?}", name("mcp"));
    assert!(shown.contains(&id), "{id} is shown: {shown}");
}

#[test]
fn the_registry_refusals_name_the_pack() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("acme/web"), idle).unwrap();
    let mut insert = |fqn| packs.insert(fixture(fqn), idle);
    let messages = [
        refused(insert("web"), "invalid_fqn", "web"),
        refused(insert("acme/web"), "duplicate_pack", "acme/web"),
        refused(insert("acme/subagents"), "pack_id_taken", "acme/subagents"),
        refused(insert("acme/user"), "pack_id_taken", "acme/user"),
        refused(insert("tests/web"), "pack_id_taken", "tests/web"),
        refused(insert("acme/adhoc"), "reserved_pack_id", "acme/adhoc"),
        refused(insert("acme/dynamic"), "reserved_pack_id", "acme/dynamic"),
    ];
    assert_eq!(
        messages,
        [
            "Pack fully qualified name `web` is not a vendor/name pair",
            "Pack `acme/web` is already in the registry",
            "Pack `acme/subagents` cannot take the Pack id subagents, \
             which the built-in Pack `runvil/subagents` holds",
            "Pack `acme/user` cannot take the Pack id user, \
             which the built-in Pack `runvil/user` holds",
            "Pack `tests/web` cannot take the Pack id web, which `acme/web` holds",
            "Pack `acme/adhoc` cannot take the Pack id adhoc, which the Engine reserves",
            "Pack `acme/dynamic` cannot take the Pack id dynamic, which the Engine reserves",
        ]
    );
}

#[test]
fn insert_unavailable_refuses_a_bad_fqn_and_a_duplicate_and_hands_on_its_reason() {
    let reason = || make_tool_error(ErrorCode::ToolOther, "kit takes no configuration");
    let mut packs = PackRegistry::new();
    refused(
        packs.insert_unavailable(PackInfo::new("kit"), reason()),
        "invalid_fqn",
        "kit",
    );
    packs
        .insert_unavailable(PackInfo::new("acme/kit"), reason())
        .unwrap();
    refused(
        packs.insert_unavailable(PackInfo::new("acme/kit"), reason()),
        "duplicate_pack",
        "acme/kit",
    );
    for builtin in [SUBAGENTS, USER] {
        refused(
            packs.insert_unavailable(PackInfo::new(builtin), reason()),
            "pack_id_taken",
            builtin,
        );
    }
    refused(
        packs.insert(PackInfo::new("acme/kit"), idle),
        "duplicate_pack",
        "acme/kit",
    );
    let entries: Vec<(&str, Option<String>)> = packs
        .into_entries()
        .map(|(info, _id, factory)| (info.fqn, factory.err().map(|error| error.to_string())))
        .collect();
    assert_eq!(
        entries,
        [("acme/kit", Some("kit takes no configuration".to_owned()))],
        "the Pack inserted unavailable keeps its reason"
    );
}
