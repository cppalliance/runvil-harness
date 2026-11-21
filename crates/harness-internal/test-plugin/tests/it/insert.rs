//! `insert_plugin_dir` against copies of the fixture library: what loads,
//! what fails and why, what is ignored, and that every library closes.
//! `insert_plugin_file` against one copy named by its file: it reports
//! the copy as the directory function reports each library it finds.
//! Each test drops its report, then removes its directory.

use std::fs;
use std::io;
use std::path::Path;

use harness_plugin_loader::{
    HostResources, PluginReport, PluginSource, insert_plugin_dir, insert_plugin_file,
};
use runvil_plugin::{Error, ErrorFact, PackRegistry};

use crate::support::{copy_fixture, remove, scratch};

/// Inserts every library in `dir` into a fresh Pack registry.
fn insert(dir: &Path) -> PluginReport {
    insert_plugin_dir(&mut PackRegistry::new(), dir, &HostResources::new())
        .expect("the directory reads")
}

/// The path `std::path::absolute` makes of `path`, as a library source.
fn library_source(path: &Path) -> PluginSource {
    PluginSource::Library(std::path::absolute(path).expect("the working directory is known"))
}

/// The fact `key` of a load failure, as text.
fn fact(failure: &Error, key: ErrorFact) -> Option<&str> {
    failure.facts.get(&key)?.as_str()
}

/// The `Path` fact a failure of the library at `path` holds.
fn library_path(path: &Path) -> String {
    std::path::absolute(path)
        .expect("the working directory is known")
        .display()
        .to_string()
}

#[test]
fn insert_plugin_dir_loads_the_fixture() {
    let dir = scratch("insert_plugin_dir_loads_the_fixture");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(
        format!("{} {}", plugin.name(), plugin.version()),
        "test-plugin 0.1.0"
    );
    assert_eq!(plugin.source(), &library_source(&file));
    drop(report);
    remove(&dir);
}

#[test]
fn the_same_fixture_in_two_directories_loads_in_both() {
    let first_dir = scratch("the_same_fixture_in_two_directories_first");
    let second_dir = scratch("the_same_fixture_in_two_directories_second");
    let first_file = copy_fixture(&first_dir, "plugin");
    let second_file = copy_fixture(&second_dir, "plugin");
    let first = insert(&first_dir);
    let second = insert(&second_dir);
    for (report, file) in [(&first, &first_file), (&second, &second_file)] {
        assert!(report.failures().is_empty(), "{:?}", report.failures());
        let [plugin] = report.plugins() else {
            panic!("one Plugin loads, got {:?}", report.plugins());
        };
        assert_eq!(plugin.name(), "test-plugin");
        assert_eq!(plugin.source(), &library_source(file));
    }
    drop((first, second));
    remove(&first_dir);
    remove(&second_dir);
}

#[test]
fn a_garbage_library_fails_at_insert_and_the_fixture_beside_it_still_loads() {
    let dir = scratch("a_garbage_library_fails_at_insert");
    // `garbage` sorts before `plugin`, so the failure comes first.
    let garbage = dir.join(format!("garbage.{}", std::env::consts::DLL_EXTENSION));
    fs::write(&garbage, b"this is not a library").expect("the garbage file is written");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    let [failure] = report.failures() else {
        panic!("one file fails, got {:?}", report.failures());
    };
    let reason = &failure.message;
    assert_eq!(fact(failure, ErrorFact::Reason), Some("open"));
    let cause = reason
        .strip_prefix("the library could not be opened: ")
        .unwrap_or_else(|| panic!("the reason starts with the open failure: {reason}"));
    // On Windows `libloading` reports only "LoadLibraryExW failed" and keeps
    // the system's reason as the error's source, so the text after the
    // prefix must carry that reason too.
    assert_ne!(cause, "LoadLibraryExW failed", "{reason}");
    #[cfg(windows)]
    assert!(
        cause.starts_with("LoadLibraryExW failed: ") && cause.ends_with("(os error 193)"),
        "a file that is not a Win32 module reports ERROR_BAD_EXE_FORMAT: {reason}"
    );
    #[cfg(not(windows))]
    assert!(!cause.is_empty(), "{reason}");
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&garbage).as_str())
    );
    let [plugin] = report.plugins() else {
        panic!("the fixture loads, got {:?}", report.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&file));
    drop(report);
    remove(&dir);
}

#[test]
fn a_file_without_the_library_extension_is_ignored() {
    let dir = scratch("a_file_without_the_library_extension_is_ignored");
    fs::write(dir.join("README.txt"), b"not a Plugin").expect("the README is written");
    copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    assert_eq!(report.plugins().len(), 1);
    drop(report);
    remove(&dir);
}

#[test]
fn a_second_copy_with_the_same_plugin_name_fails_as_a_duplicate() {
    let dir = scratch("a_second_copy_fails_as_a_duplicate");
    // The first by file name loads; the second repeats its Plugin name.
    let first = copy_fixture(&dir, "first");
    let second = copy_fixture(&dir, "second");
    let report = insert(&dir);
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&first));
    let [failure] = report.failures() else {
        panic!("the repeat fails, got {:?}", report.failures());
    };
    assert_eq!(
        failure.message,
        "a Plugin named test-plugin was already loaded from this directory"
    );
    assert_eq!(fact(failure, ErrorFact::Reason), Some("duplicate"));
    assert_eq!(fact(failure, ErrorFact::Name), Some("test-plugin"));
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&second).as_str())
    );
    drop(report);
    remove(&dir);
}

#[test]
fn a_pack_the_registry_already_holds_is_reported_and_its_plugin_still_loads() {
    let first_dir = scratch("a_pack_the_registry_already_holds_first");
    let second_dir = scratch("a_pack_the_registry_already_holds_second");
    copy_fixture(&first_dir, "plugin");
    let second_file = copy_fixture(&second_dir, "plugin");
    let mut packs = PackRegistry::new();
    let resources = HostResources::new();
    let first = insert_plugin_dir(&mut packs, &first_dir, &resources).expect("the directory reads");
    assert!(first.failures().is_empty(), "{:?}", first.failures());

    let second =
        insert_plugin_dir(&mut packs, &second_dir, &resources).expect("the directory reads");
    assert_eq!(second.plugins().len(), 1, "the second Plugin did load");
    let [failure] = second.failures() else {
        panic!("its Pack is refused, got {:?}", second.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("pack"));
    assert_eq!(fact(failure, ErrorFact::Name), Some("fixture/test"));
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&second_file).as_str())
    );
    assert_eq!(
        failure.message,
        "the Pack fixture/test of the Plugin test-plugin was not inserted: Pack `fixture/test` \
         is already in the registry"
    );
    drop((first, second, packs));
    remove(&first_dir);
    remove(&second_dir);
}

#[test]
fn insert_plugin_file_loads_the_fixture() {
    let dir = scratch("insert_plugin_file_loads_the_fixture");
    let file = copy_fixture(&dir, "plugin");
    let report = insert_plugin_file(&mut PackRegistry::new(), &file, &HostResources::new());
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(
        format!("{} {}", plugin.name(), plugin.version()),
        "test-plugin 0.1.0"
    );
    assert_eq!(plugin.source(), &library_source(&file));
    drop(report);
    remove(&dir);
}

#[test]
fn a_file_that_is_no_library_is_one_open_failure_with_its_path() {
    let dir = scratch("a_file_that_is_no_library_is_one_open_failure");
    let garbage = dir.join(format!("garbage.{}", std::env::consts::DLL_EXTENSION));
    fs::write(&garbage, b"this is not a library").expect("the garbage file is written");
    let report = insert_plugin_file(&mut PackRegistry::new(), &garbage, &HostResources::new());
    assert!(report.plugins().is_empty(), "{:?}", report.plugins());
    let [failure] = report.failures() else {
        panic!("the file fails, got {:?}", report.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("open"));
    assert!(
        failure
            .message
            .starts_with("the library could not be opened: "),
        "{}",
        failure.message
    );
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&garbage).as_str())
    );

    // A path that cannot be made absolute fails the same way, with the
    // path as it was given.
    let unnamed = insert_plugin_file(
        &mut PackRegistry::new(),
        Path::new(""),
        &HostResources::new(),
    );
    assert!(unnamed.plugins().is_empty(), "{:?}", unnamed.plugins());
    let [failure] = unnamed.failures() else {
        panic!("the empty path fails, got {:?}", unnamed.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("open"));
    assert_eq!(fact(failure, ErrorFact::Path), Some(""));
    drop((report, unnamed));
    remove(&dir);
}

#[test]
fn a_file_whose_pack_the_registry_already_holds_stays_in_the_report_with_the_refusal() {
    let dir = scratch("a_file_whose_pack_the_registry_already_holds");
    // Two copies of one library are two Plugins of one name, and each file
    // is a call of its own, so both load.
    let first_file = copy_fixture(&dir, "first");
    let second_file = copy_fixture(&dir, "second");
    let mut packs = PackRegistry::new();
    let resources = HostResources::new();
    let first = insert_plugin_file(&mut packs, &first_file, &resources);
    assert!(first.failures().is_empty(), "{:?}", first.failures());

    let second = insert_plugin_file(&mut packs, &second_file, &resources);
    let [plugin] = second.plugins() else {
        panic!("the second Plugin did load, got {:?}", second.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&second_file));
    let [failure] = second.failures() else {
        panic!("its Pack is refused, got {:?}", second.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("pack"));
    assert_eq!(fact(failure, ErrorFact::Name), Some("fixture/test"));
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&second_file).as_str())
    );
    assert_eq!(
        failure.message,
        "the Pack fixture/test of the Plugin test-plugin was not inserted: Pack `fixture/test` \
         is already in the registry"
    );
    drop((first, second, packs));
    remove(&dir);
}

#[test]
fn a_missing_directory_returns_an_io_error() {
    let dir = scratch("a_missing_directory_returns_an_io_error");
    let error = insert_plugin_dir(
        &mut PackRegistry::new(),
        &dir.join("missing"),
        &HostResources::new(),
    )
    .expect_err("a missing directory is an I/O error");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    remove(&dir);
}

/// The library is open as long as any `Arc` of its Plugin is, not as long
/// as the report is. Only Windows refuses to delete an open library, so
/// only there can a test see it.
#[cfg(windows)]
#[test]
fn a_library_stays_open_until_the_last_arc_of_its_plugin_drops() {
    use std::sync::Arc;

    let dir = scratch("a_library_stays_open_until_the_last_arc_drops");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    let plugin = Arc::clone(&report.plugins()[0]);
    drop(report);
    assert!(
        fs::remove_file(&file).is_err(),
        "the clone of the Plugin still holds the library open"
    );
    drop(plugin);
    remove(&dir);
}
