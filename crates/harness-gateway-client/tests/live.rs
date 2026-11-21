//! A Host other than Workshop reaches a real Gateway: the shipped
//! `hello.md` runs through the Harness, with this crate's broker, against
//! the Gateway the environment names.
//!
//! The tests are ignored unless asked for, because they need a running
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
//! key from any caller. `RUNVIL_E2E_MODEL` selects a model by name for the
//! first test; without it the run takes the first model the Gateway lists.
//!
//! The second test needs a Gateway that serves a local chat model and a
//! cloud chat model at once. It names neither: it reads which is which
//! from the settings the Gateway serves at `/admin/config`.

use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_gateway_client::{GatewayBroker, GatewayEndpoint, SecretString, TokioTimer};
use runvil::harness::{FindTextPrompt, MemoryRecorder};
use runvil::{Harness, Host, RunSetup};
use serde_json::Value;

/// The name the Host finds the shipped prompt by.
const HELLO: &str = "hello.md";

/// The Gateway the environment names.
struct Gateway {
    /// Its API root, from `RUNVIL_E2E_GATEWAY`.
    root: String,
    /// Its key, from `RUNVIL_E2E_KEY`.
    key: String,
}

/// A local chat model and a cloud chat model that one Gateway serves.
struct LocalAndCloud {
    /// The name of a chat model the Gateway runs on its own machine.
    local: String,
    /// The name of a chat model whose endpoint is not on the Gateway's
    /// machine.
    cloud: String,
}

impl Gateway {
    /// Reads the two variables.
    fn from_env() -> Result<Gateway, Box<dyn Error>> {
        Ok(Gateway {
            root: env::var("RUNVIL_E2E_GATEWAY").map_err(|_| "RUNVIL_E2E_GATEWAY is not set")?,
            key: env::var("RUNVIL_E2E_KEY").map_err(|_| "RUNVIL_E2E_KEY is not set")?,
        })
    }

    /// Runs the shipped `hello.md` through the Harness, with this crate's
    /// broker, against this Gateway, and returns the text the run ends
    /// with. `selected` is the model the Host selects; with `None` the run
    /// takes the first model the Gateway lists.
    async fn hello(&self, selected: Option<String>) -> Result<String, Box<dyn Error>> {
        let broker = GatewayBroker::new(
            GatewayEndpoint::new(&self.root)?,
            SecretString::new(self.key.as_str())?,
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../prompts")
            .join(HELLO);
        let hello =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let host = Host::builder(FindTextPrompt::new(HELLO, hello)).build();
        let prompt = host.parse(HELLO)?;
        let mut setup = RunSetup::new(
            Arc::new(MemoryRecorder::new()),
            Arc::new(broker),
            Arc::new(TokioTimer),
        );
        setup.selected_model = selected;
        let report = Harness::new(host, prompt, setup).run().await?;
        Ok(report.result?)
    }

    /// Finds a local chat model and a cloud chat model in the settings
    /// this Gateway serves at `/admin/config`. A cloud model is one whose
    /// endpoint is not on the Gateway's machine.
    async fn a_local_and_a_cloud_model(&self) -> Result<LocalAndCloud, Box<dyn Error>> {
        let origin = self.root.trim_end_matches('/').trim_end_matches("/v1");
        let settings: Value = reqwest::Client::new()
            .get(format!("{origin}/admin/config"))
            .bearer_auth(&self.key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let listed = |section: &str| settings[section].as_array().into_iter().flatten();
        let chat = |model: &&Value| model["kind"] == "chat";
        // An endpoint off the machine, by the same rule this crate
        // applies to the Gateway's own address.
        let off_the_machine = |endpoint: &&Value| {
            endpoint["base_url"]
                .as_str()
                .and_then(|url| GatewayEndpoint::new(url).ok())
                .is_some_and(|endpoint| !endpoint.is_loopback())
        };
        let cloud_endpoints: Vec<&Value> = listed("endpoint")
            .filter(off_the_machine)
            .map(|endpoint| &endpoint["id"])
            .collect();
        let local = listed("local_model")
            .find(chat)
            .and_then(|model| model["name"].as_str())
            .ok_or("the Gateway's settings name no local chat model")?;
        // A model is served by the first endpoint it names.
        let cloud = listed("model")
            .filter(chat)
            .find(|model| cloud_endpoints.contains(&&model["endpoints"][0]))
            .and_then(|model| model["name"].as_str())
            .ok_or("the Gateway's settings name no chat model whose endpoint is off its machine")?;
        Ok(LocalAndCloud {
            local: local.to_owned(),
            cloud: cloud.to_owned(),
        })
    }
}

#[tokio::test]
#[ignore = "needs a running Gateway with a chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn a_host_sends_the_shipped_hello_prompt_through_a_real_gateway() {
    let gateway = Gateway::from_env().expect("the environment names a Gateway and its key");

    let text = gateway
        .hello(env::var("RUNVIL_E2E_MODEL").ok())
        .await
        .expect("the run ends with text, not an error");

    assert!(
        !text.trim().is_empty(),
        "the run's text is the model's reply, which is not empty"
    );
}

#[tokio::test]
#[ignore = "needs a running Gateway with a local chat model and a cloud chat model; set RUNVIL_E2E_GATEWAY and RUNVIL_E2E_KEY"]
async fn the_shipped_hello_prompt_ends_with_text_on_a_local_model_and_on_a_cloud_model() {
    let gateway = Gateway::from_env().expect("the environment names a Gateway and its key");
    let models = gateway
        .a_local_and_a_cloud_model()
        .await
        .expect("the Gateway serves a local chat model and a cloud chat model");

    for (place, model) in [("local", models.local), ("cloud", models.cloud)] {
        let text = gateway
            .hello(Some(model.clone()))
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "the run on the {place} model `{model}` ends with text, not an error: {error}"
                )
            });
        assert!(
            !text.trim().is_empty(),
            "the run's text on the {place} model `{model}` is the model's reply, which is not empty"
        );
    }
}
