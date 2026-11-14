//! Tests for the stock prompt finders: what each one finds and how its
//! finds fail, which files of a directory a `PromptSet` takes, which
//! insert wins a name, what a rescan changes and reports, what the set
//! names, and a `Host` that parses a prompt its application added to a
//! set the two share.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use runvil_engine::{BoxFuture, Error, ErrorCode, ErrorFact, PromptFinder};
use runvil_plugin::PackRegistry;
use serde_json::json;

use super::{FindFilePrompt, FindTextPrompt, PromptSet};
use crate::host::Host;
use crate::packs::Packs;
use crate::performers::Timer;

/// Tells apart the directories one process makes; nextest runs each test
/// in its own process, so the process id tells those apart.
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// A directory made for one test at
/// `<temporary directory>/runvil-tests/<label>-<pid>-<n>`, removed on
/// drop. Its name reads no clock and no random generator.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Makes the directory, first removing anything an earlier process
    /// with the same id left at that path.
    fn new(label: &str) -> Scratch {
        let n = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join("runvil-tests")
            .join(format!("{label}-{}-{n}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).expect("a stale scratch directory is removed");
        }
        fs::create_dir_all(&path).expect("the scratch directory is made");
        Scratch { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Makes the subdirectory `name` and returns its path.
    fn dir(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::create_dir(&path).expect("the subdirectory is made");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let removed = fs::remove_dir_all(&self.path);
        assert!(
            removed.is_ok() || std::thread::panicking(),
            "the scratch directory {} must be removed: {removed:?}",
            self.path.display()
        );
    }
}

/// Writes `text` as the file `name` in `dir`, replacing any file there,
/// and returns the file's path.
fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).expect("the file is written");
    path
}

/// The failure of a find for `name`, a name the finder has no prompt
/// for. It has no `Path` fact: nothing was read.
fn missing(name: &str) -> Error {
    let mut error = Error::new(
        ErrorCode::NotFound,
        format!("no prompt named {name}"),
        "No prompt has that name.",
    );
    error.facts.insert(ErrorFact::Name, json!(name));
    error
}

/// Asserts `message` is `start`, then `: `, then the system's reason,
/// which differs by platform.
#[track_caller]
fn assert_ends_with_a_reason(message: &str, start: &str) {
    let reason = message
        .strip_prefix(start)
        .and_then(|rest| rest.strip_prefix(": "));
    assert!(
        reason.is_some_and(|reason| !reason.is_empty()),
        "required `{start}: <the system's reason>`, actual `{message}`"
    );
}

/// Asserts `found` is the failure of reading the prompt `name` from the
/// file at `path`: `code` with its default prose, a message that names
/// the prompt and the path before the system's reason, and the name and
/// the path as its only facts.
#[track_caller]
fn assert_read_failure(found: Result<String, Error>, code: ErrorCode, name: &str, path: &Path) {
    let error = found.expect_err("the read fails");
    assert_eq!(error.code, code, "{error:?}");
    assert_ends_with_a_reason(
        &error.message,
        &format!("prompt {name}: cannot read {}", path.display()),
    );
    assert_eq!(error.prose, code.prose());
    assert_eq!(error.at, None);
    assert_eq!(
        error.facts,
        BTreeMap::from([
            (ErrorFact::Name, json!(name)),
            (ErrorFact::Path, json!(path.display().to_string())),
        ])
    );
}

/// Asserts `error` is the failure of listing `directory`, which does not
/// exist: `NotFound` with its default prose, a message that names the
/// directory before the system's reason, and the directory as its only
/// fact.
#[track_caller]
fn assert_list_failure(error: &Error, directory: &Path) {
    assert_eq!(error.code, ErrorCode::NotFound, "{error:?}");
    assert_ends_with_a_reason(
        &error.message,
        &format!("cannot list prompt directory {}", directory.display()),
    );
    assert_eq!(error.prose, ErrorCode::NotFound.prose());
    assert_eq!(error.at, None);
    assert_eq!(
        error.facts,
        BTreeMap::from([(ErrorFact::Path, json!(directory.display().to_string()))])
    );
}

/// Rescans `set` and asserts that every directory it keeps was listed.
#[track_caller]
fn rescan_all(set: &PromptSet) {
    let failures = set.rescan();
    assert!(failures.is_empty(), "every directory lists: {failures:?}");
}

/// A set holding the prompt files of `directory`.
fn set_over(directory: &Path) -> PromptSet {
    let mut set = PromptSet::new();
    set.insert_dir(directory).expect("the directory lists");
    set
}

#[test]
fn a_text_prompt_serves_its_text_under_its_name_and_fails_any_other_name_as_not_found() {
    let finder = FindTextPrompt::new("greet.md", "Say hi.");
    assert_eq!(finder.find("greet.md").as_deref(), Ok("Say hi."));
    assert_eq!(finder.find("other.md"), Err(missing("other.md")));
}

#[test]
fn a_file_prompt_reads_its_file_again_at_each_find() {
    let dir = Scratch::new("a_file_prompt_reads_its_file_again");
    // The prompt's name is the one given, whatever the file is called.
    let path = write(dir.path(), "draft.txt", "first");
    let finder = FindFilePrompt::new("greet.md", path);
    assert_eq!(finder.find("greet.md").as_deref(), Ok("first"));
    write(dir.path(), "draft.txt", "second");
    assert_eq!(finder.find("greet.md").as_deref(), Ok("second"));
}

#[test]
fn a_file_prompt_whose_file_is_missing_fails_as_not_found_naming_the_prompt_and_the_path() {
    let dir = Scratch::new("a_file_prompt_whose_file_is_missing");
    let path = dir.path().join("missing.md");
    let finder = FindFilePrompt::new("greet.md", &path);
    assert_read_failure(
        finder.find("greet.md"),
        ErrorCode::NotFound,
        "greet.md",
        &path,
    );
}

#[test]
fn a_file_prompt_whose_file_is_not_utf8_fails_as_not_utf8() {
    let dir = Scratch::new("a_file_prompt_whose_file_is_not_utf8");
    let path = dir.path().join("greet.md");
    fs::write(&path, [0xff, 0xfe, 0xfd]).expect("the file is written");
    let finder = FindFilePrompt::new("greet.md", &path);
    assert_read_failure(
        finder.find("greet.md"),
        ErrorCode::NotUtf8,
        "greet.md",
        &path,
    );
}

#[test]
fn a_file_prompt_whose_path_is_a_directory_fails_as_io() {
    // Reading a directory as a file fails on every platform, and nowhere
    // as a missing file or as bytes that are not UTF-8.
    let dir = Scratch::new("a_file_prompt_whose_path_is_a_directory");
    let finder = FindFilePrompt::new("greet.md", dir.path());
    assert_read_failure(
        finder.find("greet.md"),
        ErrorCode::Io,
        "greet.md",
        dir.path(),
    );
}

#[test]
fn a_file_prompt_reads_nothing_for_another_name() {
    let dir = Scratch::new("a_file_prompt_reads_nothing_for_another_name");
    // No file is at the path, so a read would fail with the path as a
    // fact, which the failure of an unknown name does not have.
    let finder = FindFilePrompt::new("greet.md", dir.path().join("missing.md"));
    assert_eq!(finder.find("other.md"), Err(missing("other.md")));
}

#[test]
fn an_empty_set_finds_no_prompt_and_names_none() {
    let set = PromptSet::new();
    assert_eq!(set.find("greet.md"), Err(missing("greet.md")));
    assert_eq!(set.names(), Vec::<String>::new());
}

#[test]
fn a_set_serves_a_prompt_inserted_as_text_and_one_inserted_as_a_file() {
    let dir = Scratch::new("a_set_serves_text_and_a_file");
    let path = write(dir.path(), "draft.txt", "first");
    let mut set = PromptSet::new();
    set.insert("greet.md", "Say hi.");
    set.insert_file("research.md", path);
    assert_eq!(set.find("greet.md").as_deref(), Ok("Say hi."));
    assert_eq!(set.find("research.md").as_deref(), Ok("first"));
    assert_eq!(set.find("other.md"), Err(missing("other.md")));
    // The file is read again at each find.
    write(dir.path(), "draft.txt", "second");
    assert_eq!(set.find("research.md").as_deref(), Ok("second"));
}

#[test]
fn a_later_insert_wins_a_name_over_an_earlier_one() {
    let dir = Scratch::new("a_later_insert_wins_a_name");
    let file = write(dir.path(), "draft.txt", "the file");
    let prompts = dir.dir("prompts");
    write(&prompts, "greet.md", "the directory");
    let mut set = PromptSet::new();
    set.insert("greet.md", "the first text");
    set.insert("greet.md", "the second text");
    assert_eq!(set.find("greet.md").as_deref(), Ok("the second text"));
    set.insert_file("greet.md", file);
    assert_eq!(set.find("greet.md").as_deref(), Ok("the file"));
    set.insert_dir(&prompts).expect("the directory lists");
    assert_eq!(set.find("greet.md").as_deref(), Ok("the directory"));
    set.insert("greet.md", "the third text");
    assert_eq!(set.find("greet.md").as_deref(), Ok("the third text"));
}

#[test]
fn a_set_does_not_try_an_older_insert_when_the_newest_ones_read_fails() {
    let dir = Scratch::new("a_set_does_not_try_an_older_insert");
    let path = dir.path().join("missing.md");
    let mut set = PromptSet::new();
    set.insert("greet.md", "the older text");
    set.insert_file("greet.md", &path);
    assert_read_failure(set.find("greet.md"), ErrorCode::NotFound, "greet.md", &path);
}

#[test]
fn insert_dir_takes_the_md_files_directly_in_the_directory_under_their_file_names() {
    let dir = Scratch::new("insert_dir_takes_the_md_files");
    write(dir.path(), "research.md", "Look it up.");
    write(dir.path(), "greet.md", "Say hi.");
    // None of these is a prompt file: another extension, `md` in another
    // case, a longer extension, `md` as the whole name, and a name that
    // is all extension.
    let others = ["notes.txt", "shout.MD", "greet.md.bak", "md", ".md"];
    for other in others {
        write(dir.path(), other, "not a prompt");
    }
    // A directory named like a prompt file is not one, and nothing inside
    // a subdirectory is taken.
    for sub in ["sub", "x.md"] {
        write(&dir.dir(sub), "inner.md", "too deep");
    }

    let set = set_over(dir.path());
    assert_eq!(set.names(), ["greet.md", "research.md"]);
    assert_eq!(set.find("greet.md").as_deref(), Ok("Say hi."));
    assert_eq!(set.find("research.md").as_deref(), Ok("Look it up."));
    for name in others.into_iter().chain(["sub", "x.md", "inner.md"]) {
        assert_eq!(set.find(name), Err(missing(name)));
    }
}

#[test]
fn a_file_from_a_directory_is_read_again_at_each_find_with_no_rescan() {
    let dir = Scratch::new("a_file_from_a_directory_is_read_again");
    write(dir.path(), "greet.md", "first");
    let set = set_over(dir.path());
    assert_eq!(set.find("greet.md").as_deref(), Ok("first"));
    write(dir.path(), "greet.md", "second");
    assert_eq!(set.find("greet.md").as_deref(), Ok("second"));
}

#[test]
fn insert_dir_fails_for_a_missing_directory_and_does_not_keep_it() {
    let dir = Scratch::new("insert_dir_fails_for_a_missing_directory");
    let absent = dir.path().join("absent");
    let mut set = PromptSet::new();
    let error = set
        .insert_dir(&absent)
        .expect_err("the directory does not exist");
    assert_list_failure(&error, &absent);
    assert_eq!(set.names(), Vec::<String>::new());
    // A rescan would fail to list the directory had the set kept it.
    rescan_all(&set);
}

#[test]
fn a_file_added_to_a_directory_is_found_after_the_next_rescan() {
    let dir = Scratch::new("a_file_added_is_found_after_a_rescan");
    write(dir.path(), "greet.md", "Say hi.");
    let set = set_over(dir.path());
    write(dir.path(), "research.md", "Look it up.");
    assert_eq!(set.find("research.md"), Err(missing("research.md")));
    rescan_all(&set);
    assert_eq!(set.find("research.md").as_deref(), Ok("Look it up."));
    assert_eq!(set.find("greet.md").as_deref(), Ok("Say hi."));
}

#[test]
fn a_file_removed_from_a_directory_fails_its_find_until_a_rescan_forgets_it() {
    let dir = Scratch::new("a_file_removed_fails_its_find");
    let path = write(dir.path(), "greet.md", "Say hi.");
    let set = set_over(dir.path());
    fs::remove_file(&path).expect("the file is removed");
    // The last listing still has the name, so the find reads the file.
    assert_read_failure(set.find("greet.md"), ErrorCode::NotFound, "greet.md", &path);
    rescan_all(&set);
    assert_eq!(set.find("greet.md"), Err(missing("greet.md")));
}

#[test]
fn the_later_of_two_directories_wins_a_name_until_a_rescan_finds_its_file_gone() {
    let dir = Scratch::new("the_later_of_two_directories_wins");
    let shipped = dir.dir("shipped");
    let user = dir.dir("user");
    write(&shipped, "greet.md", "the shipped one");
    let path = write(&user, "greet.md", "the user's one");
    let mut set = set_over(&shipped);
    set.insert_dir(&user).expect("the directory lists");
    assert_eq!(set.find("greet.md").as_deref(), Ok("the user's one"));

    fs::remove_file(&path).expect("the file is removed");
    // Until a rescan the later directory still has the name, and the
    // earlier one is not tried.
    assert_read_failure(set.find("greet.md"), ErrorCode::NotFound, "greet.md", &path);
    rescan_all(&set);
    assert_eq!(set.find("greet.md").as_deref(), Ok("the shipped one"));
}

#[test]
fn a_prompt_inserted_as_text_after_a_directory_still_wins_after_a_rescan() {
    let dir = Scratch::new("a_text_after_a_directory_still_wins");
    write(dir.path(), "greet.md", "the directory");
    let mut set = set_over(dir.path());
    set.insert("greet.md", "the text");
    assert_eq!(set.find("greet.md").as_deref(), Ok("the text"));
    rescan_all(&set);
    assert_eq!(set.find("greet.md").as_deref(), Ok("the text"));
}

/// A set over two directories of `dir`, `lost` inserted before `kept`,
/// after `lost` was removed: returns the set and both paths. `lost` held
/// `research.md` and `kept` holds `greet.md`.
fn set_with_a_lost_directory(dir: &Scratch) -> (PromptSet, PathBuf, PathBuf) {
    let lost = dir.dir("lost");
    let kept = dir.dir("kept");
    write(&lost, "research.md", "Look it up.");
    write(&kept, "greet.md", "Say hi.");
    let mut set = set_over(&lost);
    set.insert_dir(&kept).expect("the directory lists");
    fs::remove_dir_all(&lost).expect("the directory is removed");
    (set, lost, kept)
}

#[test]
fn a_rescan_reports_a_directory_it_cannot_list_and_still_lists_the_others() {
    let dir = Scratch::new("a_rescan_reports_a_directory_it_cannot_list");
    let (set, lost, kept) = set_with_a_lost_directory(&dir);
    write(&kept, "plan.md", "Make a plan.");

    // The rescan reaches `lost` first, and goes on to `kept`.
    let failures = set.rescan();
    let [failure] = failures.as_slice() else {
        panic!("required one failure, for the lost directory, actual {failures:?}");
    };
    assert_list_failure(failure, &lost);
    assert_eq!(set.find("research.md"), Err(missing("research.md")));
    assert_eq!(set.find("plan.md").as_deref(), Ok("Make a plan."));
    assert_eq!(set.names(), ["greet.md", "plan.md"]);
}

#[test]
fn a_directory_made_again_serves_its_files_after_the_next_rescan() {
    let dir = Scratch::new("a_directory_made_again_serves_its_files");
    let (set, lost, _kept) = set_with_a_lost_directory(&dir);
    assert_eq!(set.rescan().len(), 1, "the lost directory fails to list");

    fs::create_dir(&lost).expect("the directory is made again");
    write(&lost, "plan.md", "Make a plan.");
    // The set kept the directory through the rescan that could not list
    // it, so nothing is found before the next one.
    assert_eq!(set.find("plan.md"), Err(missing("plan.md")));
    rescan_all(&set);
    assert_eq!(set.find("plan.md").as_deref(), Ok("Make a plan."));
    assert_eq!(set.find("research.md"), Err(missing("research.md")));
    assert_eq!(set.names(), ["greet.md", "plan.md"]);
}

#[test]
fn a_rescan_of_a_set_with_no_directory_reports_nothing_and_changes_nothing() {
    let dir = Scratch::new("a_rescan_of_a_set_with_no_directory");
    let path = write(dir.path(), "draft.txt", "Look it up.");
    let mut set = PromptSet::new();
    rescan_all(&set);
    set.insert("greet.md", "Say hi.");
    set.insert_file("research.md", path);
    rescan_all(&set);
    assert_eq!(set.names(), ["greet.md", "research.md"]);
    assert_eq!(set.find("greet.md").as_deref(), Ok("Say hi."));
    assert_eq!(set.find("research.md").as_deref(), Ok("Look it up."));
}

#[test]
fn names_lists_every_insert_sorted_with_each_name_once() {
    let dir = Scratch::new("names_lists_every_insert");
    let file = write(dir.path(), "draft.txt", "the file");
    let prompts = dir.dir("prompts");
    write(&prompts, "plan.md", "the directory's plan");
    write(&prompts, "greet.md", "the directory's greeting");
    let mut set = PromptSet::new();
    // Inserted out of order, and `greet.md` both as a file and by the
    // directory.
    set.insert("zoom.md", "the text");
    set.insert_file("greet.md", file);
    set.insert_dir(&prompts).expect("the directory lists");
    assert_eq!(set.names(), ["greet.md", "plan.md", "zoom.md"]);
}

#[test]
fn names_lists_what_the_last_rescan_found() {
    let dir = Scratch::new("names_lists_what_the_last_rescan_found");
    let path = write(dir.path(), "greet.md", "Say hi.");
    let set = set_over(dir.path());
    write(dir.path(), "research.md", "Look it up.");
    fs::remove_file(&path).expect("the file is removed");
    // Until a rescan the removed file is still named and the added one
    // is not.
    assert_eq!(set.names(), ["greet.md"]);
    rescan_all(&set);
    assert_eq!(set.names(), ["research.md"]);
}

/// A prompt that parses, and names itself `greet`.
const GREET: &str = "---\nname: greet\ndescription: d\nrunvil: 0\n---\n\n## Only\n\nDone.\n";

/// A timer whose every sleep ends at once.
struct NoWait;

impl Timer for NoWait {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
}

#[test]
fn a_host_parses_a_prompt_added_to_the_set_it_shares_once_its_application_rescans() {
    let dir = Scratch::new("a_host_parses_a_prompt_added_to_the_set");
    // One set behind an `Arc`: the Host holds one handle as its finder,
    // and the application keeps the other to rescan through.
    let prompts = Arc::new(set_over(dir.path()));
    let host = Host {
        packs: Packs::new(PackRegistry::new()),
        prompts: prompts.clone(),
        timer: Arc::new(NoWait),
    };
    assert_eq!(host.parse("greet.md"), Err(missing("greet.md")));

    write(dir.path(), "greet.md", GREET);
    rescan_all(&prompts);
    let prompt = host
        .parse("greet.md")
        .expect("the rescanned set finds the added prompt");
    assert_eq!(prompt.file(), "greet.md");
    assert_eq!(prompt.frontmatter().name(), "greet");
}
