//! The pick of each run's Packs: every installed Pack is in every run,
//! whatever its vendor and whatever the prompt declares, the declared
//! ones first; a bare entry means the runvil Pack of its id; an entry the
//! pick cannot resolve is on the roster with its reason or, with none,
//! left off it, and keeps the Pack another vendor installed under its
//! Pack id out of the run; the Host refuses at parse only a declared Pack
//! not installed or inserted unavailable; and the ready wait covers every
//! Pack the run holds, keying each failure by FQN.

use super::*;
use crate::packs::wait_ready;

/// A Host holding `runvil/web` and `acme/poop`, each offering one tool:
/// `web/fetch` and `poop/scoop`. `acme/poop`'s `ready` answers as
/// `poop_ready` says.
fn vendors(poop_ready: &str) -> Packs {
    host(&[
        (info("runvil/web"), offering(&["web/fetch"])),
        (
            info("acme/poop"),
            json!({ "tools": [tool("poop/scoop")], "ready": poop_ready }),
        ),
    ])
}

#[test]
fn a_run_that_declares_nothing_holds_every_installed_pack_whatever_its_vendor() {
    let host = vendors("ready");
    assert_eq!(
        catalog_ids(&host, &prompt("")),
        ["poop/scoop", "web/fetch"],
        "the catalog holds the tools of acme/poop, which no entry names"
    );
    assert_eq!(usable(&host, &prompt("")), ["poop", "subagents", "web"]);
}

#[test]
fn a_bare_entry_runs_with_the_runvil_pack_of_that_id() {
    let host = vendors("ready");
    let declares = prompt("uses:\n  - web\n");
    assert_eq!(catalog_ids(&host, &declares), ["poop/scoop", "web/fetch"]);
    let run = begin(&host, BTreeMap::new(), &declares);
    assert_eq!(run.roster().get(&name("web")), Some(&usable_from("runvil")));
}

#[test]
fn the_pick_is_every_installed_pack_with_the_declared_ones_first() {
    let host = vendors("ready");
    for (uses, fqns) in [
        (
            "",
            ["acme/poop", "runvil/subagents", "runvil/user", "runvil/web"],
        ),
        (
            "uses:\n  - acme/poop\n",
            ["acme/poop", "runvil/subagents", "runvil/user", "runvil/web"],
        ),
        (
            "uses:\n  - web\n",
            ["runvil/web", "acme/poop", "runvil/subagents", "runvil/user"],
        ),
        (
            "uses:\n  - user\n  - web\n  - acme/poop\n",
            ["runvil/user", "runvil/web", "acme/poop", "runvil/subagents"],
        ),
    ] {
        let picked = host.pick(&prompt(uses));
        assert_eq!(
            picked.fqns, fqns,
            "the declared Packs in declaration order, then every other installed Pack once: \
             {uses:?}"
        );
        assert!(picked.unresolved.is_empty(), "{:?}", picked.unresolved);
    }
}

#[test]
fn a_runs_roster_and_catalog_are_the_same_whatever_its_prompt_declares() {
    let host = vendors("ready");
    let plain = begin(&host, BTreeMap::new(), &prompt(""));
    for uses in [
        "uses:\n  - web\n",
        "uses:\n  - acme/poop\n",
        "uses:\n  - acme/poop\n  - web\n  - subagents\n",
    ] {
        let run = begin(&host, BTreeMap::new(), &prompt(uses));
        assert_eq!(run.roster(), plain.roster(), "{uses:?}");
        assert_eq!(snapshot_ids(&run), snapshot_ids(&plain), "{uses:?}");
    }
    assert_eq!(
        plain.roster(),
        BTreeMap::from([
            (name("poop"), usable_from("acme")),
            (name("subagents"), usable_from("runvil")),
            (name("user"), unavailable_for("this run has no user")),
            (name("web"), usable_from("runvil")),
        ])
    );
}

#[test]
fn a_vendor_name_entry_the_host_holds_under_another_vendor_is_unavailable_and_keeps_that_pack_out()
{
    let host = vendors("ready");
    let declares = prompt("uses:\n  - acme/web\n");
    let picked = host.pick(&declares);
    assert_eq!(
        picked.fqns,
        ["acme/poop", "runvil/subagents", "runvil/user"],
        "runvil/web holds the Pack id the unresolved entry names, so it stays out"
    );
    let run = begin(&host, BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("web")),
        Some(&unavailable_for("acme/web is not installed"))
    );
    assert_eq!(
        snapshot_ids(&run),
        ["poop/scoop"],
        "the catalog holds no tool of a Pack id the roster calls unavailable"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - web is unavailable: acme/web is not installed (at prompt:5)"
    );
}

#[test]
fn a_bare_entry_that_only_another_vendor_has_is_unavailable_naming_its_fqn() {
    let host = vendors("ready");
    let declares = prompt("uses:\n  - poop\n");
    let run = begin(&host, BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("poop")),
        Some(&unavailable_for(
            "poop means runvil/poop, which is not installed; write acme/poop to use that Pack"
        ))
    );
    assert_eq!(
        snapshot_ids(&run),
        ["web/fetch"],
        "acme/poop holds the Pack id the unresolved entry names, so it stays out"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - poop is unavailable: poop means runvil/poop, which is not installed; \
         write acme/poop to use that Pack (at prompt:5)",
        "the run names the reason and does not call the Pack missing"
    );
}

#[test]
fn an_entry_no_vendor_has_is_missing_and_an_absent_vendor_name_is_unavailable() {
    let host = vendors("ready");
    let declares = prompt("uses:\n  - nope\n  - acme/gone\n");
    let run = begin(&host, BTreeMap::new(), &declares);
    let roster = run.roster();
    assert_eq!(
        roster.get(&name("nope")),
        None,
        "no reason, so off the roster"
    );
    assert_eq!(
        roster.get(&name("gone")),
        Some(&unavailable_for("acme/gone is not installed"))
    );
    assert_eq!(
        snapshot_ids(&run),
        ["poop/scoop", "web/fetch"],
        "an entry that names no installed Pack's id keeps no Pack out"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: nope\n\
         - gone is unavailable: acme/gone is not installed (at prompt:5)"
    );
}

#[test]
fn the_roster_covers_every_declared_pack_the_run_cannot_serve() {
    let host = host_with(
        &HostServices::new(),
        &[
            (PackInfo::new("acme/broken"), backed, Value::Null),
            (info("acme/fine"), listed, Value::Null),
        ],
    );
    let declares = prompt("uses:\n  - absent\n  - acme/fine\n  - acme/broken\n");
    let run = begin(&host, BTreeMap::new(), &declares);
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (
                name("broken"),
                unavailable_for("backed needs tests/backend, and this host provides none"),
            ),
            (name("fine"), usable_from("acme")),
            (name("subagents"), usable_from("runvil")),
            (name("user"), unavailable_for("this run has no user")),
        ]),
        "the absent Pack, which no vendor has, stays off the roster"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: absent\n\
         - broken is unavailable: backed needs tests/backend, and this host provides none \
         (at prompt:5)"
    );
}

#[test]
fn refusals_name_each_declared_pack_not_installed_or_inserted_unavailable() {
    let mut registry = PackRegistry::new();
    for (fqn, make) in [
        ("runvil/web", listed as Make),
        ("acme/poop", listed),
        ("acme/broken", backed),
    ] {
        registry
            .insert(info(fqn), factory(make, Value::Null, HostServices::new()))
            .unwrap();
    }
    registry
        .insert_unavailable(
            info("acme/offline"),
            make_tool_error(ErrorCode::ToolOther, "offline takes no configuration"),
        )
        .unwrap();
    let host = Packs::new(registry);
    let declares = prompt(
        "uses:\n  - web\n  - nope\n  - acme/gone\n  - poop\n  - acme/broken\n  - acme/offline\n",
    );
    assert_eq!(
        host.refusals(&declares),
        [
            (name("nope"), "runvil/nope is not installed".to_owned()),
            (name("gone"), "acme/gone is not installed".to_owned()),
            (
                name("poop"),
                "poop means runvil/poop, which is not installed; write acme/poop to use that Pack"
                    .to_owned(),
            ),
            (name("offline"), "offline takes no configuration".to_owned()),
        ],
        "a bare id means runvil/<id>, the installed runvil/web is no refusal, and neither is \
         acme/broken, whose factory only a run calls"
    );
}

#[test]
fn refusals_are_empty_when_every_declared_pack_is_installed() {
    let host = vendors("fails");
    assert!(
        host.refusals(&prompt("uses:\n  - web\n  - acme/poop\n"))
            .is_empty(),
        "a Pack whose ready fails is still installed"
    );
    assert!(host.refusals(&prompt("")).is_empty());
}

/// Whether the ready wait over the Packs a run of `prompt` holds on
/// `host` resolves without waiting, and its failures when it does.
fn waits(host: &Packs, prompt: &Prompt) -> Option<BTreeMap<&'static str, String>> {
    let picked = host.pick(prompt);
    let made = host.make(&picked, false);
    wait_ready(&made, &CancelHandle::new()).now_or_never()
}

#[test]
fn the_ready_wait_covers_every_pack_the_run_holds() {
    let host = vendors("pends");
    for uses in ["", "uses:\n  - web\n", "uses:\n  - acme/poop\n"] {
        assert!(
            waits(&host, &prompt(uses)).is_none(),
            "a run waits on acme/poop whether or not its prompt declares it: {uses:?}"
        );
    }
    assert_eq!(
        waits(&host, &prompt("uses:\n  - poop\n")),
        Some(BTreeMap::new()),
        "a run never waits on the Pack an unresolved entry keeps out of it"
    );
    assert_eq!(
        waits(&vendors("ready"), &prompt("")),
        Some(BTreeMap::new()),
        "a run whose Packs are all ready waits on none"
    );
}

/// The snapshot of a run of `prompt` on `host` after the ready wait,
/// beside the failures the wait found.
fn waited(host: &Packs, prompt: &Prompt) -> (BTreeMap<&'static str, String>, RunPacks) {
    let picked = host.pick(prompt);
    let made = host.make(&picked, false);
    let failures = wait_ready(&made, &CancelHandle::new())
        .now_or_never()
        .unwrap();
    let run = RunPacks::snapshot(made, failures.clone(), &picked.unresolved);
    (failures, run)
}

#[test]
fn a_ready_failure_is_keyed_by_fqn_and_leaves_every_other_pack_serving() {
    let host = vendors("fails");
    for uses in ["", "uses:\n  - web\n", "uses:\n  - acme/poop\n"] {
        let (failures, run) = waited(&host, &prompt(uses));
        assert_eq!(
            failures,
            BTreeMap::from([("acme/poop", "poop will not be ready".to_owned())]),
            "{uses:?}"
        );
        assert_eq!(
            run.roster().get(&name("poop")),
            Some(&unavailable_for("poop will not be ready")),
            "{uses:?}"
        );
        assert_eq!(
            run.roster().get(&name("web")),
            Some(&usable_from("runvil")),
            "{uses:?}"
        );
        assert_eq!(
            snapshot_ids(&run),
            ["web/fetch"],
            "runvil/web still serves the run: {uses:?}"
        );
    }
}
