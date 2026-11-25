//! Tests for the Gateway's resources: the exact JSON of the web search
//! entry, the URL it names, what a Gateway that is absent or that moves
//! reads as, that the name is taken once, and that neither the `Debug` of
//! the resources nor a refusal holds the key.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use gateway_client::{GatewayEndpoint, SecretString};
use runvil::harness::HostResources;
use runvil::{ErrorCode, ErrorFact};
use serde_json::{Value, json};

use super::{WEB_SEARCH, provide_gateway_resources, web_search, web_search_entry};

/// The bearer key of every Gateway here but the one the pinned entry
/// names.
const KEY: &str = "gateway-key-7f3a";

/// What the `Debug` of resources that offer the Gateway alone shows.
const SHOWN: &str = r#"HostResources { names: ["runvil/web-search"] }"#;

fn endpoint(api_root: &str) -> GatewayEndpoint {
    GatewayEndpoint::new(api_root).expect("valid test endpoint")
}

fn key(text: &str) -> SecretString {
    SecretString::new(text).expect("non-empty test key")
}

/// A `current` for a Gateway that stays at `api_root`, keyed with [`KEY`],
/// which counts in `asked` each time it is asked.
fn counted_gateway(
    api_root: &'static str,
    asked: &Arc<AtomicU32>,
) -> impl Fn() -> Option<(GatewayEndpoint, SecretString)> + Send + Sync + 'static {
    let asked = Arc::clone(asked);
    move || {
        asked.fetch_add(1, Ordering::SeqCst);
        Some((endpoint(api_root), key(KEY)))
    }
}

#[test]
fn the_web_search_entry_is_exactly_the_json_a_plugin_reads() {
    let entry = web_search_entry(&endpoint("http://127.0.0.1:52814/v1"), &key("..."));
    // The web Plugin, in `cppalliance/runvil-plugin-web`, pins this same
    // JSON on its side. A Plugin may not depend on a Harness crate, so
    // the name and the shape are written in both repositories, and a
    // change to this text is a change to that one. The object around the
    // entry is the resources text of a Host that offers the Gateway
    // alone.
    assert_eq!(
        json!({ WEB_SEARCH: entry }).to_string(),
        r#"{"runvil/web-search":{"api_key":"...","url":"http://127.0.0.1:52814/v1/tools/web_search","version":1}}"#
    );
}

#[test]
fn the_url_is_the_api_root_then_the_web_search_route() {
    for (api_root, url) in [
        (
            "http://127.0.0.1:52814/v1",
            "http://127.0.0.1:52814/v1/tools/web_search",
        ),
        // An API root written with a trailing slash names the same route.
        (
            "http://127.0.0.1:52814/v1/",
            "http://127.0.0.1:52814/v1/tools/web_search",
        ),
        (
            "https://gateway.example.com/v1",
            "https://gateway.example.com/v1/tools/web_search",
        ),
    ] {
        let entry = web_search_entry(&endpoint(api_root), &key(KEY));
        assert_eq!(entry["url"], url, "{api_root}");
    }
}

#[test]
fn a_host_with_no_gateway_right_now_offers_no_web_search() {
    assert_eq!(web_search(&|| None), None);
}

#[test]
fn each_read_asks_for_the_gateway_as_it_is_then() {
    let asked = Cell::new(0_u32);
    // The Gateway moves to another port between two reads, and there is
    // none at the third.
    let current = || {
        asked.set(asked.get() + 1);
        let api_root = format!("http://127.0.0.1:{}/v1", 52_000 + asked.get());
        (asked.get() != 3).then(|| (endpoint(&api_root), key(KEY)))
    };
    let url_now = || web_search(&current).map(|entry| entry["url"].clone());

    assert_eq!(asked.get(), 0, "nothing is asked before a read");
    assert_eq!(
        url_now(),
        Some(json!("http://127.0.0.1:52001/v1/tools/web_search"))
    );
    assert_eq!(
        url_now(),
        Some(json!("http://127.0.0.1:52002/v1/tools/web_search"))
    );
    assert_eq!(url_now(), None);
    assert_eq!(
        url_now(),
        Some(json!("http://127.0.0.1:52004/v1/tools/web_search"))
    );
    assert_eq!(asked.get(), 4, "each read asks once");
}

#[test]
fn a_second_offer_on_the_same_resources_is_refused() {
    let asked = Arc::new(AtomicU32::new(0));
    let mut resources = HostResources::new();
    provide_gateway_resources(
        &mut resources,
        counted_gateway("http://127.0.0.1:52814/v1", &asked),
    )
    .expect("the first offer takes the name");

    let error = provide_gateway_resources(
        &mut resources,
        counted_gateway("http://127.0.0.1:52815/v1", &asked),
    )
    .expect_err("the name is offered already");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from("duplicate_name")),
        "{error:?}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from(WEB_SEARCH)),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "resource `runvil/web-search` is already offered"
    );
    assert!(!format!("{error:?}").contains(KEY), "{error:?}");
    assert_eq!(
        format!("{resources:?}"),
        SHOWN,
        "the resources stay as they were"
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "neither offer asks for a Gateway, so a refusal has no key to hold"
    );
}

#[test]
fn the_debug_of_the_resources_shows_the_name_and_never_the_key() {
    let asked = Arc::new(AtomicU32::new(0));
    let mut resources = HostResources::new();
    provide_gateway_resources(
        &mut resources,
        counted_gateway("http://127.0.0.1:52814/v1", &asked),
    )
    .expect("the name is free");

    let shown = format!("{resources:?}");
    assert_eq!(shown, SHOWN);
    assert!(!shown.contains(KEY), "{shown}");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "showing the resources asks for no Gateway, so it reads no key"
    );
}
