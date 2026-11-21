//! What a Host offers the Plugins it loads through the C contract:
//! [`HostResources`], the JSON values a Host provides under `vendor/name`
//! names, and the text of them that a tool call is lent.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use runvil_plugin::{Error, ErrorCode, ErrorFact};
use serde_json::{Map, Value};

use crate::host::is_segment;

#[cfg(test)]
#[path = "resources-tests.rs"]
mod tests;

/// Reads one resource as it is now, or `None` when the Host has none to
/// offer right now.
type Read = Arc<dyn Fn() -> Option<Value> + Send + Sync>;

/// The resources a Host offers every Plugin it loads through the C
/// contract: JSON values under `vendor/name` names, such as
/// `runvil/web-search`. What a resource holds is between the Host and the
/// Plugins that read it.
///
/// A Host fills one and hands it to the builder of its `Host`, with
/// `HostBuilder::with_resources`, or to `insert_plugin_dir`. Every tool
/// call of a Plugin loaded with it is then lent
/// the resources as they are when the call starts. A clone shares the
/// functions. Its `Debug` lists the names and never a value, because a
/// value can hold a key.
#[derive(Clone, Default)]
pub struct HostResources {
    /// The function that reads each resource, by the resource's name.
    readers: BTreeMap<String, Read>,
}

impl HostResources {
    /// Resources that offer nothing.
    #[must_use]
    pub fn new() -> HostResources {
        HostResources::default()
    }

    /// Offers what `read` returns under `name`. The loader calls `read`
    /// each time a tool call starts, on the run's own task, so it
    /// returns at once. `None` leaves the resource out of that call. A
    /// `read` that panics ends that one tool call, as a panic in the
    /// call does.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error that
    /// names `name`, with the reason `invalid_name` when `name` is not a
    /// vendor and a name joined by `/`, each one name segment as in a
    /// Pack's name, and `duplicate_name` when `name` is offered already.
    /// The resources stay as they were on either error.
    pub fn provide(
        &mut self,
        name: &str,
        read: impl Fn() -> Option<Value> + Send + Sync + 'static,
    ) -> Result<(), Error> {
        let two_segments = name
            .split_once('/')
            .is_some_and(|(vendor, resource)| is_segment(vendor) && is_segment(resource));
        if !two_segments {
            return Err(make_refusal_error(
                "invalid_name",
                format!("resource name `{name}` is not a vendor/name pair"),
                name,
            ));
        }
        if self.readers.contains_key(name) {
            return Err(make_refusal_error(
                "duplicate_name",
                format!("resource `{name}` is already offered"),
                name,
            ));
        }
        self.readers.insert(name.to_owned(), Arc::new(read));
        Ok(())
    }

    /// The JSON text of the resources as they are now: an object with one
    /// key for each resource whose function returns a value, in name
    /// order, and `{}` when none does. It calls each function once.
    pub(crate) fn text(&self) -> String {
        let object: Map<String, Value> = self
            .readers
            .iter()
            .filter_map(|(name, read)| Some((name.clone(), read()?)))
            .collect();
        Value::Object(object).to_string()
    }
}

impl fmt::Debug for HostResources {
    /// Lists the names offered, never a value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostResources")
            .field("names", &self.readers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The refusal of a resource: an `InvalidValue` that names the resource,
/// with the broken rule as its reason. Its prose is its message, which
/// names the resource and the rule.
fn make_refusal_error(reason: &'static str, message: String, name: &str) -> Error {
    let mut error = Error::new(ErrorCode::InvalidValue, message.clone(), message);
    error.facts.insert(ErrorFact::Name, Value::from(name));
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error
}
