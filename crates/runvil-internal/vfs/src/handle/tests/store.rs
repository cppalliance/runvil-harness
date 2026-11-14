//! Tests for the store: a set of files in memory under a claims ledger of
//! its own, outside every handle, and the views that reach it under an
//! access's identity and scope.

use super::*;
use crate::{PathReason, Store};

/// A view of `store` under a fresh identity in a scope of its own.
fn fresh_view(store: &Store) -> Result<Access, VfsError> {
    store.view(&access(&VfsRef::default())?)
}

/// Asserts that `result` is the refusal of an ended scope or an ended
/// identity, naming `expected`.
#[track_caller]
fn refused<T: fmt::Debug>(result: Result<T, VfsError>, expected: &str) {
    match result {
        Err(VfsError::ScopeEnded { path }) => assert_eq!(path, expected),
        other => panic!("expected an ended refusal on {expected}, got {other:?}"),
    }
}

/// The error a view reports for a path that names nothing in its store.
fn absent(path: &str) -> VfsError {
    VfsError::NotFound {
        path: path.to_owned(),
    }
}

#[test]
fn a_new_store_is_empty_and_a_clone_of_it_is_the_same_store() -> Result<(), VfsError> {
    let access = access(&VfsRef::default())?;
    for store in [Store::new(), Store::default()] {
        let view = store.view(&access)?;
        assert_eq!(view.glob("**")?, Vec::<String>::new(), "no file yet");
        // A clone is the same store: each reads what the other wrote.
        let clone = store.clone();
        let clone_view = clone.view(&access)?;
        view.write("notes.md", b"draft")?;
        assert_eq!(clone_view.read("notes.md")?, b"draft");
        clone_view.write("more.md", b"more")?;
        assert_eq!(view.glob("**")?, ["more.md", "notes.md"]);
        // The clone holds the same claims: another scope's write through
        // it meets the claim made through the store.
        conflict_message(fresh_view(&clone)?.write("notes.md", b"other"));
    }
    Ok(())
}

#[test]
fn a_handle_file_is_invisible_to_a_store_view_and_a_store_file_to_the_handle()
-> Result<(), VfsError> {
    let files = TestFs::default();
    let access = access(&handle(files.clone()))?;
    let view = Store::new().view(&access)?;
    access.write("/notes.md", b"handle")?;
    // The view names the same path and finds nothing there.
    assert!(!view.exists("notes.md")?);
    assert_eq!(view.read("notes.md"), Err(absent("notes.md")));
    assert_eq!(view.glob("**")?, Vec::<String>::new());
    view.write("draft.md", b"store")?;
    assert_eq!(view.read("draft.md")?, b"store");
    // No path of the handle reaches the store's file, and the handle's
    // backend never holds it.
    assert!(!access.exists("/draft.md")?);
    assert_eq!(access.glob("/**")?, ["/notes.md"]);
    assert_eq!(files.files().into_keys().collect::<Vec<_>>(), ["/notes.md"]);
    // One name on both sides is two files.
    view.write("notes.md", b"store")?;
    assert_eq!(access.read("/notes.md")?, b"handle");
    assert_eq!(view.read("notes.md")?, b"store");
    Ok(())
}

#[test]
fn a_store_view_records_its_claims_in_the_stores_ledger_never_in_the_handles()
-> Result<(), VfsError> {
    let vfs = VfsRef::default();
    let store = Store::new();
    let first = access(&vfs)?;
    let second = access(&vfs)?;
    // Two live scopes write one path, one through the handle and one
    // through a view of the store, either way round: the two claims sit
    // in two ledgers and never meet.
    first.write("/notes.md", b"handle")?;
    store.view(&second)?.write("notes.md", b"store")?;
    store.view(&first)?.write("draft.md", b"store")?;
    second.write("/draft.md", b"handle")?;
    // Inside either ledger, the two scopes conflict.
    conflict_message(second.write("/notes.md", b"handle"));
    conflict_message(store.view(&first)?.write("notes.md", b"store"));
    Ok(())
}

#[test]
fn two_stores_viewed_under_one_identity_share_no_file() -> Result<(), VfsError> {
    let access = access(&VfsRef::default())?;
    let first = Store::new().view(&access)?;
    let second = Store::new().view(&access)?;
    first.write("notes.md", b"first")?;
    // The write in one store is absent from the other.
    assert!(!second.exists("notes.md")?);
    assert_eq!(second.read("notes.md"), Err(absent("notes.md")));
    assert_eq!(second.glob("**")?, Vec::<String>::new());
    // One name in both stores is two files.
    second.write("notes.md", b"second")?;
    assert_eq!(first.read("notes.md")?, b"first");
    assert_eq!(second.read("notes.md")?, b"second");
    Ok(())
}

#[test]
fn a_store_keeps_case_so_two_spellings_of_one_name_are_two_files() -> Result<(), VfsError> {
    let store = Store::new();
    let first = fresh_view(&store)?;
    let second = fresh_view(&store)?;
    // The two spellings are two files under two claims on every
    // platform, so two live scopes write one each.
    first.write("Draft.md", b"1")?;
    second.write("draft.md", b"2")?;
    assert_eq!(first.read("Draft.md")?, b"1");
    assert_eq!(second.read("draft.md")?, b"2");
    Ok(())
}

#[test]
fn two_unordered_identities_writing_one_path_conflict_in_one_store_and_not_across_two()
-> Result<(), VfsError> {
    // Two forks of one parent, neither joined: nothing orders the two.
    let (parent, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let left = scope.spawn(&parent, test_origin())?;
    let right = scope.spawn(&parent, test_origin())?;
    // Each writes the path in a store of its own: two stores share no
    // claim, so neither write meets the other's.
    let (first, second) = (Store::new(), Store::new());
    let left_view = first.view(&left)?;
    let right_view = second.view(&right)?;
    left_view.write("notes.md", b"left")?;
    right_view.write("notes.md", b"right")?;
    assert_eq!(left_view.read("notes.md")?, b"left");
    assert_eq!(right_view.read("notes.md")?, b"right");
    // In one store, the same two writes conflict.
    let shared = Store::new();
    shared.view(&left)?.write("notes.md", b"left")?;
    let message = conflict_message(shared.view(&right)?.write("notes.md", b"right"));
    assert!(message.contains("notes.md"), "names the path: {message}");
    Ok(())
}

#[test]
fn two_forks_writing_one_store_path_conflict_and_a_join_orders_the_first_before_its_parent()
-> Result<(), VfsError> {
    let store = Store::new();
    let (parent, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let first = scope.spawn(&parent, test_origin())?;
    let second = scope.spawn(&parent, test_origin())?;
    store.view(&first)?.write("notes.md", b"first")?;
    // Nothing orders the first fork's write before its sibling or its
    // parent.
    conflict_message(store.view(&second)?.write("notes.md", b"second"));
    let parent_view = store.view(&parent)?;
    conflict_message(parent_view.write("notes.md", b"parent"));
    // The join orders everything the first fork did before the parent's
    // next step.
    scope.join(parent.id(), first.id());
    parent_view.write("notes.md", b"parent")?;
    assert_eq!(parent_view.read("notes.md")?, b"parent");
    Ok(())
}

#[test]
fn a_fork_views_the_same_store_and_its_work_follows_its_parents_earlier_work()
-> Result<(), VfsError> {
    let store = Store::new();
    let (parent, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let parent_view = store.view(&parent)?;
    parent_view.write("notes.md", b"parent")?;
    let child = scope.spawn(&parent, test_origin())?;
    // The child's view reaches the same files, and the fork orders the
    // parent's earlier write before the child's first step.
    let child_view = store.view(&child)?;
    assert_eq!(child_view.read("notes.md")?, b"parent");
    child_view.append("notes.md", b" and child")?;
    assert_eq!(child_view.read("notes.md")?, b"parent and child");
    // What the parent writes after the fork is not ordered before the
    // child.
    parent_view.write("later.md", b"parent")?;
    let message = conflict_message(child_view.read("later.md"));
    assert!(message.contains("later.md"), "names the path: {message}");
    Ok(())
}

#[test]
fn an_ended_scope_refuses_every_store_operation_and_every_new_view() -> Result<(), VfsError> {
    let store = Store::new();
    let (access, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let view = store.view(&access)?;
    view.write("notes.md", b"kept")?;
    scope.end();
    refused(view.read("notes.md"), "notes.md");
    refused(view.read_string("notes.md"), "notes.md");
    refused(view.read_range("notes.md", 0, 4), "notes.md");
    refused(view.write("notes.md", b"late"), "notes.md");
    refused(view.append("notes.md", b"late"), "notes.md");
    refused(view.str_replace("notes.md", "kept", "late"), "notes.md");
    refused(view.remove("notes.md", false), "notes.md");
    refused(view.exists("notes.md"), "notes.md");
    refused(view.glob("*.md"), "*.md");
    refused(view.list("dir"), "dir");
    refused(view.stat("notes.md"), "notes.md");
    refused(view.mkdir("dir", true), "dir");
    refused(view.rename("notes.md", "moved.md"), "notes.md");
    refused(view.copy("notes.md", "copy.md"), "notes.md");
    // The store gives no view of an access whose scope has ended, and
    // the view it gave forks no identity. Neither names a path, so each
    // refusal names `/`.
    refused(store.view(&access), "/");
    refused(scope.spawn(&view, test_origin()), "/");
    // No refused operation reached the store.
    let other = fresh_view(&store)?;
    assert_eq!(other.glob("**")?, ["notes.md"]);
    assert_eq!(other.read("notes.md")?, b"kept");
    Ok(())
}

#[test]
fn an_ended_scopes_claims_on_a_store_stop_conflicting_at_once() -> Result<(), VfsError> {
    let store = Store::new();
    let (access, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let view = store.view(&access)?;
    view.write("notes.md", b"first")?;
    // Two live scopes never order each other, so the second scope's write
    // conflicts.
    let other = fresh_view(&store)?;
    conflict_message(other.write("notes.md", b"next"));
    scope.end();
    // The ended scope's claim is ignored, though its accesses are still
    // held.
    other.write("notes.md", b"next")?;
    assert_eq!(other.read("notes.md")?, b"next");
    refused(view.read("notes.md"), "notes.md");
    Ok(())
}

#[test]
fn a_view_of_an_identity_whose_tool_call_ended_is_refused() -> Result<(), VfsError> {
    let store = Store::new();
    let (owner, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let call = scope.spawn(&owner, test_origin())?;
    let view = store.view(&call)?;
    view.write("a.md", b"call")?;
    scope.end_access(Some(owner.id()), call.id());
    // The view held across the end refuses its next operation and
    // spawn, and the store gives no new view of the ended identity.
    refused(view.read("a.md"), "a.md");
    refused(view.write("b.md", b"late"), "b.md");
    refused(scope.spawn(&view, test_origin()), "/");
    refused(store.view(&call), "/");
    // The scope stays open: the end joined the call's store write into
    // the owner, whose own view reads it.
    let owner_view = store.view(&owner)?;
    assert_eq!(owner_view.read("a.md")?, b"call");
    assert!(!owner_view.exists("b.md")?);
    Ok(())
}

#[test]
fn a_handles_policy_and_observer_never_see_a_store_operation() -> Result<(), VfsError> {
    let verdict = Arc::new(Mutex::new(Verdict::Deny("the handle is sealed".to_owned())));
    let events: Arc<Mutex<Vec<(Op, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    let vfs = VfsRef::builder()
        .mount("/", TestFs::default())
        .policy(FlipPolicy {
            verdict: Arc::clone(&verdict),
        })
        .on_op(move |event: OpEvent<'_>| {
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((event.op(), event.path().to_string()));
        })
        .build();
    let access = access(&vfs)?;
    let view = Store::new().view(&access)?;
    // The handle's policy denies the access every operation, and the
    // store's operations through a view of that access run all the same.
    assert!(matches!(
        access.write("/notes.md", b"handle"),
        Err(VfsError::PermissionDenied { .. })
    ));
    view.write("notes.md", b"store")?;
    view.append("notes.md", b"!")?;
    assert_eq!(view.read("notes.md")?, b"store!");
    assert_eq!(view.glob("*.md")?, ["notes.md"]);
    assert!(
        events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "a store operation fired the handle's observer"
    );
    // The observer records the handle's own admitted write, and nothing
    // of the store's operations around it.
    *verdict.lock().unwrap_or_else(PoisonError::into_inner) = Verdict::Allow;
    access.write("/notes.md", b"handle")?;
    assert!(view.remove("notes.md", false)?);
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        &[(Op::Write, "/notes.md".to_owned())]
    );
    Ok(())
}

#[test]
fn each_strict_path_rule_is_reported_in_check_order() -> Result<(), VfsError> {
    let view = fresh_view(&Store::new())?;
    let cases = [
        // One rule each, in the order the rules run.
        ("", PathReason::Empty),
        ("/absolute.txt", PathReason::Absolute),
        ("a\u{0}b.txt", PathReason::Control),
        ("a\u{7f}b.txt", PathReason::Control),
        ("a\\b.txt", PathReason::Backslash),
        ("a:b.txt", PathReason::ReservedName),
        ("C:", PathReason::ReservedName),
        ("a//b.txt", PathReason::EmptySegment),
        ("dir/", PathReason::EmptySegment),
        ("../escape.txt", PathReason::Traversal),
        ("a/./b.txt", PathReason::Traversal),
        ("trailing.", PathReason::UnsafeSuffix),
        ("trailing ", PathReason::UnsafeSuffix),
        ("CON", PathReason::ReservedName),
        ("dir/nul.txt", PathReason::ReservedName),
        ("com1", PathReason::ReservedName),
        ("LPT9.log", PathReason::ReservedName),
        ("COM0", PathReason::ReservedName),
        ("lpt0.txt", PathReason::ReservedName),
        ("COM\u{b9}", PathReason::ReservedName),
        ("dir/lpt\u{b2}.log", PathReason::ReservedName),
        ("LPT\u{b3}", PathReason::ReservedName),
        // Two rules each: the one that runs first is the one reported.
        ("/a\u{0}b.txt", PathReason::Absolute),
        ("a\u{0}\\b.txt", PathReason::Control),
        ("a\\b:c.txt", PathReason::Backslash),
        ("a:b//c.txt", PathReason::ReservedName),
        ("a//../b.txt", PathReason::EmptySegment),
        ("../CON", PathReason::Traversal),
        ("CON.", PathReason::UnsafeSuffix),
    ];
    for (path, reason) in cases {
        assert_eq!(
            view.read(path),
            Err(VfsError::InvalidPath {
                path: path.to_owned(),
                reason,
            }),
            "{path:?} is reported as supplied, with the first rule it breaks"
        );
    }
    Ok(())
}

#[test]
fn the_path_length_ceiling_is_1024_bytes() -> Result<(), VfsError> {
    let view = fresh_view(&Store::new())?;
    // A path at the exact ceiling is accepted; one byte over is refused
    // as too long, whatever other rule it breaks.
    let maximum = "a".repeat(1024);
    view.write(&maximum, b"at the ceiling")?;
    assert_eq!(view.read(&maximum)?, b"at the ceiling");
    for too_long in ["a".repeat(1025), format!("/{maximum}")] {
        let refusal = view.read(&too_long);
        assert_eq!(
            refusal,
            Err(VfsError::InvalidPath {
                path: too_long,
                reason: PathReason::TooLong,
            })
        );
    }
    // Names that merely contain a device name are allowed.
    view.write("console.txt", b"ok")?;
    view.write("com10.txt", b"ok")?;
    view.write("com\u{b9}0.txt", b"ok")?;
    view.write("lpt\u{2074}.txt", b"ok")?;
    Ok(())
}

#[test]
fn the_strict_path_rules_apply_to_glob_patterns() -> Result<(), VfsError> {
    let view = fresh_view(&Store::new())?;
    view.write("a.txt", b"")?;
    view.write("dir/sub/b.txt", b"")?;
    // A pattern is a store path, refused like any other when it is
    // absolute or holds a traversal segment, an empty segment, or a
    // colon.
    for (pattern, reason) in [
        ("/etc/*", PathReason::Absolute),
        ("../*.txt", PathReason::Traversal),
        ("a/./b", PathReason::Traversal),
        ("a//*.txt", PathReason::EmptySegment),
        ("C:*", PathReason::ReservedName),
    ] {
        assert_eq!(
            view.glob(pattern),
            Err(VfsError::InvalidPath {
                path: pattern.to_owned(),
                reason,
            }),
            "{pattern:?} is reported as supplied"
        );
    }
    // Well-formed patterns - whole-segment `**` included - match inside
    // the store, and their results come back relative to it.
    assert_eq!(view.glob("*.txt")?, ["a.txt"]);
    assert_eq!(view.glob("**")?, ["a.txt", "dir/sub/b.txt"]);
    assert_eq!(view.glob("dir/*/")?, ["dir/sub"]);
    Ok(())
}

#[test]
fn error_paths_come_back_in_the_callers_logical_form() -> Result<(), VfsError> {
    let store = Store::new();
    let view = fresh_view(&store)?;
    // A backend error names the path as the caller spelled it, never the
    // canonical `/dir/notes.md`.
    assert_eq!(view.read("dir/notes.md"), Err(absent("dir/notes.md")));
    view.write("dir/notes.md", b"one")?;
    // A two-path operation names its destination the same way.
    for (op, result) in [
        ("copy", view.copy("dir/notes.md", "dir")),
        ("rename", view.rename("dir/notes.md", "dir")),
    ] {
        assert_eq!(
            result,
            Err(VfsError::IsADirectory {
                path: "dir".to_owned(),
            }),
            "{op}"
        );
    }
    // A conflict names the logical path, in its path and in its
    // diagnosis.
    let other = fresh_view(&store)?;
    match other.write("dir/notes.md", b"two") {
        Err(VfsError::Conflict { path, detail }) => {
            assert_eq!(path, "dir/notes.md");
            assert!(detail.contains(" dir/notes.md "), "{detail}");
            assert!(!detail.contains("/dir"), "{detail}");
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    // The refused write never landed.
    assert_eq!(view.read("dir/notes.md")?, b"one");
    Ok(())
}

#[test]
fn one_identity_never_conflicts_with_itself_through_two_views() -> Result<(), VfsError> {
    let store = Store::new();
    let access = access(&VfsRef::default())?;
    let view = store.view(&access)?;
    view.write("a.txt", b"one")?;
    view.append("a.txt", b"two")?;
    assert_eq!(view.read("a.txt")?, b"onetwo");
    // A second view under the same access holds the same identity, so
    // its operations are ordered with the first view's.
    let also = store.view(&access)?;
    assert_eq!((view.id(), also.id()), (access.id(), access.id()));
    assert_eq!(also.read("a.txt")?, b"onetwo");
    also.write("a.txt", b"three")?;
    assert_eq!(view.read("a.txt")?, b"three");
    Ok(())
}

#[test]
fn an_arm_spawned_from_a_store_view_views_the_same_store() -> Result<(), VfsError> {
    let store = Store::new();
    let (access, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let view = store.view(&access)?;
    view.write("notes.md", b"before the fork")?;
    let arm = scope.spawn(&view, test_origin())?;
    assert_ne!(arm.id(), view.id(), "the arm is an identity of its own");
    // The arm reads the store file its parent wrote before the fork,
    // under the view's contract: strict paths and logical error paths.
    assert_eq!(arm.read("notes.md")?, b"before the fork");
    assert_eq!(
        arm.read("/notes.md"),
        Err(VfsError::InvalidPath {
            path: "/notes.md".to_owned(),
            reason: PathReason::Absolute,
        })
    );
    assert_eq!(arm.read("absent.md"), Err(absent("absent.md")));
    // The arm's write lands in the store, and never in the handle the
    // access came from.
    arm.write("arm.md", b"arm")?;
    scope.join(view.id(), arm.id());
    assert_eq!(view.read("arm.md")?, b"arm");
    assert!(!access.exists("/arm.md")?);
    Ok(())
}

#[test]
fn closing_a_store_view_drops_one_reference_to_its_identity() -> Result<(), VfsError> {
    let store = Store::new();
    // Another scope's write conflicts for as long as the identity lives.
    let other = fresh_view(&store)?;

    // The access outlives the view: the identity lives on past the
    // view's close, and ends with the access.
    let outliving = access(&VfsRef::default())?;
    let view = store.view(&outliving)?;
    view.write("closed-first.md", b"1")?;
    view.close()?;
    conflict_message(other.write("closed-first.md", b"2"));
    drop(outliving);
    other.write("closed-first.md", b"2")?;

    // The view outlives the access: it is the identity's last access, so
    // the identity, and its scope, end when it closes.
    let outlived = access(&VfsRef::default())?;
    let view = store.view(&outlived)?;
    drop(outlived);
    view.write("closed-last.md", b"1")?;
    conflict_message(other.write("closed-last.md", b"2"));
    view.close()?;
    other.write("closed-last.md", b"2")?;
    Ok(())
}
