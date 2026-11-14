//! The files a Host seeds in the store: a seeded file is the run's to
//! read, wherever the handle mounts the store.

use harness_runner::{RunReport, RunSetup};
use runvil_engine::vfs::{MemoryBackend, Origin, VfsRef};

use super::{bare, completed, recorder, report_of, setup_over};

/// A prompt whose one section writes `report.md` from the `paper.md` it
/// finds in the store, and returns the report.
const FILES: &str = "---\nname: files\ndescription: d\nrunvil: 0\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('report.md', 'seen: ' .. store.read('paper.md'))\n\
    return store.read('report.md')\n```\n";

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
async fn a_store_mounted_away_from_the_root_is_read_by_its_logical_path() {
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .store("/store", MemoryBackend::new())
        .build();
    seed_paper(&vfs, "# Paper");
    let report = run_over(FILES, &vfs).await;
    assert_eq!(completed(report.result), "seen: # Paper");
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
    assert_eq!(completed(report.result), "seen: # Seeded");
}
