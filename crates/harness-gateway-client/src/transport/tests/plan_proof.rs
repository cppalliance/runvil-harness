//! A Host-supplied plan proof is one header on a model round, redacted in
//! `Debug`, and absent unless the Host passed it. The client has no path
//! argument and reads no private file.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::HeaderMap;
use axum::routing::post;
use runvil::model::Message;

use super::{keyed_client, ok_stream, openai_options};
use crate::config::{GatewayEndpoint, SecretString};
use crate::{GatewayChat, PLAN_PROOF_HEADER};

const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Records every plan-proof header value the mock saw, in arrival order,
/// beside the authorization header.
async fn spawn_capturing() -> (String, Arc<Mutex<Vec<(Option<String>, Vec<String>)>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&captured);
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |headers: HeaderMap| {
            let slot = Arc::clone(&slot);
            async move {
                let authorization = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let plan_proof = headers
                    .get_all(PLAN_PROOF_HEADER)
                    .iter()
                    .filter_map(|value| value.to_str().ok().map(str::to_owned))
                    .collect();
                slot.lock()
                    .expect("capture lock")
                    .push((authorization, plan_proof));
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    ok_stream(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback listener binds");
    let addr = listener
        .local_addr()
        .expect("a bound listener has a local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("the mock gateway serves cleanly");
    });
    (format!("http://{addr}/v1"), captured)
}

fn proof() -> SecretString {
    SecretString::new(PROOF).expect("the test proof is non-empty")
}

async fn complete(client: &GatewayChat) {
    client
        .complete(&[Message::user("hi")], None, &openai_options(), |_| {})
        .await
        .expect("the completion succeeds");
}

#[tokio::test]
async fn a_passed_proof_is_one_header_on_the_round_and_the_bearer_stays() {
    let (base, captured) = spawn_capturing().await;
    let client = keyed_client(&base).with_plan_proof(proof());
    complete(&client).await;
    let seen = captured.lock().expect("capture lock").clone();
    assert_eq!(seen.len(), 1, "one round, one request");
    assert_eq!(seen[0].0.as_deref(), Some("Bearer tok"));
    assert_eq!(seen[0].1, [PROOF], "exactly one plan-proof header");
}

#[tokio::test]
async fn a_keyless_client_with_a_proof_omits_authorization_and_sends_the_header() {
    let (base, captured) = spawn_capturing().await;
    let client = GatewayChat::keyless(GatewayEndpoint::new(&base).expect("valid endpoint"))
        .with_plan_proof(proof());
    complete(&client).await;
    let seen = captured.lock().expect("capture lock").clone();
    assert_eq!(seen[0].0, None, "keyless still omits Authorization");
    assert_eq!(seen[0].1, [PROOF]);
}

#[test]
fn debug_redacts_the_plan_proof_whether_or_not_one_was_passed() {
    let endpoint = GatewayEndpoint::new("http://127.0.0.1:8081/v1").expect("valid");
    let plain = GatewayChat::keyless(endpoint.clone());
    let holding = GatewayChat::keyless(endpoint).with_plan_proof(proof());
    for rendered in [format!("{plain:?}"), format!("{holding:?}")] {
        assert!(
            !rendered.contains(PROOF),
            "the plan proof must never appear in Debug output, got: {rendered}"
        );
        assert!(rendered.contains("<redacted>"), "got: {rendered}");
        assert!(
            !rendered.contains("Some") && !rendered.contains("None"),
            "presence of the proof must not leak, got: {rendered}"
        );
    }
}

#[tokio::test]
async fn from_env_sends_no_plan_proof_header() {
    // Constructors take an endpoint and, when keyed, a bearer. None takes a
    // path, and the environment constructor reads only the URL and the key.
    let (base, captured) = spawn_capturing().await;
    let client = super::super::from_env_with(super::lookup_from(&[
        ("RUNVIL_GATEWAY_URL", &base),
        ("RUNVIL_GATEWAY_API_KEY", "tok"),
    ]))
    .expect("a URL and a key build");
    complete(&client).await;
    let seen = captured.lock().expect("capture lock").clone();
    assert_eq!(seen[0].0.as_deref(), Some("Bearer tok"));
    assert!(
        seen[0].1.is_empty(),
        "from_env must not invent a plan proof, got {:?}",
        seen[0].1
    );
}
