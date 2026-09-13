//! Compile-level proof that the facade entry paths resolve.

#[test]
fn pipeline_paths_resolve() {
    use promptforge::pipeline::{RunConfig, RunError, run as pipeline_run};

    // Function items and types must resolve; nothing here is executed for effect.
    let _ = std::mem::size_of_val(&pipeline_run);
    let _: Option<RunConfig> = None;
    let _: Option<RunError> = None;
}
