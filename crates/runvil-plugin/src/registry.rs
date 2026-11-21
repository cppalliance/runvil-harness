//! The Packs a Host's Plugins insert: [`PackRegistry`], which starts
//! empty and holds each Pack a Plugin inserts with its factory.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use runvil_types::packs::PackId;
use serde_json::Value;

use crate::{PackFactory, PackInfo};

#[cfg(test)]
#[path = "registry-tests.rs"]
mod tests;

/// Every Pack a Host's Plugins insert, by fully qualified name, each
/// with its Pack id and its factory, or the reason it was inserted
/// unavailable. It holds one Pack per Pack id, whatever the vendor.
///
/// The build of a Host owns the registry; a Plugin only inserts its
/// Packs into it, by calling [`insert`](PackRegistry::insert) or
/// [`insert_unavailable`](PackRegistry::insert_unavailable) from its
/// `install` function. The build makes the registry with
/// [`new`](PackRegistry::new), which starts empty, and lends it to each
/// Plugin the Host named. Then it reads the registry
/// through [`into_entries`](PackRegistry::into_entries), keeps
/// each Pack's factory, and adds the built-in Packs `runvil/subagents`
/// and `runvil/user`, which the Harness makes itself. So the registry
/// refuses a Pack that takes the Pack id of either. Every run that
/// starts holds every Pack of the registry and both built-ins, and each
/// run calls the factory of each Pack it holds to make that run's
/// instance.
#[derive(Default)]
pub struct PackRegistry {
    entries: BTreeMap<&'static str, Registered>,
}

/// One Pack the registry holds.
struct Registered {
    info: PackInfo,
    id: PackId,
    /// The Pack's factory, or the reason it was inserted unavailable.
    factory: Result<Arc<dyn PackFactory>, Error>,
}

impl PackRegistry {
    /// A registry holding no Pack.
    #[must_use]
    pub fn new() -> PackRegistry {
        PackRegistry::default()
    }

    /// Adds the Pack `info` names as a Plugin's Pack, made by `factory`.
    ///
    /// The Pack id is the FQN's second segment, so `runvil/web` is `web`.
    /// The registry holds one Pack per Pack id, so it holds `runvil/web`
    /// or `acme/web` and never both: the first one inserted keeps the
    /// Pack id.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error that names
    /// the FQN, so the Host's author sees the refusal at startup, with the
    /// reason:
    ///
    /// - `invalid_fqn` when the FQN is not a `vendor/name` pair of valid
    ///   one-segment names;
    /// - `duplicate_pack` when the registry holds the FQN already;
    /// - `reserved_pack_id` when the Pack id is
    ///   [reserved](PackId::is_reserved), such as `adhoc`;
    /// - `pack_id_taken` when a built-in Pack holds that Pack id, as
    ///   `runvil/user` holds `user`, or when the registry holds a Pack of
    ///   that Pack id under another FQN. The message names the holder.
    ///
    /// A Pack whose factory refuses a run is not refused here; that run
    /// holds the Pack as unavailable instead.
    pub fn insert(
        &mut self,
        info: PackInfo,
        factory: impl PackFactory + 'static,
    ) -> Result<(), Error> {
        self.add(info, Ok(Arc::new(factory)))
    }

    /// Adds the Pack `info` names as a Plugin's Pack that is unavailable for
    /// `reason`, such as a configuration the Plugin cannot use.
    /// `Host::parse` refuses a prompt that declares it, naming `reason`.
    ///
    /// # Errors
    /// Refuses what [`insert`](PackRegistry::insert) refuses, with the
    /// same errors.
    pub fn insert_unavailable(&mut self, info: PackInfo, reason: Error) -> Result<(), Error> {
        self.add(info, Err(reason))
    }

    /// Adds `info` as a Plugin's Pack with `factory`, or refuses it as
    /// [`insert`](PackRegistry::insert) documents.
    fn add(
        &mut self,
        info: PackInfo,
        factory: Result<Arc<dyn PackFactory>, Error>,
    ) -> Result<(), Error> {
        let fqn = info.fqn;
        let id = pack_id(fqn).ok_or_else(|| {
            make_refusal_error(
                "invalid_fqn",
                format!("Pack fully qualified name `{fqn}` is not a vendor/name pair"),
                fqn,
            )
        })?;
        if self.entries.contains_key(fqn) {
            return Err(make_refusal_error(
                "duplicate_pack",
                format!("Pack `{fqn}` is already in the registry"),
                fqn,
            ));
        }
        if id.is_reserved() {
            return Err(make_refusal_error(
                "reserved_pack_id",
                format!("Pack `{fqn}` cannot take the Pack id {id}, which the Engine reserves"),
                fqn,
            ));
        }
        if let Some(builtin) = builtin_holding(&id) {
            return Err(make_refusal_error(
                "pack_id_taken",
                format!(
                    "Pack `{fqn}` cannot take the Pack id {id}, which the built-in Pack \
                     `{builtin}` holds"
                ),
                fqn,
            ));
        }
        if let Some(holder) = self.holder(&id) {
            return Err(make_refusal_error(
                "pack_id_taken",
                format!("Pack `{fqn}` cannot take the Pack id {id}, which `{holder}` holds"),
                fqn,
            ));
        }
        self.entries
            .insert(info.fqn, Registered { info, id, factory });
        Ok(())
    }

    /// Every Pack the registry holds, in FQN order.
    pub fn list(&self) -> impl Iterator<Item = &PackInfo> {
        self.entries.values().map(|registered| &registered.info)
    }

    /// Every Pack the registry holds, with its Pack id and its factory,
    /// or the reason it was inserted unavailable, in FQN order: what a
    /// Host builds its Packs from.
    pub fn into_entries(
        self,
    ) -> impl Iterator<Item = (PackInfo, PackId, Result<Arc<dyn PackFactory>, Error>)> {
        self.entries
            .into_values()
            .map(|registered| (registered.info, registered.id, registered.factory))
    }

    /// The FQN of the Pack that holds the Pack id `id`, which no other
    /// Pack may take.
    fn holder(&self, id: &PackId) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(_, registered)| registered.id == *id)
            .map(|(fqn, _)| *fqn)
    }
}

impl fmt::Debug for PackRegistry {
    /// Shows each Pack's FQN and Pack id.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<(&str, &PackId)> = self
            .entries
            .values()
            .map(|registered| (registered.info.fqn, &registered.id))
            .collect();
        f.debug_struct("PackRegistry")
            .field("entries", &entries)
            .finish_non_exhaustive()
    }
}

/// The refusal of an insert: an `InvalidValue` that names the FQN, with the
/// broken rule as its reason. Its prose is its message, which names the Pack
/// and the rule.
fn make_refusal_error(reason: &'static str, message: String, fqn: &str) -> Error {
    let mut error = Error::new(ErrorCode::InvalidValue, message.clone(), message);
    error.facts.insert(ErrorFact::Name, Value::from(fqn));
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error
}

/// The built-in Packs, by FQN. Every Host has both, and the Harness makes
/// them itself, so no registry holds one. Each still holds its Pack id:
/// a Plugin's Pack can take neither the name nor the Pack id of one.
const BUILTIN: [&str; 2] = ["runvil/subagents", "runvil/user"];

/// The FQN of the built-in Pack that holds the Pack id `id`.
fn builtin_holding(id: &PackId) -> Option<&'static str> {
    BUILTIN
        .into_iter()
        .find(|fqn| pack_id(fqn).is_some_and(|held| held == *id))
}

/// The Pack id of the FQN `vendor/name`: its second segment, when both
/// segments are valid one-segment names.
fn pack_id(fqn: &str) -> Option<PackId> {
    let (vendor, name) = fqn.split_once('/')?;
    PackId::parse(vendor).ok()?;
    PackId::parse(name).ok()
}
