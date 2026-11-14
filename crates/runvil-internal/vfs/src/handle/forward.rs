//! The forwarding wrappers: a handle mounted as another handle's
//! backend, and a store view's session, each forwarding every operation,
//! the second re-spelling error paths where they are produced.

use super::store_view::relativize_error;
use super::{Access, VfsRef};
use crate::error::VfsError;
use crate::path::{VfsPath, VfsPathBuf};
use crate::stat::{Entry, Stat};
use crate::traits::{AcquireContext, ExecId, Vfs, VfsAccess};

/// Makes a handle usable as a backend, so it can be mounted inside
/// another handle.
///
/// Operations routed to the mounted handle pass through its own policy
/// and claims, under the caller's identity. [`VfsRef::overlay`] relies
/// on this. It mounts the base handle at the root of the overlay's
/// router. Both views of the same storage share one claims table.
impl Vfs for VfsRef {
    fn acquire(&mut self, cx: &AcquireContext) -> Result<Box<dyn VfsAccess>, VfsError> {
        // The mount forward leaves the origin unset: the outer handle already
        // fired the caller's, and a fabricated one here would double the
        // event with a less precise label.
        Ok(Box::new(HandleAccess(self.acquire_with(cx, None)?)))
    }

    fn release(&mut self, id: ExecId) -> Result<(), VfsError> {
        // The vended session's Drop drops the identity's reference;
        // nothing is registered at this level.
        let _ = id;
        Ok(())
    }

    fn read_only(&self) -> bool {
        self.backend().read_only()
    }
}

/// The session vended by a mounted handle: forwards every operation
/// through the base handle's capability, so its policy and claims apply
/// under the caller's identity. Paths arrive canonical, so the
/// capability's canonicalization at receipt is an idempotent re-check.
///
/// The POSIX extras keep their trait defaults: the public capability
/// exposes none of them, so there is nothing to forward to.
struct HandleAccess(Access);

impl VfsAccess for HandleAccess {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>, VfsError> {
        self.0.read(path.as_str())
    }

    fn read_range(&self, path: &VfsPath, offset: u64, len: u64) -> Result<Vec<u8>, VfsError> {
        // Forwarded, not defaulted, so a base backend that can seek never
        // materializes the file.
        self.0.read_range(path.as_str(), offset, len)
    }

    fn write(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.0.write(path.as_str(), contents)
    }

    fn append(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.0.append(path.as_str(), contents)
    }

    fn remove(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        // The backend trait's contract reports an absent path as
        // NotFound; the capability's Ok(false) maps back onto it.
        if self.0.remove(path.as_str(), recursive)? {
            Ok(())
        } else {
            Err(VfsError::NotFound {
                path: path.to_string(),
            })
        }
    }

    fn exists(&self, path: &VfsPath) -> Result<bool, VfsError> {
        self.0.exists(path.as_str())
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>, VfsError> {
        self.0.glob(pattern)
    }

    fn glob_kind(&self, pattern: &str, dirs_only: bool) -> Result<Vec<String>, VfsError> {
        // The forward re-enters the full public pipeline with the flag
        // intact, so the dirs-only split survives the handle boundary.
        self.0.glob_pattern(pattern, dirs_only)
    }

    fn list(&self, path: &VfsPath) -> Result<Vec<Entry>, VfsError> {
        self.0.list(path.as_str())
    }

    fn stat(&self, path: &VfsPath) -> Result<Stat, VfsError> {
        self.0.stat(path.as_str())
    }

    fn mkdir(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.0.mkdir(path.as_str(), recursive)
    }

    fn rename(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.0.rename(from.as_str(), to.as_str())
    }

    fn copy(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.0.copy(from.as_str(), to.as_str())
    }

    fn str_replace(&mut self, path: &VfsPath, old: &str, new: &str) -> Result<(), VfsError> {
        self.0.str_replace(path.as_str(), old, new)
    }

    fn close(&mut self) -> Result<(), VfsError> {
        self.0.release()
    }
}

/// A store view's backend session: the store's one mount behind its
/// router. The mount and the router both name canonical paths in their
/// errors, re-spelled here into the caller's logical form.
pub(super) struct StoreScoped {
    pub(super) inner: Box<dyn VfsAccess>,
}

impl VfsAccess for StoreScoped {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>, VfsError> {
        self.inner.read(path).map_err(relativize_error)
    }

    fn read_range(&self, path: &VfsPath, offset: u64, len: u64) -> Result<Vec<u8>, VfsError> {
        self.inner
            .read_range(path, offset, len)
            .map_err(relativize_error)
    }

    fn write(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.inner.write(path, contents).map_err(relativize_error)
    }

    fn append(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.inner.append(path, contents).map_err(relativize_error)
    }

    fn remove(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.inner.remove(path, recursive).map_err(relativize_error)
    }

    fn exists(&self, path: &VfsPath) -> Result<bool, VfsError> {
        self.inner.exists(path).map_err(relativize_error)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>, VfsError> {
        self.inner.glob(pattern).map_err(relativize_error)
    }

    fn glob_kind(&self, pattern: &str, dirs_only: bool) -> Result<Vec<String>, VfsError> {
        self.inner
            .glob_kind(pattern, dirs_only)
            .map_err(relativize_error)
    }

    fn list(&self, path: &VfsPath) -> Result<Vec<Entry>, VfsError> {
        self.inner.list(path).map_err(relativize_error)
    }

    fn stat(&self, path: &VfsPath) -> Result<Stat, VfsError> {
        self.inner.stat(path).map_err(relativize_error)
    }

    fn mkdir(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.inner.mkdir(path, recursive).map_err(relativize_error)
    }

    fn rename(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.inner.rename(from, to).map_err(relativize_error)
    }

    fn copy(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.inner.copy(from, to).map_err(relativize_error)
    }

    fn str_replace(&mut self, path: &VfsPath, old: &str, new: &str) -> Result<(), VfsError> {
        self.inner
            .str_replace(path, old, new)
            .map_err(relativize_error)
    }

    fn symlink(&mut self, target: &VfsPath, link: &VfsPath) -> Result<(), VfsError> {
        self.inner.symlink(target, link).map_err(relativize_error)
    }

    fn read_link(&self, path: &VfsPath) -> Result<VfsPathBuf, VfsError> {
        self.inner.read_link(path).map_err(relativize_error)
    }

    fn chmod(&mut self, path: &VfsPath, mode: u32) -> Result<(), VfsError> {
        self.inner.chmod(path, mode).map_err(relativize_error)
    }

    fn close(&mut self) -> Result<(), VfsError> {
        self.inner.close().map_err(relativize_error)
    }
}
