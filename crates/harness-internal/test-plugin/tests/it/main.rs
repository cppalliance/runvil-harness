//! End-to-end tests for the loader against the fixture's real dynamic
//! library: `insert_plugin_dir` opens it, loads its Plugin, inserts its
//! Pack, and closes it again, the builder of a Host loads it from a
//! directory, from its file, and through its entry point, and a prompt
//! calls its tools through the Harness.
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]
#![expect(
    unsafe_code,
    reason = "reads the fixture's counters and its entry point through symbols of the fixture \
              library, and opens a copy of it as a linked Plugin's code"
)]

mod host;
mod insert;
mod lifecycle;
mod run;
mod support;
