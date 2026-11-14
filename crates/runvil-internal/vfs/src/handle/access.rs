//! The capability's machinery: spawning, joining, and the gate, the
//! admission, and the event every operation passes through.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::Access;
use super::claims::Claims;
use super::forward::StoreScoped;
use super::scope::Scope;
use super::store_view::{relativize_error, validate_store_path};
#[cfg(doc)]
use super::{ScopeHandle, VfsRef};
use crate::error::VfsError;
use crate::observe::{OpEvent, Origin};
use crate::path::{VfsPath, canonicalize};
use crate::traits::{AcquireContext, ExecId, Op, Verdict, Vfs, VfsAccess};

impl Access {
    /// Returns the capability for a new concurrent thread of execution.
    /// The spawn is the fork: the child gets a fresh [`ExecId`] and a
    /// shared snapshot of this capability's clock plus its own entry,
    /// while this capability's entry advances, so its later accesses are
    /// not ordered before the child's. [`ScopeHandle::join`] is the
    /// matching join. `origin` labels the child's operation events, as in
    /// [`VfsRef::acquire`].
    ///
    /// # Errors
    /// Returns an error when the backend refuses to acquire the child's
    /// identity; see [`VfsRef::acquire`]. A failed spawn still advances
    /// this capability's own clock entry, which orders nothing new: its
    /// later accesses are merely no longer ordered before a child that
    /// never ran. Returns [`VfsError::ScopeEnded`], before the fork, when
    /// the run or the tool call that owned this capability has ended.
    ///
    /// Crate-internal: backs [`ScopeHandle::spawn`].
    pub(crate) fn spawn(&self, origin: Origin) -> Result<Access, VfsError> {
        // A spawn names no path, so its refusal names `/`, on a store
        // view too, as the refusal of a new view does.
        let root = VfsPath::root();
        self.scope.refuse_if_closed(&root)?;
        let id = ExecId::vend();
        // The fork comes first so a wrapped handle acquiring the child
        // finds it already registered in the scope.
        self.scope.fork(self.id, id, &root)?;
        let inner = match self
            .backend()
            .acquire(&AcquireContext::new(id, Arc::clone(&self.scope)))
        {
            Ok(inner) => inner,
            Err(err) => {
                self.scope.release(id);
                return Err(err);
            }
        };
        // A store view's arms keep the view's logical error paths.
        let inner: Box<dyn VfsAccess> = if self.store_view {
            Box::new(StoreScoped { inner })
        } else {
            inner
        };
        Ok(Access {
            id,
            origin: Some(origin),
            // A store view's arm is a view of the same store.
            store_view: self.store_view,
            volume: Arc::clone(&self.volume),
            policy: Arc::clone(&self.policy),
            scope: Arc::clone(&self.scope),
            inner: Mutex::new(inner),
            released: false,
        })
    }

    /// The identity every operation through this capability is
    /// attributed to, as [`ScopeHandle::join`] and
    /// [`ScopeHandle::end_access`] name it.
    #[must_use]
    pub const fn id(&self) -> ExecId {
        self.id
    }

    /// The claims-table half of one operation: refuses once the run that
    /// owned this access has ended, runs `claim` against this access's
    /// identity and scope on `path`'s claim key - whose view refuses once
    /// the tool call that owned the identity has ended - and re-spells a
    /// store view's error paths into the caller's logical form.
    pub(super) fn admit(
        &self,
        claim: impl FnOnce(&Claims, &Arc<Scope>, ExecId, &VfsPath) -> Result<(), VfsError>,
        path: &VfsPath,
    ) -> Result<(), VfsError> {
        self.scope
            .refuse_if_closed(path)
            .and_then(|()| {
                claim(
                    &self.volume.claims,
                    &self.scope,
                    self.id,
                    &self.claim_key(path),
                )
            })
            .map_err(|err| self.relativize(err))
    }

    /// The key `path` is claimed under: the path, case-folded below a
    /// mount whose file system folds case.
    pub(super) fn claim_key(&self, path: &VfsPath) -> VfsPath {
        self.volume.folds.key(path)
    }

    /// Re-spells an error's paths for the store view's caller; a plain
    /// access's errors pass through untouched.
    pub(super) fn relativize(&self, err: VfsError) -> VfsError {
        if self.store_view {
            relativize_error(err)
        } else {
            err
        }
    }

    /// Canonicalizes at receipt and consults the policy - in that order,
    /// so a denied operation never registers a claim and every claim key
    /// is the canonical path. A path without a leading `/` is relative to
    /// `/`. The store view applies the store's strict logical-path rules
    /// before canonicalization. A store has no policy of its own, so a
    /// view's check always allows.
    pub(super) fn gate(&self, op: Op, path: &str) -> Result<VfsPath, VfsError> {
        // The strict rules run on every caller-supplied path, glob
        // patterns included, so a store path is always in its relative,
        // logical form. A pattern's wildcard grammar is validated
        // separately, in `Access::glob`, before the gate.
        if self.store_view {
            validate_store_path(path)?;
        }
        let path = canonicalize(path)?;
        self.check_policy(op, &path)
            .map_err(|err| self.relativize(err))?;
        Ok(path)
    }

    /// Consults the handle's policy. `Ask` maps to `ApprovalRequired`:
    /// this layer never asks for approval, and the reason string still
    /// names what was asked and which rule fired.
    fn check_policy(&self, op: Op, path: &VfsPath) -> Result<(), VfsError> {
        match self.policy.check(op, path) {
            Verdict::Allow => Ok(()),
            Verdict::Deny(reason) => Err(VfsError::PermissionDenied {
                path: path.to_string(),
                reason,
            }),
            Verdict::Ask(reason) => Err(VfsError::ApprovalRequired {
                path: path.to_string(),
                reason,
            }),
        }
    }

    /// Fires the installed sink for one admitted path of `op` -
    /// fire-and-forget, after policy and claims pass, before the backend
    /// executes. A handle without a sink, or a capability vended to a
    /// mounted-handle forward (the outer handle already fired with the
    /// caller's origin), fires nothing.
    pub(super) fn fire(&self, op: Op, path: &VfsPath) {
        let (Some(sink), Some(origin)) = (&self.volume.sink, &self.origin) else {
            return;
        };
        sink(OpEvent { op, path, origin });
    }

    /// Poison-safe lock on the backend's access object, held per call.
    pub(super) fn inner(&self) -> MutexGuard<'_, Box<dyn VfsAccess>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Poison-safe lock on the backend.
    pub(super) fn backend(&self) -> MutexGuard<'_, Box<dyn Vfs>> {
        self.volume
            .backend
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
