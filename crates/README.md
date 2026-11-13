# Crates

This directory holds the workspace's crates. The crates at this level are the public layer: the Engine's public API, the Rust Plugin API, the Harness and the pieces a Host plugs into it, and the structural checks. Each product family keeps its private crates in a container directory of its own, and code in one family reaches another family only through a crate at this level.
