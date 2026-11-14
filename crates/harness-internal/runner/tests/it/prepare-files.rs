//! The prompt's declared output file: the report carries what a completed
//! run left at the frontmatter's `output:` path, read through the store
//! view wherever the handle mounts the store; a file the Host seeds in
//! the store is the run's to read; and a prompt that declares no output
//! reports `NoOutput`.

use harness_runner::files::read_output;
use harness_runner::{RunReport, RunSetup};
use runvil_engine::vfs::{MemoryBackend, Origin, VfsRef};
use runvil_engine::{ErrorCode, ErrorFact};
use serde_json::json;

use super::{PLAIN, bare, completed, recorder, report_of, setup_over};

/// A prompt declaring `report.md` as its output, whose one section writes
/// it from the `paper.md` it finds in the store.
const FILES: &str = "---\nname: files\ndescription: d\nrunvil: 0\n\
    output:\n  path: report.md\n  description: The report\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('report.md', 'seen: ' .. store.read('paper.md'))\nreturn 'done'\n```\n";

/// Writes `text` at `paper.md` in `vfs`'s store, as a Host seeds a store
/// before a run.
fn seed_paper(vfs: &VfsRef, text: &str) {
    vfs.acquire_store(Origin::new("prepare files test"))
        .unwrap()
        .write("paper.md", text.as_bytes())
        .unwrap();
}

/// Runs `source` over `vfs` and returns the run's report.
async fn run_over(source: &str, vfs: &VfsRef) -> RunReport {
    let setup = RunSetup {
        vfs: vfs.clone(),
        ..setup_over(&recorder())
    };
    report_of(source, bare(), setup).await
}

#[tokio::test]
async fn a_completed_run_reports_what_it_left_at_its_declared_output() {
    let vfs = VfsRef::default();
    seed_paper(&vfs, "# Paper");
    let report = run_over(FILES, &vfs).await;
    assert_eq!(completed(report.result), "done");
    assert_eq!(report.output, Ok("seen: # Paper".to_owned()));
    assert_eq!(read_output(&vfs, "report.md").unwrap(), "seen: # Paper");
}

#[tokio::test]
async fn a_store_mounted_away_from_the_root_is_read_by_its_logical_path() {
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .store("/store", MemoryBackend::new())
        .build();
    seed_paper(&vfs, "# Paper");
    let report = run_over(FILES, &vfs).await;
    completed(report.result);
    assert_eq!(report.output, Ok("seen: # Paper".to_owned()));
    let access = vfs.acquire(Origin::new("prepare files test")).unwrap();
    assert_eq!(access.read("/store/report.md").unwrap(), b"seen: # Paper");
    assert!(
        !access.exists("/report.md").unwrap(),
        "the base never saw it"
    );
}

#[tokio::test]
async fn a_file_the_host_seeds_in_the_store_is_the_runs_to_read() {
    let vfs = VfsRef::default();
    seed_paper(&vfs, "# Seeded");
    let report = run_over(FILES, &vfs).await;
    assert_eq!(report.output, Ok("seen: # Seeded".to_owned()));
}

#[tokio::test]
async fn a_prompt_without_an_output_declaration_reports_no_output() {
    let report = run_over(PLAIN, &VfsRef::default()).await;
    completed(report.result);
    let error = report.output.expect_err("an undeclared output has no text");
    assert_eq!(error.code, ErrorCode::NoOutput);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&json!("undeclared"))
    );
}
