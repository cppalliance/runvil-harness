# runvil-vfs

- The machinery modules hold no run concepts. A `Store` is generic machinery (an empty memory backend under its own claims ledger, which any access's identity can view), so it lives with the handle; the mode gate stays at the crate root.
- Origin labels are the most specific available: a section name for a chain, a tool id for a tool, a fixture name for a test.
