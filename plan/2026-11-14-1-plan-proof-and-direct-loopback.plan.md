---
name: Plan Proof And Direct Loopback
overview: "`harness-gateway-client` sends a plan proof that a Host passes to it, as `X-Runvil-Plan-Proof` on model rounds and on the model list, and it contacts a Gateway on the local machine with no proxy and without following a redirect."
todos:
  - id: plan-proof
    content: "harness-gateway-client: `PLAN_PROOF_HEADER`, `with_plan_proof` on `GatewayChat` and `GatewayBroker`, `fetch_model_catalog_with_plan_proof`; header and redaction tests"
    status: completed
  - id: loopback
    content: "harness-gateway-client: `transport::http_client`, used by chat, the catalog, and search; the catalog validates its API root; `tests/loopback_direct.rs`"
    status: completed
  - id: verify
    content: "runvil-harness: run every check in AGENTS.md on each commit"
    status: completed
isProject: false
---

# A Host-supplied plan proof, and a direct path to a loopback Gateway

## Why

The ChatGPT plan extension proposed for `runvil-gateway` serves a plan model only to a caller that presents the plan proof of the Gateway's current boot. The Gateway writes that proof to a private file, and a Host on the same machine reads it. This crate is the Host's client, so it has to carry the proof. It could not.

The second gap was there before. Every client in the crate was `reqwest::Client::new()`. That client uses the process's proxy settings and follows redirects. So a request to a Gateway on the local machine could leave the machine through `HTTP_PROXY` or `ALL_PROXY`, or be sent on by a redirect. A followed redirect drops `Authorization` when the host changes, but it keeps any other header. A proof header makes that worth closing.

## The rule

- **A Host passes the proof.** The crate reads no file and no environment variable to find one. `GatewayChat::from_env` still reads only the URL and the key.
- **No proof, no header.** A client that was given no proof sends the requests it sent before.
- **The proof is not a bearer.** `Authorization` is unchanged, with or without a key.
- **It goes on a model round and on the model list, and nowhere else.** `GatewaySearch` sends none.
- **It is redacted like the key.** The value is a `SecretString`, and `Debug` prints `<redacted>` for it on `GatewayChat` and on `GatewayBroker`.
- **A loopback Gateway is contacted directly.** No proxy is used, and a redirect comes back as its own status.
- **Any other address keeps reqwest's defaults**, a proxy and redirects included.
- **The catalog validates its API root** the way a chat endpoint is validated, before any request.

Decisions:

- **The proof is sent to whatever Gateway the Host configured**, loopback or not. Whether to pass it is the Host's decision, and a Host that reads the proof from a local file has a local Gateway.
- **A loopback redirect is an error, not a hop.** A Gateway does not redirect its API, so a redirect there is a fault or an attack.
- **A loopback client that cannot be built panics.** Falling back to the default client would bring the proxy and the redirects back without a sign. `Client::new` already panics on the same TLS failure.
- **No change to `runvil` or to `build_request_body`.** The proof is a header, so the chat-completions body is still shaped in one place.

## Commit 1: the plan proof

- [transport.rs](../crates/harness-gateway-client/src/transport.rs): the public constant `PLAN_PROOF_HEADER`, re-exported from the crate root; the `plan_proof` field on `GatewayChat`; `GatewayChat::with_plan_proof`; and the header on the one request that `complete` sends.
- [broker.rs](../crates/harness-gateway-client/src/broker.rs): `GatewayBroker::with_plan_proof` gives the proof to its `GatewayChat` and keeps a copy for `models`.
- [catalog.rs](../crates/harness-gateway-client/src/catalog.rs): `fetch_model_catalog_with_plan_proof(base_url, token, plan_proof)` beside `fetch_model_catalog(base_url, token)`. Both call the private `fetch_catalog`, which sets the header on the single request when a proof was passed.
- [config.rs](../crates/harness-gateway-client/src/config.rs) and [lib.rs](../crates/harness-gateway-client/src/lib.rs): the docs of `SecretString` and of the crate say what a plan proof is and where it is sent.

## Commit 2: the direct loopback path

- [transport.rs](../crates/harness-gateway-client/src/transport.rs): `http_client(loopback)` builds the client. For a loopback endpoint it sets `no_proxy()` and `redirect::Policy::none()`. `GatewayChat::new` and `GatewayChat::keyless` call it with `GatewayEndpoint::is_loopback`.
- [search.rs](../crates/harness-gateway-client/src/search.rs): `GatewaySearch` calls it the same way.
- [catalog.rs](../crates/harness-gateway-client/src/catalog.rs): `catalog_client(loopback)` keeps two process-wide clients, one for a loopback Gateway and one for any other, so a remote catalog fetch still follows a redirect and uses a proxy. `fetch_catalog` builds a `GatewayEndpoint` from `base_url` first.

One behavior change comes with that last point. `fetch_model_catalog` took any string and sent it. It now returns `InvalidValue` for a root that `GatewayEndpoint::new` refuses: not http or https, no host, embedded credentials, a query, or a fragment. A root such as `http://127.0.0.1:8081/v1?x` is refused, not classified, so it cannot be sent with the client that uses a proxy and follows redirects.

## Tests

- **The header.** `a_passed_proof_is_one_header_on_the_round_and_the_bearer_stays`, `a_keyless_client_with_a_proof_omits_authorization_and_sends_the_header`, and `from_env_sends_no_plan_proof_header` in `transport/tests/plan_proof.rs`; `models_sends_the_plan_proof_only_when_the_host_passed_one` in `broker-tests.rs`.
- **Redaction.** `debug_redacts_the_plan_proof_whether_or_not_one_was_passed` and `debug_redacts_a_plan_proof_the_same_way_it_redacts_the_key`.
- **No redirect.** In [loopback_direct.rs](../crates/harness-gateway-client/tests/loopback_direct.rs), a chat round, a catalog fetch, and a search each get a redirect from a loopback Gateway, and the target must receive nothing. For the chat round and the search the target counts every method, because a followed redirect can change the method. The error must not contain the proof, or the key for the search.
- **No proxy.** The same three run in a child process with `HTTP_PROXY` and `ALL_PROXY` set to a listener that counts connections. A plain reqwest client is sent through first as a canary. The test fails if the canary did not reach the listener, and fails if anything after it did.
- **The invalid root.** `an_invalid_loopback_catalog_root_is_refused_before_any_request` gives the catalog a loopback root with a query, with a fragment, and with credentials. Each returns `InvalidValue`, and a catch-all fixture receives nothing.

The redirect, proxy, and invalid-root tests were each checked by removing the line they guard and watching them fail.

## Left alone

- **Error text.** A proof is never formatted into an error by this crate. The crate still keeps a bounded, control-escaped piece of a backend's error body, as it did, so a backend that echoes a header would put it there.
- **HTTPS proxies and TLS.** The fixtures are plain HTTP.
- **`GatewayChat::from_env`.** It gains no variable for the proof.

## Verification

Every command in [AGENTS.md](../AGENTS.md) on each commit: format, clippy, nextest, both doc builds, `cargo test -p build-xtask`, the wording check, `xtask api --check`, and dylint. Also `cargo deny check` and `cargo audit`.

## What the build found

- **The public API listings are unchanged.** `xtask api --check` reports no violation, and neither `public-api.txt` lists this crate.
- **`reqwest` is now a dev-dependency too.** The integration test is its own crate and needs a client for the canary.
- **One test fails on macOS, with and without this change.** `lifecycle::the_fixture_is_loaded_invoked_unloaded_once_and_closed` in `harness-test-plugin` fails the same way on `9dd953b5`. The other 3,647 tests pass.
- **Run on macOS arm64 only.** The Windows job was not run.
