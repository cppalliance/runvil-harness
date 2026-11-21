//! A Host built over copies of the fixture library through
//! `Host::builder`: a Plugin directory, one library file, and an entry
//! point each load the fixture, and the Host has its Pack and keeps its
//! Plugin; a directory that cannot be read is one failure and the Host
//! is built; a Pack that the registry refuses is reported in one way for
//! a library and for an entry point, and its Plugin stays loaded with
//! the Host; and the Host keeps one report for each step that names a
//! Plugin, in the order written. Each test drops its Host, then removes
//! its directory.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use runvil::harness::{HostResources, PluginSource, PromptSet};
use runvil::packs::PackId;
use runvil::plugin::{Pack, PackInfo, PackRegistry, ToolContext};
use runvil::{
    BoxFuture, Error, ErrorCode, ErrorFact, Host, HostBuilder, ToolInfo, ToolOutput,
    make_tool_error,
};
use serde_json::Value;

use crate::support::{
    already_loaded, copy_fixture, counter, fact, library_path, library_source, remove, scratch,
};

/// The name and the version the fixture's Plugin gives itself.
const FIXTURE: &str = "test-plugin 0.1.0";

/// The builder of a Host whose finder serves no prompt.
fn builder() -> HostBuilder {
    Host::builder(Arc::new(PromptSet::new()))
}

/// The name of each of `host`'s Packs, in the order `Host::packs` lists
/// them.
fn fqns(host: &Host) -> Vec<&'static str> {
    host.packs().map(|info| info.fqn).collect()
}

/// The name with the version, and the source, of the one Plugin that the
/// one step of `host` loaded with no failure.
fn one_plugin(host: &Host) -> (String, PluginSource) {
    let [report] = host.plugin_reports() else {
        panic!("one report for the one step: {:?}", host.plugin_reports());
    };
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    (
        format!("{} {}", plugin.name(), plugin.version()),
        plugin.source().clone(),
    )
}

/// Opens the copy of the fixture library at `file`, as a Host that links
/// a Plugin holds that Plugin's code.
fn open(file: &Path) -> Library {
    // SAFETY: the file is a copy of the fixture library, whose
    // initialization code does nothing a test depends on.
    unsafe { Library::new(file) }.expect("the copy of the fixture opens")
}

/// `builder` with the entry point of `library` named as a Plugin that is
/// linked into the Host. The caller keeps `library` open until the Host
/// that the builder builds is dropped.
fn with_entry_of(builder: HostBuilder, library: &Library) -> HostBuilder {
    // SAFETY: the fixture exports `runvil_plugin` as an entry point of
    // the C contract, which is the type `with_plugin` takes.
    let entry =
        unsafe { library.get(b"runvil_plugin") }.expect("the fixture exports its entry point");
    builder.with_plugin(*entry)
}

/// A Pack with no tools that answers no call.
struct Idle;

impl Pack for Idle {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async {
            Err(make_tool_error(
                ErrorCode::ToolOther,
                "idle answers no call",
            ))
        })
    }
}

/// The factory of an [`Idle`] Pack.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn idle(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Idle))
}

/// A Rust Plugin that inserts `acme/test`, which takes the Pack id `test`
/// that the fixture's Pack `fixture/test` needs.
fn install_acme_test(packs: &mut PackRegistry) -> Result<(), Error> {
    packs.insert(PackInfo::new("acme/test"), idle)
}

/// What the registry says when it refuses the fixture's Pack after
/// [`install_acme_test`] ran.
const TEST_IS_TAKEN: &str = "the Pack fixture/test of the Plugin test-plugin was not inserted: \
    Pack `fixture/test` cannot take the Pack id test, which `acme/test` holds";

#[test]
fn a_host_over_a_plugin_directory_has_its_plugin_and_its_pack() {
    let dir = scratch("a_host_over_a_plugin_directory");
    let file = copy_fixture(&dir, "plugin");
    let host = builder().with_plugin_dir(&dir).build();
    assert_eq!(
        one_plugin(&host),
        (FIXTURE.to_owned(), library_source(&file))
    );
    assert_eq!(
        fqns(&host),
        ["fixture/test", "runvil/subagents", "runvil/user"],
        "the fixture's Pack stands beside the two built-ins"
    );
    drop(host);
    remove(&dir);
}

#[test]
fn a_plugin_directory_that_cannot_be_read_is_one_failure_and_the_host_is_built() {
    let dir = scratch("a_plugin_directory_that_cannot_be_read");
    let missing = dir.join("missing");
    let host = builder().with_plugin_dir(&missing).build();
    assert_eq!(
        fqns(&host),
        ["runvil/subagents", "runvil/user"],
        "the Host is built, with the built-ins alone"
    );
    let [report] = host.plugin_reports() else {
        panic!("one report for the one step: {:?}", host.plugin_reports());
    };
    assert!(report.plugins().is_empty(), "{:?}", report.plugins());
    let [failure] = report.failures() else {
        panic!("the directory is one failure, got {:?}", report.failures());
    };
    assert_eq!(failure.code, ErrorCode::PluginLoadFailed);
    assert_eq!(fact(failure, ErrorFact::Reason), Some("directory"));
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(missing.display().to_string().as_str()),
        "the directory as the builder was given it"
    );
    let reason = failure
        .message
        .strip_prefix("the Plugin directory could not be read: ")
        .unwrap_or_else(|| panic!("the message starts with what failed: {}", failure.message));
    assert!(!reason.is_empty(), "the system's reason follows");
    drop(host);
    remove(&dir);
}

#[test]
fn a_library_named_by_its_file_loads_as_one_found_in_a_directory_does() {
    let dir = scratch("a_library_named_by_its_file");
    let file = copy_fixture(&dir, "plugin");
    let from_dir = builder().with_plugin_dir(&dir).build();
    let (found, found_packs) = (one_plugin(&from_dir), fqns(&from_dir));
    drop(from_dir);

    let from_file = builder().with_plugin_file(&file).build();
    assert_eq!(found, (FIXTURE.to_owned(), library_source(&file)));
    assert_eq!(
        one_plugin(&from_file),
        found,
        "the same Plugin from the same library"
    );
    assert_eq!(fqns(&from_file), found_packs);
    drop(from_file);
    remove(&dir);
}

#[test]
fn an_entry_point_given_to_the_builder_loads_as_a_linked_plugin() {
    let dir = scratch("an_entry_point_given_to_the_builder");
    let library = open(&copy_fixture(&dir, "plugin"));
    let host = with_entry_of(builder(), &library).build();
    assert_eq!(
        one_plugin(&host),
        (FIXTURE.to_owned(), PluginSource::Linked)
    );
    assert_eq!(
        fqns(&host),
        ["fixture/test", "runvil/subagents", "runvil/user"]
    );
    assert_eq!(
        counter(&library, "live"),
        1,
        "the Host keeps the Plugin loaded"
    );
    drop(host);
    assert_eq!(
        (counter(&library, "unloads"), counter(&library, "live")),
        (1, 0),
        "the Plugin unloads with the Host"
    );
    drop(library);
    remove(&dir);
}

#[test]
fn a_library_whose_pack_id_an_earlier_rust_plugin_took_is_a_failure_and_stays_loaded_with_the_host()
{
    let dir = scratch("a_library_whose_pack_id_an_earlier_rust_plugin_took");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let host = builder()
        .with_rust_plugin(install_acme_test)
        .with_plugin_file(&file)
        .build();
    let library = already_loaded(&file).expect("the Host holds the library open");
    assert_eq!(
        fqns(&host),
        ["acme/test", "runvil/subagents", "runvil/user"],
        "the Rust Plugin stands first, so its Pack keeps the Pack id"
    );
    let [rust, fixture] = host.plugin_reports() else {
        panic!("one report for each step: {:?}", host.plugin_reports());
    };
    assert!(
        rust.plugins().is_empty() && rust.failures().is_empty(),
        "{rust:?}"
    );
    let [plugin] = fixture.plugins() else {
        panic!("the fixture did load, got {:?}", fixture.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&file));
    let [failure] = fixture.failures() else {
        panic!("its Pack is refused, got {:?}", fixture.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("pack"));
    assert_eq!(fact(failure, ErrorFact::Name), Some("fixture/test"));
    assert_eq!(
        fact(failure, ErrorFact::Path),
        Some(library_path(&file).as_str())
    );
    assert_eq!(failure.message, TEST_IS_TAKEN);

    // The Plugin inserted no Pack, so the report of its step is all that
    // holds it.
    assert_eq!(
        counter(&library, "live"),
        1,
        "the Host keeps a Plugin that inserted no Pack loaded"
    );
    drop(host);
    assert_eq!(
        (counter(&library, "unloads"), counter(&library, "live")),
        (1, 0),
        "the Plugin unloads with the Host"
    );
    drop(library);
    remove(&dir);
}

#[test]
fn an_entry_point_whose_pack_id_an_earlier_rust_plugin_took_is_reported_as_a_library_is() {
    /// The one Plugin and the one failure in the report of the second
    /// step of `host`, whose first step is [`install_acme_test`].
    fn refused(host: &Host) -> (String, PluginSource, Error) {
        let [_rust, fixture] = host.plugin_reports() else {
            panic!("one report for each step: {:?}", host.plugin_reports());
        };
        let ([plugin], [failure]) = (fixture.plugins(), fixture.failures()) else {
            panic!("the fixture loads and its Pack is refused: {fixture:?}");
        };
        (
            format!("{} {}", plugin.name(), plugin.version()),
            plugin.source().clone(),
            failure.clone(),
        )
    }

    let dir = scratch("an_entry_point_whose_pack_id_an_earlier_rust_plugin_took");
    let file = copy_fixture(&dir, "plugin");
    let by_file = builder()
        .with_rust_plugin(install_acme_test)
        .with_plugin_file(&file)
        .build();
    let (file_plugin, file_source, mut file_failure) = refused(&by_file);
    drop(by_file);

    let library = open(&file);
    let by_entry = with_entry_of(builder().with_rust_plugin(install_acme_test), &library).build();
    let (entry_plugin, entry_source, entry_failure) = refused(&by_entry);
    drop(by_entry);
    drop(library);

    assert_eq!(
        (file_plugin.as_str(), entry_plugin.as_str()),
        (FIXTURE, FIXTURE)
    );
    assert_eq!(file_source, library_source(&file));
    assert_eq!(entry_source, PluginSource::Linked);
    assert_eq!(fact(&entry_failure, ErrorFact::Reason), Some("pack"));
    assert_eq!(entry_failure.message, TEST_IS_TAKEN);
    assert_eq!(
        fact(&file_failure, ErrorFact::Path),
        Some(library_path(&file).as_str()),
        "the failure of a library names the library"
    );
    file_failure.facts.remove(&ErrorFact::Path);
    assert_eq!(
        file_failure, entry_failure,
        "the two failures differ in the `Path` fact alone"
    );
    remove(&dir);
}

#[test]
fn the_host_keeps_one_report_for_each_step_in_the_order_written() {
    let dir = scratch("the_host_keeps_one_report_for_each_step");
    // The directory holds one copy, and a second copy stands outside it,
    // so the directory's step loads one Plugin and the file's step loads
    // another of the same name.
    let plugins = dir.join("plugins");
    fs::create_dir(&plugins).expect("the Plugin directory is made");
    let in_dir = copy_fixture(&plugins, "plugin");
    let single = copy_fixture(&dir, "single");
    let host = builder()
        .with_rust_plugin(|packs| packs.insert(PackInfo::new("acme/kit"), idle))
        .with_resources(HostResources::new())
        .with_plugin_dir(&plugins)
        .with_plugin_file(&single)
        .build();
    let [rust, from_dir, from_file] = host.plugin_reports() else {
        panic!(
            "one report for each step that names a Plugin, and none for the resources: {:?}",
            host.plugin_reports()
        );
    };
    assert!(
        rust.plugins().is_empty() && rust.failures().is_empty(),
        "{rust:?}"
    );

    let [plugin] = from_dir.plugins() else {
        panic!(
            "the directory loads one Plugin, got {:?}",
            from_dir.plugins()
        );
    };
    assert_eq!(plugin.source(), &library_source(&in_dir));
    assert!(from_dir.failures().is_empty(), "{:?}", from_dir.failures());

    // Each step is one call of the loader, so the second Plugin of the
    // same name loads too, and the registry refuses its Pack.
    let [plugin] = from_file.plugins() else {
        panic!("the file loads one Plugin, got {:?}", from_file.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&single));
    let [failure] = from_file.failures() else {
        panic!("its Pack is refused, got {:?}", from_file.failures());
    };
    assert_eq!(fact(failure, ErrorFact::Reason), Some("pack"));
    assert_eq!(
        failure.message,
        "the Pack fixture/test of the Plugin test-plugin was not inserted: Pack `fixture/test` \
         is already in the registry"
    );
    assert_eq!(
        fqns(&host),
        [
            "acme/kit",
            "fixture/test",
            "runvil/subagents",
            "runvil/user"
        ]
    );
    drop(host);
    remove(&dir);
}
