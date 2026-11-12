[![CI](https://github.com/cppalliance/runvil-harness/actions/workflows/ci.yml/badge.svg)](https://github.com/cppalliance/runvil-harness/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-BSL--1.0-blue.svg)](LICENSE)

# Runvil Engine and Harness

The Rust libraries that run [Runvil](https://github.com/cppalliance/runvil) prompts. A Runvil prompt is one Markdown file: prose for the model and Lua for the logic.

The Engine parses a prompt and steps through it. Whenever the run needs a model reply, a tool's result, a timer, or a file, the Engine asks its caller for one and waits. The Harness answers each of those requests and records the run. A Host is any application that runs prompts through the Harness.

## Crates

| Crate | What it is |
|---|---|
| `runvil` | The Harness, and the one crate a Host depends on. It also re-exports the Engine items a Host reads and the Plugin contract. |
| `harness-gateway-client` | The standard client for the [Runvil Gateway](https://github.com/cppalliance/runvil-gateway): the inference broker that runs each model round, the model list, and the web search provider. |
| `plugin-web` | A Plugin a Host links in. Its Pack, `runvil/web`, has web search and web fetch. |
| `plugin-mcp` | A Plugin a Host links in. Its Pack, `runvil/mcp`, reaches the tools of remote MCP servers. |
| `runvil-engine` | The Engine's public API, for code that steps a run itself. |
| `runvil-plugin` | The Plugin contract that a Rust Plugin crate is written against. |

The Engine's private crates are under `crates/runvil-internal/`, and the Harness's are under `crates/harness-internal/`. `crates/build-xtask` holds the structural checks, and `prompts/` holds sample prompts.

## Using it from a Host

No crate here is published to crates.io. A Host names the crates it uses by git, all at one commit:

```toml
[dependencies]
runvil = { version = "0.3.0", git = "https://github.com/cppalliance/runvil-harness", rev = "<commit>" }
harness-gateway-client = { version = "0.3.0", git = "https://github.com/cppalliance/runvil-harness", rev = "<commit>" }
```

The Harness loads library Plugins through the C contract in [runvil-plugin-api](https://github.com/cppalliance/runvil-plugin-api), which this workspace pins by commit as `runvil-plugin-sys`. That crate sets `links`, so Cargo refuses two commits of the contract in one build. A Host whose build also names `runvil-plugin-sdk`, or a Plugin built on it, pins that same commit.

Two examples show the whole path in one file each:

- `cargo run -p runvil --example run-prompt` runs one prompt through the Harness, as a Host does.
- `cargo run -p runvil-engine --example greeter` steps a run directly on the Engine.

## Contributing

Every change must pass the formatting, lint, and test checks that CI runs. [AGENTS.md](AGENTS.md) explains the project's terms and conventions and how to run those checks locally.

## License

Distributed under the [Boost Software License 1.0](LICENSE).
