//! Tests for the catalog rules against an axum mock gateway: the entry
//! filter, and failure prose that holds no Gateway-supplied text. The
//! client's own tests hold the body bounds and the fetch's causes.

use gateway_client::{SecretString, fetch_models};
use runvil::ErrorCode;
use runvil::model::ModelCatalog;

use super::*;

/// The text the fetch appended to `err`'s message after its prose: the
/// provider detail or the cause, which the prose never holds.
fn appended(err: &Error) -> &str {
    err.message
        .strip_prefix(err.prose.as_str())
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or_else(|| panic!("the message extends the prose: {err:?}"))
}

async fn spawn_models(app: axum::Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

/// What the broker's `models` does: the client fetches the list, and the
/// catalog rules turn it into the Engine's catalog.
async fn fetch_model_catalog(base_url: &str, token: &str) -> Result<ModelCatalog, Error> {
    let key = SecretString::new(token).expect("a key");
    let models = fetch_models(base_url, &key)
        .await
        .map_err(IntoEngine::into_engine)?;
    catalog_from(models)
}

#[tokio::test]
async fn fetch_model_catalog_skips_entries_without_a_context_window() {
    use axum::Router;
    use axum::routing::get;

    // A gateway with speech-to-text lists its transcription models beside
    // the inference models, and those entries omit `context` and
    // `thinking` (they answer no completion request). The fetch must keep
    // the inference models' `ModelInfo` values instead of rejecting the whole
    // list, otherwise the Harness binds each run on such a gateway to a
    // fallback model info and the context precheck refuses real conversations.
    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "chat-model", "object": "model", "kind": "chat",
                  "description": "a chat model", "context": 1_000_000, "thinking": "never" },
                { "id": "whisper-base-en", "object": "model", "kind": "transcription" },
                { "id": "whisper-small-en", "object": "model", "kind": "transcription" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let catalog = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect("transcription entries must not fail the inference catalog");
    assert_eq!(
        catalog.models().len(),
        1,
        "only the inference model gets a model info"
    );
    let chat = catalog
        .get(&ModelId::gateway("chat-model").expect("valid id"))
        .expect("the inference model survives the filter");
    assert_eq!(
        chat.context(),
        NonZeroU32::new(1_000_000).expect("non-zero")
    );
    assert_eq!(chat.thinking(), ThinkingMode::Never);
}

#[tokio::test]
async fn fetch_model_catalog_reads_a_provider_only_when_the_entry_names_one() {
    use axum::Router;
    use axum::routing::get;

    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "grok", "object": "model", "kind": "chat", "description": "d",
                  "context": 4096, "thinking": "never", "provider": "xai" },
                { "id": "local", "object": "model", "kind": "chat", "description": "d",
                  "context": 4096, "thinking": "never" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let catalog = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect("an optional provider decodes");
    let provider = |name: &str| {
        catalog
            .get(&ModelId::gateway(name).expect("valid id"))
            .expect("the model is in the catalog")
            .provider()
            .map(str::to_owned)
    };
    assert_eq!(provider("grok").as_deref(), Some("xai"));
    assert_eq!(provider("local"), None);
}

#[tokio::test]
async fn fetch_model_catalog_still_rejects_a_zero_context_window() {
    use axum::Router;
    use axum::routing::get;

    // Skipping applies only to entries with no context field at all; an
    // inference entry that declares a zero window is still malformed.
    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "broken", "object": "model", "kind": "chat",
                  "description": "d", "context": 0, "thinking": "never" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("a zero context window is malformed");
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(err.prose.contains("zero-token"), "got {err:?}");
    assert!(!err.prose.contains("broken"), "got {err:?}");
    assert!(
        appended(&err).contains("broken"),
        "the model name belongs in the message only, got {err:?}"
    );
}

/// Serves `data` as the gateway model list and returns the fetch failure.
async fn fetch_rejection(data: serde_json::Value) -> Error {
    let list = serde_json::json!({ "object": "list", "data": data });
    let app = axum::Router::new().route(
        "/models",
        axum::routing::get(move || async move { axum::Json(list) }),
    );
    let addr = spawn_models(app).await;
    fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("the model list is malformed")
}

#[tokio::test]
async fn fetch_model_catalog_keeps_the_name_of_a_model_without_a_thinking_mode_out_of_the_prose() {
    let err = fetch_rejection(serde_json::json!([
        { "id": "thoughtless", "object": "model", "kind": "chat",
          "description": "d", "context": 4096 }
    ]))
    .await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(!err.prose.contains("thoughtless"), "got {err:?}");
    assert!(
        appended(&err).contains("thoughtless"),
        "the model name belongs in the message only, got {err:?}"
    );
}

#[tokio::test]
async fn fetch_model_catalog_keeps_a_duplicate_id_out_of_the_prose() {
    let entry = serde_json::json!(
        { "id": "twin", "object": "model", "kind": "chat",
          "description": "d", "context": 4096, "thinking": "never" }
    );
    let err = fetch_rejection(serde_json::json!([entry.clone(), entry])).await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(!err.prose.contains("twin"), "got {err:?}");
    assert!(
        appended(&err).contains("gateway/twin"),
        "the duplicate identity belongs in the message only, got {err:?}"
    );
}

#[tokio::test]
async fn fetch_model_catalog_bounds_a_gateway_supplied_detail() {
    let long = "n".repeat(MAX_CATALOG_ERROR_BODY * 4);
    let entry = |context: u32, thinking: Option<&str>| {
        serde_json::json!({ "id": long, "object": "model", "kind": "chat",
            "description": "d", "context": context, "thinking": thinking })
    };
    let rejections = [
        serde_json::json!([entry(0, Some("never"))]),
        serde_json::json!([entry(4096, None)]),
        serde_json::json!([entry(4096, Some("never")), entry(4096, Some("never"))]),
    ];
    for data in rejections {
        let err = fetch_rejection(data).await;
        assert_eq!(err.code, ErrorCode::MalformedResponse);
        let detail = appended(&err);
        assert!(
            detail.chars().count() <= MAX_CATALOG_ERROR_BODY,
            "a Gateway-supplied detail must be bounded, got {} chars",
            detail.chars().count()
        );
    }
}

#[tokio::test]
async fn fetch_model_catalog_keeps_an_invalid_id_out_of_the_message() {
    let err = fetch_rejection(serde_json::json!([
        { "id": "smuggled\u{7}id", "object": "model", "kind": "chat",
          "description": "d", "context": 4096, "thinking": "never" }
    ]))
    .await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    let message = err.to_string();
    assert!(!message.contains("smuggled"), "got {err:?}");
    assert!(!message.contains('\u{7}'), "got {err:?}");
}
