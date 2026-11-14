//! The executor's `VfsRef` contract with its caller: the handle is the
//! files the run's tool calls work in and no part of the run's store. A
//! run's sections share the run's own store, no file of the handle is in
//! that store, no file of that store is in the handle, and the handle's
//! policy and operation observer never see a store operation.
//! Then the run-bounded scope: a run's `Done`, or its drop before `Done`,
//! ends its scope however long the caller holds the store views it was
//! handed.

use std::sync::{Arc, Mutex};

use crate::Error;
use crate::execute::run::{Effect, IssuedEffect, Run, Step};
use crate::execute::tests::context::{parse, test_params};
use crate::test_support::perform_locally;
use crate::test_support::scenario::{Scenario, fresh_store_view};
use runvil_prompt::test_support::{one_section, prompt};
use runvil_vfs::{MemoryBackend, Op, Origin, Policy, Verdict, VfsError, VfsPath, VfsRef};

#[test]
fn one_sections_store_write_is_the_next_sections_read() {
    // The store survives the context-clearing section transition: every
    // section of a run works in the run's one store.
    let source = prompt(
        "",
        &[
            ("First", "store.write('handoff.txt', 'across the reset')"),
            ("Second", "return store.read('handoff.txt')"),
        ],
    );
    let trace = Scenario::new(source).run();
    assert_eq!(trace.text(), "across the reset");
    // The read after the run takes a fresh view: the run's identities
    // ended with it, so nothing the run touched can conflict here.
    assert_eq!(
        trace
            .read("handoff.txt")
            .expect("the run's write stays in its store"),
        "across the reset"
    );
}

#[test]
fn a_runs_store_holds_no_file_of_its_handle_and_its_handle_no_file_of_its_store()
-> Result<(), VfsError> {
    // The handle holds `/notes.md` when the run starts. The access that
    // wrote it drops at once, so its scope ends before the run's starts.
    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("the caller's files"))?
        .write("/notes.md", b"the caller's notes")?;
    let source = one_section(
        "
        local seen = store.exists('notes.md')
        store.write('notes.md', 'the run wrote this')
        store.write('report.md', 'the run wrote this too')
        return tostring(seen)",
    );
    let trace = Scenario::new(source).vfs(vfs.clone()).run();
    assert_eq!(
        trace.text(),
        "false",
        "the run's store holds no file of the handle"
    );
    assert_eq!(
        trace.glob("**")?,
        ["notes.md", "report.md"],
        "both writes landed in the run's store"
    );
    let files = vfs.acquire(Origin::new("after the run"))?;
    assert_eq!(
        files.glob("/**")?,
        ["/notes.md"],
        "the handle holds no file the run's store took"
    );
    assert_eq!(
        files.read("/notes.md")?,
        b"the caller's notes",
        "the store's write of the same name left the handle's file as it was"
    );
    Ok(())
}

/// Each operation a handle's policy was asked about and each event its
/// observer received, in order.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Seen {
    fn note(&self, what: String) {
        self.0
            .lock()
            .expect("the record is not poisoned")
            .push(what);
    }

    fn so_far(&self) -> Vec<String> {
        self.0.lock().expect("the record is not poisoned").clone()
    }
}

/// A policy that denies every operation and notes each one it is asked
/// about.
struct DenyAll(Seen);

impl Policy for DenyAll {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        self.0.note(format!("policy: {op:?} {path}"));
        Verdict::Deny("this handle denies every operation".to_owned())
    }
}

#[test]
fn a_handles_policy_and_observer_never_see_a_store_operation() -> Result<(), VfsError> {
    let seen = Seen::default();
    let observer = seen.clone();
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .policy(DenyAll(seen.clone()))
        .on_op(move |event| observer.note(format!("observer: {:?} {}", event.op(), event.path())))
        .build();
    let source = one_section(
        "
        store.write('kept.txt', 'from')
        store.append('kept.txt', ' the run')
        store.str_replace('kept.txt', 'from', 'by')
        local listed = store.glob('*.txt')
        local found = store.exists('kept.txt')
        local text = store.read('kept.txt')
        store.delete('kept.txt')
        return #listed .. '|' .. tostring(found) .. '|' .. text",
    );
    let trace = Scenario::new(source).vfs(vfs.clone()).run();
    assert_eq!(
        trace.text(),
        "1|true|by the run",
        "every store operation ran over a handle whose policy denies every operation"
    );
    assert_eq!(
        seen.so_far(),
        Vec::<String>::new(),
        "neither the handle's policy nor its observer saw a store operation"
    );
    // The policy is live: it is asked about a write through the handle
    // and denies it, so the observer receives nothing.
    let files = vfs.acquire(Origin::new("the handle's own files"))?;
    assert!(
        matches!(
            files.write("/kept.txt", b"from"),
            Err(VfsError::PermissionDenied { .. })
        ),
        "the handle's policy denies a write to the handle's files"
    );
    assert_eq!(seen.so_far(), ["policy: Write /kept.txt"]);
    Ok(())
}

const HELD_STORE: &str = "---\nname: held-store\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n\
    ```lua\n\
    store.write('kept.txt', 'from the run')\n\
    return store.read('kept.txt')\n\
    ```\n";

/// Builds the `HELD_STORE` run.
fn held_store_run() -> Run {
    Run::new(Arc::new(parse(HELD_STORE)), "{}", test_params())
}

/// Answers `effects` through the local performer, keeping each one in
/// `held` so the store views they hold outlive the answer.
fn answer_holding(run: &mut Run, effects: Vec<IssuedEffect>, held: &mut Vec<Effect>) {
    assert!(!effects.is_empty(), "every issued effect is answered");
    for IssuedEffect { id, effect, .. } in effects {
        let answer = perform_locally(&effect, &mut |effect| {
            panic!("the fixture issues no model round: {effect:?}")
        });
        run.resume(id, answer);
        held.push(effect);
    }
}

/// Drives the `HELD_STORE` run to `Done`, keeping every effect, and
/// returns the run itself too, so both the run and every store view it
/// handed out are still alive when the caller asserts.
fn drive_holding_effects() -> (Run, Result<String, Error>, Vec<Effect>) {
    let mut run = held_store_run();
    let mut held = Vec::new();
    loop {
        match run.step() {
            Step::Done { result, .. } => return (run, result, held),
            Step::Pending { effects, .. } => answer_holding(&mut run, effects, &mut held),
        }
    }
}

#[test]
fn a_run_ends_its_scope_at_done_while_the_caller_still_holds_its_store_views()
-> Result<(), VfsError> {
    let (run, result, held) = drive_holding_effects();
    assert!(
        matches!(&result, Ok(text) if text == "from the run"),
        "the run succeeds: {result:?}"
    );
    assert!(
        held.iter()
            .any(|effect| matches!(effect, Effect::Vfs { .. })),
        "the test driver holds the run's store views past Done: {held:?}"
    );
    // A fresh scope reads the run's write without a conflict: the run's
    // scope ended at Done, not when the run or the test driver's views dropped.
    let fresh = fresh_store_view(&run.store_for_test(), "after done")?;
    assert_eq!(fresh.read("kept.txt")?, b"from the run");
    drop(held);
    drop(run);
    Ok(())
}

#[test]
fn an_operation_through_a_store_view_held_past_done_is_refused_and_changes_nothing()
-> Result<(), VfsError> {
    let (run, _, held) = drive_holding_effects();
    let access = match held.first() {
        Some(Effect::Vfs { access, .. }) => access,
        other => panic!("the run's first effect is its store write: {other:?}"),
    };
    match access.write("kept.txt", b"after the run") {
        Err(VfsError::ScopeEnded { path }) => {
            assert_eq!(path, "kept.txt", "the refusal names the logical path");
        }
        other => panic!("a write through a view held past Done is refused, got {other:?}"),
    }
    let fresh = fresh_store_view(&run.store_for_test(), "after done")?;
    assert_eq!(fresh.read("kept.txt")?, b"from the run");
    drop(held);
    drop(run);
    Ok(())
}

#[test]
fn dropping_a_run_before_done_ends_its_scope_while_the_caller_still_holds_its_store_views()
-> Result<(), VfsError> {
    let mut run = held_store_run();
    let store = run.store_for_test();
    let mut held = Vec::new();
    let effects = match run.step() {
        Step::Pending { effects, .. } => effects,
        other @ Step::Done { .. } => panic!("the run parks on its store write: {other:?}"),
    };
    answer_holding(&mut run, effects, &mut held);
    let effects = match run.step() {
        Step::Pending { effects, .. } => effects,
        other @ Step::Done { .. } => panic!("the run parks on its store read: {other:?}"),
    };
    held.extend(effects.into_iter().map(|issued| issued.effect));
    drop(run);
    // A fresh scope reads the dropped run's write without a conflict,
    // though the test driver still holds every store view the run handed out.
    let fresh = fresh_store_view(&store, "after drop")?;
    assert_eq!(fresh.read("kept.txt")?, b"from the run");
    drop(held);
    Ok(())
}
