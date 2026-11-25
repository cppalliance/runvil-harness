//! The Gateway's resources for the Plugins a Host loads through the C
//! contract. [`provide_gateway_resources`] offers them in a Host's
//! `HostResources`, so every tool call of such a Plugin is lent the
//! address and the key of the Gateway, and the Plugin calls the Gateway
//! itself.

use gateway_client::{GatewayEndpoint, SecretString};
use runvil::Error;
use runvil::harness::HostResources;
use serde_json::{Value, json};

/// The name of the Gateway's web search among a Host's resources. A
/// Plugin may not depend on a Harness crate, so the Plugin that reads the
/// entry writes this same name on its side.
const WEB_SEARCH: &str = "runvil/web-search";

/// Offers `resources` everything the Gateway serves to Plugins.
///
/// `current` is asked as each tool call of a Plugin starts. It returns
/// the Gateway to use right now, as the API root and the bearer key that
/// [`GatewayBroker::new`](crate::GatewayBroker::new) takes, or `None`
/// when there is none right now, which leaves the Gateway's resources out
/// of that call. The Harness asks on the task that polls the run, so
/// `current` returns at once.
///
/// The Gateway serves Plugins one resource, its web search, under the
/// name `runvil/web-search`. The entry is a JSON object:
///
/// ```json
/// {
///   "version": 1,
///   "url": "http://127.0.0.1:52814/v1/tools/web_search",
///   "api_key": "..."
/// }
/// ```
///
/// What the entry tells the Plugin that reads it:
///
/// - `version` 1 means: send the Gateway's web search request as the
///   JSON body of a POST to `url`, and read the Gateway's web search
///   reply. The version is in the entry and never in the request body,
///   because the Gateway refuses a body that holds a field it does not
///   know.
/// - `url` is the Gateway's web search route: the API root, then
///   `/tools/web_search`.
/// - `api_key`, when present, goes in the request as the header
///   `Authorization: Bearer <api_key>`. An entry without it means a
///   request with no `Authorization` header. This function always writes
///   it, as `GatewayBroker::new` always takes a key.
///
/// The entry holds the key as plain text, because the Plugin presents it
/// to the Gateway itself. Every Plugin that is lent `resources` can
/// therefore read the key.
///
/// # Errors
/// Returns the [`InvalidValue`](runvil::ErrorCode::InvalidValue) error of
/// [`HostResources::provide`], with the reason `duplicate_name`, when
/// `resources` offers `runvil/web-search` already. The error names the
/// resource and holds no key, and `resources` stays as it was.
pub fn provide_gateway_resources(
    resources: &mut HostResources,
    current: impl Fn() -> Option<(GatewayEndpoint, SecretString)> + Send + Sync + 'static,
) -> Result<(), Error> {
    resources.provide(WEB_SEARCH, move || web_search(&current))
}

/// The web search resource as it is now: the entry for the Gateway that
/// `current` returns, or `None` when it returns none.
fn web_search(current: &impl Fn() -> Option<(GatewayEndpoint, SecretString)>) -> Option<Value> {
    let (endpoint, key) = current()?;
    Some(web_search_entry(&endpoint, &key))
}

/// The `runvil/web-search` entry for the Gateway whose API root is
/// `endpoint` and whose bearer key is `key`, in the shape
/// [`provide_gateway_resources`] documents.
fn web_search_entry(endpoint: &GatewayEndpoint, key: &SecretString) -> Value {
    json!({
        "version": 1,
        "url": format!("{}/tools/web_search", endpoint.url()),
        "api_key": key.expose_secret(),
    })
}

#[cfg(test)]
#[path = "resources-tests.rs"]
mod tests;
