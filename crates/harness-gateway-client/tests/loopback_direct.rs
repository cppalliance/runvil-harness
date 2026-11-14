//! A loopback Gateway is contacted directly. A redirect is not followed,
//! and an `HTTP_PROXY` / `ALL_PROXY` listener receives nothing beyond the
//! canary that proves the variable is live. The proof is set so a followed
//! redirect or a proxy would have carried it.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// integration-test helpers. Failing one of these by panicking names the
// invariant the helper exists to check.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
#![expect(
    clippy::unwrap_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::io::{Read, Write};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use harness_gateway_client::{
    GatewayChat, GatewayEndpoint, GatewaySearch, SecretString, fetch_model_catalog_with_plan_proof,
};
use plugin_web::{SearchProvider, SearchQuery};
use runvil::ErrorCode;
use runvil::model::{CompletionOptions, Message};

const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SEARCH_KEY: &str = "search-secret-key";
const ROLE: &str = "LOOPBACK_DIRECT_ROLE";
const SIGNAL: &str = "LOOPBACK_DIRECT_SIGNAL";

async fn bind() -> (String, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (format!("http://{addr}"), listener)
}

fn spawn(listener: tokio::net::TcpListener, app: Router) {
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
}

fn proof() -> SecretString {
    SecretString::new(PROOF).expect("non-empty")
}

fn chat(base: &str) -> GatewayChat {
    GatewayChat::new(
        GatewayEndpoint::new(base).expect("loopback endpoint"),
        SecretString::new("tok").expect("non-empty"),
    )
    .with_plan_proof(proof())
}

fn presented_proof(headers: &HeaderMap) -> bool {
    headers
        .get("x-runvil-plan-proof")
        .and_then(|value| value.to_str().ok())
        == Some(PROOF)
}

fn ok_sse() -> &'static str {
    "data: {\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n\
     data: {\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
     data: [DONE]\n\n"
}

const CATALOG_BODY: &str = r#"{"data":[{"id":"mock-model","description":"the mock model","context":131072,"thinking":"never"}]}"#;

/// Accepts every connection, counts it, and closes. A client that honors
/// the proxy variable finishes instead of hanging on an unread request.
async fn proxy_listener() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            recorded.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    (format!("http://{addr}"), hits)
}

/// Reads the child's canary mark and returns how many proxy connections
/// existed at that moment. The ack is what lets the child send the
/// loopback request only after this count is stored.
async fn canary_hits(listener: tokio::net::TcpListener, hits: Arc<AtomicUsize>) -> usize {
    let (socket, _) = listener.accept().await.expect("the child marks the canary");
    let _ = socket.readable().await;
    let mut buf = [0_u8; 1];
    let _ = socket.try_read(&mut buf);
    let seen = hits.load(Ordering::SeqCst);
    let _ = socket.writable().await;
    let _ = socket.try_write(b"k");
    seen
}

fn run_child(role: &str, proxy: &str, signal: &str, test_name: &str) -> ExitStatus {
    Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", test_name, "--test-threads=1", "--nocapture"])
        .env(ROLE, role)
        .env(SIGNAL, signal)
        .env("HTTP_PROXY", proxy)
        .env("ALL_PROXY", proxy)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("REQUEST_METHOD")
        .status()
        .expect("the proxy child starts")
}

async fn prove_canary(origin: &str) {
    let client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .build()
        .expect("canary client");
    let finished = tokio::time::timeout(Duration::from_secs(5), client.get(origin).send()).await;
    assert!(
        finished.is_ok(),
        "the canary hung instead of finishing through HTTP_PROXY"
    );
}

fn wait_until_counted() {
    let addr = std::env::var(SIGNAL).expect("the parent publishes the canary signal");
    let mut stream = std::net::TcpStream::connect(addr).expect("canary signal");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
    stream.write_all(b"c").expect("canary signal");
    let mut buf = [0_u8; 1];
    stream.read_exact(&mut buf).expect("the canary was counted");
}

async fn drive_proxy_check(role: &str, test_name: &str) {
    let (proxy, hits) = proxy_listener().await;
    let gate = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gate_addr = gate.local_addr().unwrap().to_string();
    let recorded = Arc::clone(&hits);
    let marked = tokio::spawn(canary_hits(gate, recorded));
    let proxy_for_child = proxy.clone();
    let role = role.to_owned();
    let test_name = test_name.to_owned();
    let status = tokio::task::spawn_blocking(move || {
        run_child(&role, &proxy_for_child, &gate_addr, &test_name)
    })
    .await
    .expect("the child join");
    assert!(status.success(), "the proxy child exited {status}");
    let canary = marked.await.expect("the canary mark");
    assert!(
        canary >= 1,
        "HTTP_PROXY was not consulted, so the direct-client check proves nothing"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        canary,
        "the loopback client contacted the proxy"
    );
}

#[tokio::test]
async fn a_loopback_chat_does_not_follow_a_redirect() {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let (dest_base, dest_listener) = bind().await;
    // Count before routing. A followed 302 arrives as GET and would miss a
    // POST-only route, so the counter has to see every method.
    let count_every_request = move |request: Request, next: Next| {
        let recorded = Arc::clone(&recorded);
        async move {
            recorded.fetch_add(1, Ordering::SeqCst);
            let response: Response = next.run(request).await;
            response
        }
    };
    spawn(
        dest_listener,
        Router::new()
            .route("/chat/completions", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(count_every_request)),
    );
    let (origin, origin_listener) = bind().await;
    let location = format!("{dest_base}/chat/completions");
    spawn(
        origin_listener,
        Router::new().route(
            "/v1/chat/completions",
            post(move |headers: HeaderMap| {
                let location = location.clone();
                async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    // 307 keeps the method. A 302 that was followed would
                    // arrive at the target as GET and still be counted.
                    (StatusCode::TEMPORARY_REDIRECT, [("location", location)])
                }
            }),
        ),
    );
    let error = chat(&format!("{origin}/v1"))
        .complete(
            &[Message::user("hi")],
            None,
            &CompletionOptions::new("m"),
            |_| {},
        )
        .await
        .expect_err("a redirect is not a completed round");
    assert!(
        !error.to_string().contains(PROOF) && !error.message.contains(PROOF),
        "the redirect error contains the plan proof"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the redirect target received a request"
    );
}

#[tokio::test]
async fn a_loopback_catalog_does_not_follow_a_redirect() {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let (dest_base, dest_listener) = bind().await;
    spawn(
        dest_listener,
        Router::new().route(
            "/models",
            get(move || {
                let recorded = Arc::clone(&recorded);
                async move {
                    recorded.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        ),
    );
    let (origin, origin_listener) = bind().await;
    let location = format!("{dest_base}/models");
    spawn(
        origin_listener,
        Router::new().route(
            "/models",
            get(move |headers: HeaderMap| {
                let location = location.clone();
                async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    (StatusCode::FOUND, [("location", location)])
                }
            }),
        ),
    );
    let error = fetch_model_catalog_with_plan_proof(&origin, "tok", &proof())
        .await
        .expect_err("a redirect is not a catalog");
    assert!(
        !error.to_string().contains(PROOF) && !error.message.contains(PROOF),
        "the redirect error contains the plan proof"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_loopback_chat_does_not_use_an_http_proxy() {
    if std::env::var(ROLE).ok().as_deref() == Some("chat") {
        let (base, listener) = bind().await;
        spawn(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(move |headers: HeaderMap| async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        ok_sse(),
                    )
                }),
            ),
        );
        prove_canary(&format!("{base}/v1/chat/completions")).await;
        wait_until_counted();
        chat(&format!("{base}/v1"))
            .complete(
                &[Message::user("hi")],
                None,
                &CompletionOptions::new("m"),
                |_| {},
            )
            .await
            .expect("the loopback round completes directly");
        return;
    }
    drive_proxy_check("chat", "a_loopback_chat_does_not_use_an_http_proxy").await;
}

#[tokio::test]
async fn a_loopback_catalog_does_not_use_an_http_proxy() {
    if std::env::var(ROLE).ok().as_deref() == Some("catalog") {
        let (base, listener) = bind().await;
        spawn(
            listener,
            Router::new().route(
                "/models",
                get(move |headers: HeaderMap| async move {
                    assert!(
                        presented_proof(&headers),
                        "the direct request arrived without the plan proof"
                    );
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        CATALOG_BODY,
                    )
                }),
            ),
        );
        prove_canary(&format!("{base}/models")).await;
        wait_until_counted();
        fetch_model_catalog_with_plan_proof(&base, "tok", &proof())
            .await
            .expect("the loopback catalog completes directly");
        return;
    }
    drive_proxy_check("catalog", "a_loopback_catalog_does_not_use_an_http_proxy").await;
}

const SEARCH_BODY: &str = r#"{"query":"hi","results":[]}"#;

/// An API root that names the local machine but is not a bare root is
/// refused before any request. It must never reach the client that follows
/// redirects and uses the process proxy, and its error names the rule, not
/// the input.
#[tokio::test]
async fn an_invalid_loopback_catalog_root_is_refused_before_any_request() {
    const SECRET: &str = "synthetic-credential-4f1c";
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let (base, listener) = bind().await;
    spawn(
        listener,
        Router::new().fallback(move || {
            let recorded = Arc::clone(&recorded);
            async move {
                recorded.fetch_add(1, Ordering::SeqCst);
                StatusCode::OK
            }
        }),
    );
    let authority = base.trim_start_matches("http://");
    let roots = [
        format!("{base}#{SECRET}"),
        format!("{base}?key={SECRET}"),
        format!("http://user:{SECRET}@{authority}"),
    ];
    for root in roots {
        let error = fetch_model_catalog_with_plan_proof(&root, "tok", &proof())
            .await
            .expect_err("an API root with a query, fragment or credentials is refused");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        let shown = format!("{error} {} {error:?}", error.message);
        assert!(
            !shown.contains(SECRET) && !shown.contains(PROOF),
            "the refusal repeats part of the input or the plan proof"
        );
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a refused root was requested"
    );
}

fn search(base: &str) -> GatewaySearch {
    GatewaySearch::new(
        GatewayEndpoint::new(base).expect("loopback endpoint"),
        SecretString::new(SEARCH_KEY).expect("non-empty"),
    )
}

fn search_query() -> SearchQuery {
    SearchQuery {
        query: "hi".to_owned(),
        ..SearchQuery::default()
    }
}

fn presented_search_key(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {SEARCH_KEY}"))
}

#[tokio::test]
async fn a_loopback_search_does_not_follow_a_redirect() {
    let hits = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&hits);
    let (dest_base, dest_listener) = bind().await;
    let count_every_request = move |request: Request, next: Next| {
        let recorded = Arc::clone(&recorded);
        async move {
            recorded.fetch_add(1, Ordering::SeqCst);
            let response: Response = next.run(request).await;
            response
        }
    };
    spawn(
        dest_listener,
        Router::new()
            .route("/tools/web_search", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(count_every_request)),
    );
    let (origin, origin_listener) = bind().await;
    let location = format!("{dest_base}/tools/web_search");
    spawn(
        origin_listener,
        Router::new().route(
            "/v1/tools/web_search",
            post(move |headers: HeaderMap| {
                let location = location.clone();
                async move {
                    assert!(
                        presented_search_key(&headers),
                        "the direct request arrived without the search bearer key"
                    );
                    (StatusCode::TEMPORARY_REDIRECT, [("location", location)])
                }
            }),
        ),
    );
    let error = SearchProvider::search(&search(&format!("{origin}/v1")), search_query())
        .await
        .expect_err("a redirect is not a completed search");
    let source_text = std::error::Error::source(&error)
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        !error.to_string().contains(SEARCH_KEY) && !source_text.contains(SEARCH_KEY),
        "the redirect error contains the search bearer key"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the redirect target received a request"
    );
}

#[tokio::test]
async fn a_loopback_search_does_not_use_an_http_proxy() {
    if std::env::var(ROLE).ok().as_deref() == Some("search") {
        let (base, listener) = bind().await;
        spawn(
            listener,
            Router::new().route(
                "/v1/tools/web_search",
                post(move |headers: HeaderMap| async move {
                    assert!(
                        presented_search_key(&headers),
                        "the direct request arrived without the search bearer key"
                    );
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        SEARCH_BODY,
                    )
                }),
            ),
        );
        prove_canary(&format!("{base}/v1/tools/web_search")).await;
        wait_until_counted();
        SearchProvider::search(&search(&format!("{base}/v1")), search_query())
            .await
            .expect("the loopback search completes directly");
        return;
    }
    drive_proxy_check("search", "a_loopback_search_does_not_use_an_http_proxy").await;
}
