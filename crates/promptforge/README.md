# promptforge

Integrator-facing facade for the PromptForge library product. One entry point, no logic of its own.

## Entry points

```rust
// Document prompts (.md): sections, prose, the built-in tool loop.
use promptforge::pipeline::{run, RunConfig, RunError};
```

Substrate types (parser, store, tools, models) come from their own crates - this package depends only on `promptforge-core`.
