//! A Host other than Workshop reaches a real Gateway: the shipped
//! `hello.md` runs through the Harness, with this crate's broker, against
//! the Gateway the environment names.
//!
//! The test is ignored unless asked for, because it needs a running
//! Gateway that serves a chat model:
//!
//! ```text
//! RUNVIL_E2E_GATEWAY=http://127.0.0.1:8189/v1 RUNVIL_E2E_KEY=<key> \
//!     cargo nextest run -p harness-gateway-client --test live --run-ignored ignored-only
//! ```
//!
//! `RUNVIL_E2E_GATEWAY` is the Gateway's API root and `RUNVIL_E2E_KEY` is
//! its key. The key is needed even where the Gateway trusts loopback
//! callers: the broker always sends one, and the Gateway refuses a wrong
//! key from any caller. `RUNVIL_E2E_MODEL` selects a model by name;
//! without it the run takes the first model the Gateway lists.

use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_gateway_client::{GatewayBroker, GatewayEndpoint, SecretString, TokioTimer};
use runvil::harness::{FindTextPrompt, MemoryRecorder};
use runvil::{Harness, Host, RunSetup};

/// The name the Host finds the shipped prompt by.
const HELLO: &str = "hello.md";

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_host_sends_the_shipped_hello_prompt_through_a_real_gateway() {
    let root = env::var("RUNVIL_E2E_GATEWAY").expect("RUNVIL_E2E_GATEWAY is the API root");
    let key = env::var("RUNVIL_E2E_KEY").expect("RUNVIL_E2E_KEY is the Gateway's key");
    let broker = GatewayBroker::new(
        GatewayEndpoint::new(&root).expect("RUNVIL_E2E_GATEWAY is a valid API root"),
        SecretString::new(key).expect("RUNVIL_E2E_KEY is not empty"),
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../prompts")
        .join(HELLO);
    let hello = fs::read_to_string(&path).expect("the shipped hello prompt is readable");
    let host = Host::builder(FindTextPrompt::new(HELLO, hello)).build();
    let prompt = host.parse(HELLO).expect("the shipped hello prompt parses");
    let mut setup = RunSetup::new(
        Arc::new(MemoryRecorder::new()),
        Arc::new(broker),
        Arc::new(TokioTimer),
    );
    setup.selected_model = env::var("RUNVIL_E2E_MODEL").ok();

    let report = Harness::new(host, prompt, setup)
        .run()
        .await
        .expect("the run is reported");

    let text = report.result.expect("the run ends with text, not an error");
    assert!(
        !text.trim().is_empty(),
        "the run's text is the model's reply, which is not empty"
    );
}
