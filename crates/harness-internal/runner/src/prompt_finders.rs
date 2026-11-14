//! Stock prompt finders: [`FindTextPrompt`] serves one prompt from text,
//! [`FindFilePrompt`] serves one from a file, and [`PromptSet`] serves the
//! prompts a Host inserts as text, as files, and as directories.
//!
//! A file is read with a blocking `std::fs` call at each find, and
//! nothing is cached, so an edited prompt shows in the next find. The
//! Harness calls a finder only from `Host::parse`, before the run
//! exists, so no read happens inside a run's future.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use runvil_engine::{Error, ErrorCode, ErrorFact, PromptFinder};

/// The extension of a prompt file, which a listing compares exactly.
const PROMPT_EXTENSION: &str = "md";

/// What the model reads when a find names no prompt. The default prose
/// of `NotFound` speaks of a path, and a prompt's name is not one.
const NO_SUCH_PROMPT: &str = "No prompt has that name.";

/// Finds one prompt, whose source is the text it was built with.
///
/// A find for any other name fails as [`NotFound`](ErrorCode::NotFound),
/// with the message `no prompt named {name}` and the name as the
/// [`Name`](ErrorFact::Name) fact.
#[derive(Debug)]
pub struct FindTextPrompt {
    name: String,
    text: String,
}

impl FindTextPrompt {
    /// A finder that serves `text` as the source of the prompt `name`.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> FindTextPrompt {
        FindTextPrompt {
            name: name.into(),
            text: text.into(),
        }
    }
}

impl PromptFinder for FindTextPrompt {
    fn find(&self, name: &str) -> Result<String, Error> {
        if name == self.name {
            Ok(self.text.clone())
        } else {
            Err(make_missing_error(name))
        }
    }
}

/// Finds one prompt, whose source is a file read again at each find, so
/// a change to the file shows in the next find.
///
/// The read is a blocking `std::fs::read_to_string`. When it fails, the
/// find fails with the message `prompt {name}: cannot read {path}` and
/// the system's reason, the name as the [`Name`](ErrorFact::Name) fact,
/// and the path as the [`Path`](ErrorFact::Path) fact. Its code is
/// [`NotFound`](ErrorCode::NotFound) when the file does not exist,
/// [`NotUtf8`](ErrorCode::NotUtf8) when its contents are not UTF-8, and
/// [`Io`](ErrorCode::Io) otherwise.
///
/// A find for any other name reads nothing and fails as
/// [`FindTextPrompt`]'s does.
#[derive(Debug)]
pub struct FindFilePrompt {
    name: String,
    path: PathBuf,
}

impl FindFilePrompt {
    /// A finder that serves the file at `path` as the source of the
    /// prompt `name`. Nothing is read or checked until a find. A relative
    /// `path` resolves against the working directory of each find.
    #[must_use]
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> FindFilePrompt {
        FindFilePrompt {
            name: name.into(),
            path: path.into(),
        }
    }
}

impl PromptFinder for FindFilePrompt {
    fn find(&self, name: &str) -> Result<String, Error> {
        if name == self.name {
            read_prompt(name, &self.path)
        } else {
            Err(make_missing_error(name))
        }
    }
}

/// Finds the prompts a Host inserted: as text, as a file, or as every
/// prompt file in a directory.
///
/// A find takes the newest insert that has the name, so a later insert
/// wins over an earlier one. A Host that inserts its shipped directory
/// and then its user's directory serves the user's file under a name
/// both hold. A find for a name no insert has fails as
/// [`FindTextPrompt`]'s does.
///
/// A file is read again at each find, so a change to it shows in the
/// next find, and a read that fails is the find's failure, as
/// [`FindFilePrompt`]'s is. A directory is listed when it is inserted and
/// again at each [`rescan`](PromptSet::rescan), and not in between: a
/// file added to it is found after the next rescan, and a find for a
/// file removed from it fails with the read's failure until then,
/// without trying an older insert.
///
/// A Host fills the set, then shares it behind an `Arc`. One handle goes
/// into its `Host` as the finder. The Host keeps the other, to call
/// `rescan` when it sees a directory change and
/// [`names`](PromptSet::names) to list what the set serves. The set
/// watches no directory itself. A rescan changes what later finds
/// return, and never a run that has started, which holds its parsed
/// prompt.
///
/// Reads and listings are blocking `std::fs` calls.
#[derive(Debug, Default)]
pub struct PromptSet {
    /// One entry per insert, oldest first.
    entries: Vec<Entry>,
}

impl PromptSet {
    /// An empty set, which finds no prompt.
    #[must_use]
    pub fn new() -> PromptSet {
        PromptSet::default()
    }

    /// Inserts `text` as the source of the prompt `name`.
    pub fn insert(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.entries.push(Entry::Text {
            name: name.into(),
            text: text.into(),
        });
    }

    /// Inserts the file at `path` as the source of the prompt `name`.
    /// Nothing is read or checked until a find. A relative `path`
    /// resolves against the working directory of each find.
    pub fn insert_file(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        self.entries.push(Entry::File {
            name: name.into(),
            path: path.into(),
        });
    }

    /// Inserts every prompt file in `directory`, each under its file
    /// name, such as `greet.md`, and keeps the directory for
    /// [`rescan`](PromptSet::rescan).
    ///
    /// A prompt file is a regular file directly in the directory whose
    /// extension is exactly `md` and whose name is UTF-8. A link to a
    /// regular file counts as that file. Subdirectories are not entered.
    /// A relative `directory` resolves against the working directory of
    /// each listing and find.
    ///
    /// # Errors
    /// Returns the failure when the directory cannot be listed, and then
    /// inserts nothing and does not keep the directory. Its message is
    /// `cannot list prompt directory {directory}` and the system's
    /// reason, with the directory as the [`Path`](ErrorFact::Path) fact.
    /// Its code is [`NotFound`](ErrorCode::NotFound) when the directory
    /// does not exist and [`Io`](ErrorCode::Io) otherwise.
    pub fn insert_dir(&mut self, directory: impl Into<PathBuf>) -> Result<(), Error> {
        let path = directory.into();
        let names = list_prompts(&path)?;
        self.entries.push(Entry::Dir {
            path,
            names: Mutex::new(names),
        });
        Ok(())
    }

    /// Lists again every directory [`insert_dir`](PromptSet::insert_dir)
    /// kept, so a file added to one is found and a file removed from one
    /// is not. The order between inserts stays as it was, and prompts
    /// inserted as text or as a file are untouched.
    ///
    /// Returns one failure for each directory that could not be listed,
    /// the failure `insert_dir` returns for it, and nothing when all
    /// were listed. A directory that could not be listed holds no prompt
    /// until a later rescan lists it, and the others are still listed.
    ///
    /// Two rescans never list one directory at the same time, and a find
    /// that reaches a directory while it is being listed waits.
    #[must_use = "each failure names a directory that now holds no prompt"]
    pub fn rescan(&self) -> Vec<Error> {
        let mut failures = Vec::new();
        for entry in &self.entries {
            let Entry::Dir { path, names } = entry else {
                continue;
            };
            // Held across the listing, so the rescan that lists later
            // also stores later.
            let mut listing = lock(names);
            match list_prompts(path) {
                Ok(listed) => *listing = listed,
                Err(failure) => {
                    listing.clear();
                    failures.push(failure);
                }
            }
        }
        failures
    }

    /// Every name the set has a prompt for, sorted, each name once. A
    /// file removed since its directory was last listed is still named,
    /// so a find for a name from this list can fail.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut all = BTreeSet::new();
        for entry in &self.entries {
            match entry {
                Entry::Text { name, .. } | Entry::File { name, .. } => {
                    all.insert(name.clone());
                }
                Entry::Dir { names, .. } => all.extend(lock(names).iter().cloned()),
            }
        }
        all.into_iter().collect()
    }
}

impl PromptFinder for PromptSet {
    fn find(&self, name: &str) -> Result<String, Error> {
        // Newest first, so a later insert wins over an earlier one.
        self.entries
            .iter()
            .rev()
            .find_map(|entry| entry.source_of(name))
            .unwrap_or_else(|| Err(make_missing_error(name)))
    }
}

/// One insert into a `PromptSet`.
#[derive(Debug)]
enum Entry {
    /// The prompt `name`, whose source is `text`.
    Text { name: String, text: String },
    /// The prompt `name`, whose source is the file at `path`.
    File { name: String, path: PathBuf },
    /// The prompt files of the directory `path`.
    Dir {
        path: PathBuf,
        /// The file name of each prompt file the last listing found,
        /// sorted. A rescan holds the lock while it lists the directory,
        /// so two rescans store their listings in the order they took
        /// them. A find holds it only to look a name up, and reads the
        /// file after releasing it.
        names: Mutex<Vec<String>>,
    },
}

impl Entry {
    /// The source of the prompt `name`, or the failure of reading it,
    /// when this insert has a prompt of that name, and `None` when it
    /// has none.
    fn source_of(&self, name: &str) -> Option<Result<String, Error>> {
        match self {
            Entry::Text { name: held, text } => (held == name).then(|| Ok(text.clone())),
            Entry::File { name: held, path } => (held == name).then(|| read_prompt(name, path)),
            Entry::Dir { path, names } => {
                // The lock ends with this statement, before the read.
                let listed = lock(names)
                    .binary_search_by(|held| held.as_str().cmp(name))
                    .is_ok();
                listed.then(|| read_prompt(name, &path.join(name)))
            }
        }
    }
}

/// Locks the listing of one directory.
fn lock(names: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
    // A listing is always replaced whole, so a poisoned lock still holds
    // a usable one.
    names.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the source of the prompt `name` from the file at `path`.
fn read_prompt(name: &str, path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|cause| {
        let message = format!("prompt {name}: cannot read {}", path.display());
        let mut error = make_io_error(message, path, &cause);
        error.facts.insert(ErrorFact::Name, name.into());
        error
    })
}

/// The file name of each prompt file in `directory`, sorted.
fn list_prompts(directory: &Path) -> Result<Vec<String>, Error> {
    let list = || -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_name = entry.file_name();
            // A find asks by `&str`, so it can never ask for a name that
            // is not UTF-8.
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if Path::new(name).extension() != Some(OsStr::new(PROMPT_EXTENSION)) {
                continue;
            }
            // `metadata` follows a link. `NotFound` is an entry removed
            // since it was listed, or a link that leads nowhere.
            match std::fs::metadata(entry.path()) {
                Ok(metadata) if metadata.is_file() => names.push(name.to_owned()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        names.sort();
        Ok(names)
    };
    list().map_err(|cause| {
        let message = format!("cannot list prompt directory {}", directory.display());
        make_io_error(message, directory, &cause)
    })
}

/// The failure of a find for a name the finder has no prompt for.
fn make_missing_error(name: &str) -> Error {
    let mut error = Error::new(
        ErrorCode::NotFound,
        format!("no prompt named {name}"),
        NO_SUCH_PROMPT,
    );
    error.facts.insert(ErrorFact::Name, name.into());
    error
}

/// The failure of a read or a listing that `cause` ended: `message`,
/// then the system's reason, with `path` as the `Path` fact and the code
/// that `cause`'s kind maps to.
fn make_io_error(message: String, path: &Path, cause: &io::Error) -> Error {
    let code = match cause.kind() {
        io::ErrorKind::NotFound => ErrorCode::NotFound,
        // What `read_to_string` reports for contents that are not UTF-8.
        io::ErrorKind::InvalidData => ErrorCode::NotUtf8,
        _ => ErrorCode::Io,
    };
    let mut error = Error::new(code, message, code.prose()).with_cause(cause);
    error
        .facts
        .insert(ErrorFact::Path, path.display().to_string().into());
    error
}

#[cfg(test)]
#[path = "prompt_finders-tests.rs"]
mod tests;
