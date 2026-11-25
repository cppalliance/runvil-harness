//! The mock-gateway helpers the broker's tests share: a loopback server, an
//! SSE answer, and the chunks it streams.

use serde_json::Value;

/// Serves `app` on a loopback port and returns its `/v1` root.
pub(crate) async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/v1")
}

/// Renders `events` as SSE `data:` lines closed by the `[DONE]` sentinel.
pub(crate) fn sse_body(events: &[Value]) -> String {
    let mut body = String::new();
    for event in events {
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// A mock gateway that answers every completion with the given SSE body.
pub(crate) fn sse_app(body: String) -> axum::Router {
    use axum::Router;
    use axum::routing::post;

    Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let body = body.clone();
            async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
            }
        }),
    )
}

/// One streamed chunk with a content fragment.
pub(crate) fn content_chunk(text: &str) -> Value {
    serde_json::json!({
        "model": "qwen3-30b",
        "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }]
    })
}
