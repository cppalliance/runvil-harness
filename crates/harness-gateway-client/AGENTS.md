# harness-gateway-client

- Conversions between `gateway-client` types and Engine types live in `convert.rs`, through the traits of that file. No code here shapes a request body, because the client's `build_request_body` is the one place that does.
- Every `Completion` and `ToolCall` is built through the Engine's constructors, so the Engine's checks run on each reply.
- `expose_secret` is called in `resources.rs` only. The broker passes the key to the client as a `SecretString` and never reads it.
