//! Tests for the scope handle: it forks, joins, and ends the identities
//! of its own scope, ends that scope, and leaves every other scope alone.

use std::fmt;

use crate::test_support::{access, test_origin};
use crate::{VfsError, VfsRef};

/// Asserts that `result` is the refusal of an ended scope or an ended
/// identity, naming `expected`.
#[track_caller]
fn refused<T: fmt::Debug>(result: Result<T, VfsError>, expected: &str) {
    match result {
        Err(VfsError::ScopeEnded { path }) => assert_eq!(path, expected),
        other => panic!("expected an ended refusal on {expected}, got {other:?}"),
    }
}

/// Asserts that `result` is a conflict with another access's claim.
#[track_caller]
fn conflicts<T: fmt::Debug>(result: Result<T, VfsError>) {
    match result {
        Err(VfsError::Conflict { .. }) => {}
        other => panic!("expected a conflict, got {other:?}"),
    }
}

#[test]
fn a_spawn_forks_an_identity_ordered_after_its_parents_earlier_work() -> Result<(), VfsError> {
    let (parent, scope) = VfsRef::default().acquire_scope(test_origin())?;
    parent.write("/before.txt", b"parent")?;
    let child = scope.spawn(&parent, test_origin())?;
    assert_ne!(
        child.id(),
        parent.id(),
        "the child is an identity of its own"
    );
    // Everything the parent did before the spawn happens before the
    // child's first step.
    assert_eq!(child.read("/before.txt")?, b"parent");
    // Past the fork, nothing orders the parent's work and the child's.
    parent.write("/after.txt", b"parent")?;
    conflicts(child.read("/after.txt"));
    child.write("/child.txt", b"child")?;
    conflicts(parent.read("/child.txt"));
    Ok(())
}

#[test]
fn a_join_orders_the_childs_work_before_the_owners_next_step() -> Result<(), VfsError> {
    let (owner, scope) = VfsRef::default().acquire_scope(test_origin())?;
    // A child whose identity is still live contributes its current clock.
    let live = scope.spawn(&owner, test_origin())?;
    live.write("/live.txt", b"live")?;
    conflicts(owner.read("/live.txt"));
    scope.join(owner.id(), live.id());
    assert_eq!(owner.read("/live.txt")?, b"live");
    // A child whose last access dropped contributes its final clock.
    let dropped = scope.spawn(&owner, test_origin())?;
    dropped.write("/dropped.txt", b"dropped")?;
    let dropped_id = dropped.id();
    drop(dropped);
    conflicts(owner.read("/dropped.txt"));
    scope.join(owner.id(), dropped_id);
    assert_eq!(owner.read("/dropped.txt")?, b"dropped");
    Ok(())
}

#[test]
fn ending_an_identity_refuses_its_accesses_and_joins_it_into_the_owner_it_names()
-> Result<(), VfsError> {
    let (owner, scope) = VfsRef::default().acquire_scope(test_origin())?;
    let joined = scope.spawn(&owner, test_origin())?;
    let unjoined = scope.spawn(&owner, test_origin())?;
    joined.write("/joined.txt", b"call")?;
    unjoined.write("/unjoined.txt", b"call")?;
    scope.end_access(Some(owner.id()), joined.id());
    scope.end_access(None, unjoined.id());
    // An access still holding an ended identity refuses its next
    // operation and spawn.
    for (ended, path) in [(&joined, "/joined.txt"), (&unjoined, "/unjoined.txt")] {
        refused(ended.read(path), path);
        refused(scope.spawn(ended, test_origin()), "/");
    }
    // The end that named the owner merged the identity's clock into it.
    // The end that named none merged nothing, until a join does.
    assert_eq!(owner.read("/joined.txt")?, b"call");
    conflicts(owner.read("/unjoined.txt"));
    scope.join(owner.id(), unjoined.id());
    assert_eq!(owner.read("/unjoined.txt")?, b"call");
    // The scope stays open: the owner still forks.
    scope
        .spawn(&owner, test_origin())?
        .write("/next.txt", b"next")?;
    Ok(())
}

#[test]
fn ending_the_scope_refuses_its_accesses_and_its_claims_stop_conflicting() -> Result<(), VfsError> {
    let vfs = VfsRef::default();
    let (root, scope) = vfs.acquire_scope(test_origin())?;
    root.write("/a.txt", b"first")?;
    let arm = scope.spawn(&root, test_origin())?;
    // Two live scopes never order each other, so another scope's write
    // of the path conflicts.
    let other = access(&vfs)?;
    conflicts(other.write("/a.txt", b"next"));
    scope.end();
    // Every access still held in the scope refuses its next operation
    // and spawn.
    for held in [&root, &arm] {
        refused(held.read("/a.txt"), "/a.txt");
        refused(held.write("/b.txt", b"late"), "/b.txt");
        refused(scope.spawn(held, test_origin()), "/");
    }
    // The other scope writes the path at once: the ended scope's claim
    // is ignored, though its accesses are still held.
    other.write("/a.txt", b"next")?;
    assert_eq!(other.read("/a.txt")?, b"next");
    assert!(!other.exists("/b.txt")?);
    // Ending an ended scope does nothing more.
    scope.end();
    refused(root.read("/a.txt"), "/a.txt");
    Ok(())
}

#[test]
#[should_panic(expected = "the access belongs to another scope than this scope handle's")]
fn a_scope_handle_refuses_to_fork_another_scopes_access() {
    let vfs = VfsRef::default();
    let (_mine, scope) = vfs
        .acquire_scope(test_origin())
        .expect("the memory backend acquires");
    let theirs = access(&vfs).expect("the memory backend acquires");
    let _refused = scope.spawn(&theirs, test_origin());
}

#[test]
fn a_scope_handle_leaves_every_other_scope_alone() -> Result<(), VfsError> {
    let vfs = VfsRef::default();
    let (_mine, mine) = vfs.acquire_scope(test_origin())?;
    let (theirs, their_scope) = vfs.acquire_scope(test_origin())?;
    let call = their_scope.spawn(&theirs, test_origin())?;
    call.write("/x.txt", b"call")?;
    // Another scope's handle names identities it never held: its join
    // orders nothing in their scope, and its ends end nothing there.
    mine.join(theirs.id(), call.id());
    mine.end_access(Some(theirs.id()), call.id());
    mine.end();
    conflicts(theirs.read("/x.txt"));
    call.write("/y.txt", b"still live")?;
    // The scope's own handle orders the call's writes before the root's
    // next step.
    their_scope.join(theirs.id(), call.id());
    assert_eq!(theirs.read("/x.txt")?, b"call");
    Ok(())
}

#[test]
fn a_scope_handle_never_keeps_its_scope_alive() -> Result<(), VfsError> {
    let vfs = VfsRef::default();
    let (root, scope) = vfs.acquire_scope(test_origin())?;
    let child = scope.spawn(&root, test_origin())?;
    child.write("/x.txt", b"child")?;
    let (root_id, child_id) = (root.id(), child.id());
    drop((root, child));
    // The scope went with its last access, though its handle is still
    // held, so another scope writes the path at once.
    assert!(scope.0.upgrade().is_none());
    let other = access(&vfs)?;
    other.write("/x.txt", b"next")?;
    // Every call on the handle of a scope that is gone does nothing.
    scope.join(root_id, child_id);
    scope.end_access(Some(root_id), child_id);
    scope.end();
    assert_eq!(other.read("/x.txt")?, b"next");
    Ok(())
}
