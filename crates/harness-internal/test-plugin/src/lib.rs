//! `harness-test-plugin` - a library Plugin for the loader's end-to-end
//! tests: a dynamic library that exports `runvil_plugin` and follows the C
//! contract. It inserts one Pack, `fixture/test`. Three of its tools show
//! a call that ends in one step, one that takes three, and one that never
//! ends. One answers with the Host's resources that its call is lent,
//! and one is woken by a thread that it starts. Nine more make one file
//! request each, one for each file function of the contract: `stat`,
//! `list`, and `read` read, and `write`, `append`, `mkdir`, `remove`,
//! `rename`, and `copy` change files. The last, `read_many`, reads many
//! files in one call.
//!
//! ## Invariants
//!
//! - No `[dependencies]` at all, so a library Plugin needs nothing from
//!   Runvil. Only the end-to-end tests in `tests/it` name other crates, as
//!   `[dev-dependencies]`. The C types below are a private copy of the
//!   contract's, not an import. Each lays its fields out as the
//!   contract's type does.
//! - Only `runvil_plugin` and the six counters `harness_test_plugin_loads`,
//!   `harness_test_plugin_unloads`, `harness_test_plugin_live`,
//!   `harness_test_plugin_packs`, `harness_test_plugin_calls`, and
//!   `harness_test_plugin_steps` are public. The mirrored C types stay
//!   private, and unit tests pin them to the same layout numbers as
//!   `runvil-plugin-sys`.
//! - The counters are process-global, so each copy of the library that is
//!   opened counts on its own, from zero.
//! - The entry point never unwinds: it has no `unwrap`, `expect`,
//!   indexing, or arithmetic that can panic.
//! - `RV_PLUGIN_LOAD` loads only when the `RvHost` table reports exactly
//!   version 0.7, and checks that before it reads `args`.
//! - `RV_PLUGIN_LOAD` describes the Plugin exactly once, as `test-plugin`
//!   at `0.1.0`, inserts its one Pack, and sets the instance pointer only
//!   when both succeed. A failing load frees what it made, and so does a
//!   failing `RV_PACK_NEW`.
//! - A tool call's state is freed by the step that ends the call or by
//!   `RV_TOOL_CANCEL`, never by both.
//! - A step that returns `RV_PENDING` with no file request parks its
//!   call, and only a wake through the waker the step is lent brings the
//!   call's next step. A step of `steps` or `resources` wakes its call
//!   before it returns, and the first step of `later` starts a thread
//!   that wakes the call a little later. The `never` tool wakes nothing,
//!   so its call stays parked until the Host abandons it.
//! - The thread that a `later` call starts is joined when the call's
//!   state is freed, so no thread outlives its call, and none is left
//!   when `RV_PLUGIN_UNLOAD` comes.
//! - A step makes one file request at most and then returns
//!   `RV_PENDING`. The step after it reads the answer it is lent before
//!   it returns, and keeps no pointer into it.
//! - A tool whose calls change files sets `RV_TOOL_CHANGES_FILES`, and no
//!   other tool does.
//! - A file operation that fails never fails its call: a tool that makes
//!   one request ends its call with a text that says so, and `read_many`
//!   counts the failure and goes on.
#![expect(unsafe_code, reason = "exports a C entry point")]

use std::ffi::{c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(test)]
#[path = "lib-tests.rs"]
mod tests;

/// What every function in the contract returns, except `wake`, which
/// returns nothing: zero for success.
type RvStatus = u32;
/// Success.
const RV_OK: RvStatus = 0;
/// The caller broke the contract.
const RV_ERR_MISUSE: RvStatus = 4;
/// The Plugin declined to load. This Plugin never refuses.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "pinned by the unit tests; never returned")
)]
const RV_ERR_REFUSED: RvStatus = 5;
/// The Plugin does not know the verb it was sent.
const RV_ERR_UNSUPPORTED: RvStatus = 6;
/// The tool call is parked. The Host sends its next step after the Plugin
/// wakes the call, or, when the step made a file request, when the answer
/// is ready.
const RV_PENDING: RvStatus = 7;
/// A file operation failed: the `status` of an `RvFileAnswer` whose
/// `error` says how.
const RV_ERR_FILE: RvStatus = 8;

/// The operation an `RvCall` asks for.
type RvVerb = u32;
/// Make the Plugin instance, name the Plugin, and insert its Packs.
const RV_PLUGIN_LOAD: RvVerb = 1;
/// Free the Plugin instance.
const RV_PLUGIN_UNLOAD: RvVerb = 2;
/// Make one run's instance of a Pack and list its tools.
const RV_PACK_NEW: RvVerb = 3;
/// Free one run's instance of a Pack.
const RV_PACK_DELETE: RvVerb = 4;
/// Run one step of a tool call.
const RV_TOOL_CALL: RvVerb = 5;
/// Free the state of a tool call the Host has abandoned.
const RV_TOOL_CANCEL: RvVerb = 6;

/// How a tool call ended.
type RvResultKind = u32;
/// The call succeeded, and the Plugin vouches for its text.
const RV_RESULT_TRUSTED: RvResultKind = 1;
/// The call failed for a reason the other kinds do not name.
const RV_RESULT_OTHER: RvResultKind = 5;

/// What a path names, in an `RvStat`.
type RvFileType = u32;
/// A regular file.
const RV_FILE_TYPE_FILE: RvFileType = 1;
/// A directory.
const RV_FILE_TYPE_DIRECTORY: RvFileType = 2;
/// A symbolic link.
const RV_FILE_TYPE_SYMLINK: RvFileType = 3;

/// Why a file operation failed, in an `RvFileError`.
type RvFileErrorKind = u32;

/// What a tool does beyond answering its calls, in an `RvToolInfo`.
type RvToolFlags = u32;
/// A call of the tool changes files of its run.
const RV_TOOL_CHANGES_FILES: RvToolFlags = 1;

/// The contract's major version, which the `RvHost` table must report.
const RV_API_MAJOR: u32 = 0;
/// The contract's minor version, which the `RvHost` table must report.
const RV_API_MINOR: u32 = 7;

/// The name this Plugin gives `describe_plugin`.
const NAME: &str = "test-plugin";
/// The version this Plugin gives `describe_plugin`.
const VERSION: &str = "0.1.0";
/// The name of the one Pack this Plugin inserts.
const PACK: &str = "fixture/test";
/// What the Pack says it offers.
const PACK_DESCRIPTION: &str = "The loader's test fixture";
/// The parameters of a tool that takes none: an object with no properties.
const NO_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;
/// The parameters of a tool that works on one path: an object with the
/// one string `path`.
const PATH_PARAMETERS: &str =
    r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#;
/// The parameters of a tool that writes: the strings `path` and `text`.
const TEXT_PARAMETERS: &str = r#"{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"]}"#;
/// The parameters of a tool that makes or removes: the string `path` and
/// the boolean `recursive`.
const RECURSIVE_PARAMETERS: &str = r#"{"type":"object","properties":{"path":{"type":"string"},"recursive":{"type":"boolean"}},"required":["path"]}"#;
/// The parameters of a tool that works on two paths: the strings `from`
/// and `to`.
const PAIR_PARAMETERS: &str = r#"{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"]}"#;
/// The parameters of the tool that reads many files: the string `path`
/// and the integer `count`.
const MANY_PARAMETERS: &str = r#"{"type":"object","properties":{"path":{"type":"string"},"count":{"type":"integer"}},"required":["path","count"]}"#;
/// The tool that ends in one step, with `hello ` and its arguments.
const HELLO: &str = "hello";
/// The tool that ends in its third step, with `3 steps`. It wakes its
/// call before each step that returns `RV_PENDING`.
const STEPS: &str = "steps";
/// The tool that never ends and wakes nothing, so its call stays parked.
const NEVER: &str = "never";
/// The tool that ends in its second step, with the Host's resources as
/// that step is lent them.
const RESOURCES: &str = "resources";
/// The tool whose first step starts a thread that wakes the call after
/// [`LATER_DELAY`]. It ends in its second step: with `woken` when the
/// thread's wake had come by then, and with `stepped before the wake`
/// when the Host sent that step first.
const LATER: &str = "later";
/// How long the thread of a `later` call waits before it wakes the call.
const LATER_DELAY: Duration = Duration::from_millis(50);
/// The tool that says what its `path` names.
const STAT: &str = "stat";
/// The tool that lists the directory at its `path`.
const LIST: &str = "list";
/// The tool that answers the text of the file at its `path`.
const READ: &str = "read";
/// The tool that writes its `text` as the whole file at its `path`.
const WRITE: &str = "write";
/// The tool that adds its `text` to the end of the file at its `path`.
const APPEND: &str = "append";
/// The tool that makes a directory at its `path`.
const MKDIR: &str = "mkdir";
/// The tool that removes the file or the directory at its `path`.
const REMOVE: &str = "remove";
/// The tool that moves what its `from` names to its `to`.
const RENAME: &str = "rename";
/// The tool that copies the file its `from` names to its `to`.
const COPY: &str = "copy";
/// The tool that reads `count` files of the directory at its `path`, the
/// ones named `0`, `1`, and so on, one in each step.
const READ_MANY: &str = "read_many";

/// A UTF-8 string that is not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvStr {
    /// The first byte.
    ptr: *const c_char,
    /// The length in bytes.
    len: usize,
}

/// A run of bytes: what `file_write` and `file_append` take, and what the
/// answer to `file_read` holds.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvBytes {
    /// The first byte.
    ptr: *const u8,
    /// The length in bytes.
    len: usize,
}

/// What a path names, which the answer to `file_stat` and each
/// `RvDirEntry` hold.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvStat {
    /// One of the `RV_FILE_TYPE_` constants.
    file_type: RvFileType,
    /// Zero.
    reserved: u32,
    /// The length of a file in bytes.
    len: u64,
}

/// One entry of a directory.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvDirEntry {
    /// The entry's name within its directory.
    name: RvStr,
    /// What the entry is.
    stat: RvStat,
}

/// The entries of a directory, which the answer to `file_list` holds.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvDirEntries {
    /// The first entry.
    ptr: *const RvDirEntry,
    /// How many entries there are.
    len: usize,
}

/// Why a file operation failed, in an `RvFileAnswer` whose `status` is
/// `RV_ERR_FILE`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvFileError {
    /// One of the contract's `RV_FILE_ERROR_` constants.
    kind: RvFileErrorKind,
    /// Zero.
    reserved: u32,
    /// What went wrong.
    text: RvStr,
}

/// The answer to a file request, which the step after the request is
/// lent through `answer` in its `RvToolCallArgs`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvFileAnswer {
    /// `size_of::<RvFileAnswer>()`.
    size: u32,
    /// `RV_OK`, or `RV_ERR_FILE` with `error` filled.
    status: RvStatus,
    /// What the path names, in the answer to `file_stat`.
    stat: RvStat,
    /// The entries of the directory, in the answer to `file_list`.
    entries: RvDirEntries,
    /// The bytes read, in the answer to `file_read`.
    bytes: RvBytes,
    /// Why the operation failed, when `status` is `RV_ERR_FILE`.
    error: RvFileError,
}

/// What a Plugin tells `describe_plugin` about itself.
#[repr(C)]
#[derive(Debug)]
struct RvPluginInfo {
    /// `size_of::<RvPluginInfo>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The Plugin's name.
    name: RvStr,
    /// The Plugin's version.
    version: RvStr,
}

/// What a Plugin tells `insert_pack` about one of its Packs.
#[repr(C)]
#[derive(Debug)]
struct RvPackInfo {
    /// `size_of::<RvPackInfo>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The Pack's name, a vendor and a Pack id joined by `/`.
    name: RvStr,
    /// One line saying what the Pack offers.
    description: RvStr,
}

/// What a Pack instance tells `add_tool` about one of its tools.
#[repr(C)]
#[derive(Debug)]
struct RvToolInfo {
    /// `size_of::<RvToolInfo>()`.
    size: u32,
    /// Zero, or `RV_TOOL_CHANGES_FILES`.
    flags: RvToolFlags,
    /// The tool's name within its Pack.
    name: RvStr,
    /// The name a model calls the tool by, or empty.
    wire_name: RvStr,
    /// What the tool does.
    description: RvStr,
    /// The JSON text of the tool's parameter schema.
    parameters: RvStr,
}

/// How a tool call ended, which a step tells `finish`.
#[repr(C)]
#[derive(Debug)]
struct RvToolResult {
    /// `size_of::<RvToolResult>()`.
    size: u32,
    /// One of the `RV_RESULT_` constants.
    kind: RvResultKind,
    /// The call's output, or a message saying what went wrong.
    text: RvStr,
}

/// What the `RvHost` table reports on every call: the contract version.
#[repr(C)]
#[derive(Debug)]
struct RvHost {
    /// `size_of::<RvHost>()`.
    size: u32,
    /// The contract's major version.
    api_major: u32,
    /// The contract's minor version.
    api_minor: u32,
    /// Zero.
    reserved: u32,
}

/// The arguments of `RV_PLUGIN_LOAD`: the functions the Plugin may call
/// while it loads, each taking `ctx` first.
#[repr(C)]
#[derive(Debug)]
struct RvLoadArgs {
    /// `size_of::<RvLoadArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this load, which the Plugin passes back and
    /// never reads through.
    ctx: *mut c_void,
    /// Declines the load. This Plugin never calls it.
    refuse: unsafe extern "C" fn(ctx: *mut c_void, reason: RvStr) -> RvStatus,
    /// Names the Plugin, exactly once.
    describe_plugin: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvPluginInfo) -> RvStatus,
    /// Inserts one Pack.
    insert_pack: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvPackInfo) -> RvStatus,
}

/// The arguments of `RV_PACK_NEW`: the Pack to make an instance of, and
/// the functions the Plugin may call while it makes one.
#[repr(C)]
#[derive(Debug)]
struct RvPackNewArgs {
    /// `size_of::<RvPackNewArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this call.
    ctx: *mut c_void,
    /// The name of the Pack, as the Plugin gave it to `insert_pack`.
    pack: RvStr,
    /// Declines to make the instance. This Plugin never calls it.
    refuse: unsafe extern "C" fn(ctx: *mut c_void, reason: RvStr) -> RvStatus,
    /// Adds one tool to the instance.
    add_tool: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus,
}

/// The file functions of an `RvToolCallArgs` that read, in the
/// contract's order. Each takes a request and fills nothing.
#[repr(C)]
#[derive(Debug)]
struct RvFileReads {
    /// The contract's `file_stat`: asks what `path` names.
    stat: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr) -> RvStatus,
    /// The contract's `file_list`: asks for the entries of the directory
    /// at `path`.
    list: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr) -> RvStatus,
    /// The contract's `file_read`: asks for up to `len` bytes of the
    /// file at `path` from `offset` on.
    read: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr, offset: u64, len: u64) -> RvStatus,
}

/// The file functions of an `RvToolCallArgs` that change files, in the
/// contract's order. Each takes a request and fills nothing.
#[repr(C)]
#[derive(Debug)]
struct RvFileChanges {
    /// The contract's `file_write`: asks to write `bytes` as the whole
    /// contents of the file at `path`.
    write: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr, bytes: RvBytes) -> RvStatus,
    /// The contract's `file_append`: asks to add `bytes` to the end of
    /// the file at `path`.
    append: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr, bytes: RvBytes) -> RvStatus,
    /// The contract's `file_mkdir`: asks to make a directory at `path`,
    /// with each absent directory on the way to it when `recursive` is 1.
    mkdir: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr, recursive: u32) -> RvStatus,
    /// The contract's `file_remove`: asks to remove the file or the
    /// directory at `path`, with everything in a directory when
    /// `recursive` is 1.
    remove: unsafe extern "C" fn(ctx: *mut c_void, path: RvStr, recursive: u32) -> RvStatus,
    /// The contract's `file_rename`: asks to move the file or the
    /// directory at `from` to `to`.
    rename: unsafe extern "C" fn(ctx: *mut c_void, from: RvStr, to: RvStr) -> RvStatus,
    /// The contract's `file_copy`: asks to copy the file at `from` to
    /// `to`.
    copy: unsafe extern "C" fn(ctx: *mut c_void, from: RvStr, to: RvStr) -> RvStatus,
}

/// Wakes one tool call. The Plugin may call `wake` on any thread, any
/// number of times, from the call's first step until `RV_PLUGIN_UNLOAD`
/// returns.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvWaker {
    /// The Host's context for the waker, which the Plugin passes back to
    /// `wake` and never reads through.
    ctx: *mut c_void,
    /// Asks the Host to send the call's next step.
    wake: unsafe extern "C" fn(ctx: *mut c_void),
}

/// The arguments of `RV_TOOL_CALL`: the tool, its arguments, the function
/// that ends the call, the file functions, the answer to the file
/// request of the call's last step, the Host's resources, and the call's
/// waker.
///
/// The contract's struct holds the nine file functions as fields of its
/// own. This copy holds them as two structs, because the workspace's
/// structural checks hold a struct to twelve named fields. A `repr(C)`
/// struct of function pointers lies where its fields would lie one by
/// one, so each function is at the contract's offset, which the unit
/// tests pin.
#[repr(C)]
#[derive(Debug)]
struct RvToolCallArgs {
    /// `size_of::<RvToolCallArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this step.
    ctx: *mut c_void,
    /// The tool's name, as the instance gave it to `add_tool`.
    tool: RvStr,
    /// The JSON text of the call's arguments.
    arguments: RvStr,
    /// Ends the call with its result.
    finish: unsafe extern "C" fn(ctx: *mut c_void, result: *const RvToolResult) -> RvStatus,
    /// The contract's `file_stat`, `file_list`, and `file_read`.
    reads: RvFileReads,
    /// The answer to the file request the call's last step made, or NULL
    /// when that step made none.
    answer: *const RvFileAnswer,
    /// The contract's `file_write`, `file_append`, `file_mkdir`,
    /// `file_remove`, `file_rename`, and `file_copy`.
    changes: RvFileChanges,
    /// The Host's resources as they were when the call started: the JSON
    /// text of an object. Every step of one call is lent the same text.
    resources: RvStr,
    /// The waker of this call, the same one at every step.
    waker: RvWaker,
}

/// One call into the Plugin's entry point.
#[repr(C)]
#[derive(Debug)]
struct RvCall {
    /// `size_of::<RvCall>()`.
    size: u32,
    /// The operation.
    verb: RvVerb,
    /// What the `RvHost` table reports on every call.
    host: *const RvHost,
    /// The Plugin instance: NULL going into `RV_PLUGIN_LOAD`, which sets
    /// it.
    plugin: *mut c_void,
    /// One run's instance of a Pack: NULL going into `RV_PACK_NEW`, which
    /// sets it.
    pack_run: *mut c_void,
    /// The verb's own arguments, or NULL for a verb that has none.
    args: *const c_void,
    /// The state of one tool call: NULL going into its first step.
    tool_call: *mut c_void,
}

/// What `RV_PLUGIN_LOAD` makes and `RV_PLUGIN_UNLOAD` frees. It is not
/// zero-sized, so the allocation is real and a leak or a double free
/// shows up under Miri.
struct Instance {
    /// Ballast that gives the instance a size.
    _ballast: [u8; 16],
}

/// What `RV_PACK_NEW` makes and `RV_PACK_DELETE` frees.
struct PackRun {
    /// Ballast that gives the instance a size.
    _ballast: [u8; 16],
}

/// What a tool call that takes more than one step keeps between them.
struct CallState {
    /// How many steps the call has run.
    steps: u64,
    /// How many bytes the reads of a `read_many` call were answered with.
    bytes: u64,
    /// How many reads of a `read_many` call failed.
    failed: u64,
    /// The thread that the first step of a `later` call started, until
    /// the call's second step or the drop of this state has joined it.
    waking: Option<Waking>,
}

/// The thread that wakes a `later` call.
struct Waking {
    /// Set once the thread is about to wake the call.
    woke: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

/// A call's waker, for a thread of the Plugin to wake the call through.
struct ThreadWaker(RvWaker);

// SAFETY: the contract lets a Plugin call `wake` with its waker's `ctx`
// on any thread until `RV_PLUGIN_UNLOAD` returns, and nothing here reads
// through `ctx`, so the pair may move to a thread of the Plugin.
unsafe impl Send for ThreadWaker {}

impl ThreadWaker {
    /// Wakes the call.
    fn wake(&self) {
        // SAFETY: the Host keeps what the `ctx` of a waker points at
        // valid until `RV_PLUGIN_UNLOAD` returns, and the state of the
        // call joins the thread that calls this before then.
        unsafe { (self.0.wake)(self.0.ctx) };
    }
}

#[cfg(test)]
thread_local! {
    /// How many `Instance` values this thread has made and not yet
    /// dropped. The unit tests read it to prove that a failing load and an
    /// unload each free the instance.
    static LIVE_INSTANCES: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Successful `RV_PLUGIN_LOAD` calls since this copy of the library was
/// opened.
static LOADS: AtomicU32 = AtomicU32::new(0);
/// `RV_PLUGIN_UNLOAD` calls that freed an instance since this copy of the
/// library was opened.
static UNLOADS: AtomicU32 = AtomicU32::new(0);
/// `Instance` values made and not yet dropped, across every thread.
static LIVE: AtomicU32 = AtomicU32::new(0);
/// `PackRun` values made and not yet dropped, across every thread.
static PACKS: AtomicU32 = AtomicU32::new(0);
/// `CallState` values made and not yet dropped, across every thread.
static CALLS: AtomicU32 = AtomicU32::new(0);
/// `RV_TOOL_CALL` steps that named a tool since this copy of the library
/// was opened.
static STEPPED: AtomicU32 = AtomicU32::new(0);

impl Instance {
    /// Makes one instance and counts it as live.
    fn new() -> Instance {
        LIVE.fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() + 1);
        Instance { _ballast: [0; 16] }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
        #[cfg(test)]
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() - 1);
    }
}

impl PackRun {
    /// Makes one Pack instance and counts it as live.
    fn new() -> PackRun {
        PACKS.fetch_add(1, Ordering::Relaxed);
        PackRun { _ballast: [0; 16] }
    }
}

impl Drop for PackRun {
    fn drop(&mut self) {
        PACKS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl CallState {
    /// Makes the state of one call and counts it as live.
    fn new() -> CallState {
        CALLS.fetch_add(1, Ordering::Relaxed);
        CallState {
            steps: 0,
            bytes: 0,
            failed: 0,
            waking: None,
        }
    }
}

impl Drop for CallState {
    fn drop(&mut self) {
        // A Plugin has no thread left when it is unloaded, so the state
        // of a call waits here for the thread the call started.
        if let Some(waking) = self.waking.take() {
            let _joined = waking.thread.join();
        }
        CALLS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Successful `RV_PLUGIN_LOAD` calls since this copy of the library was
/// opened.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_loads() -> u32 {
    LOADS.load(Ordering::Relaxed)
}

/// `RV_PLUGIN_UNLOAD` calls that freed an instance since this copy of the
/// library was opened.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_unloads() -> u32 {
    UNLOADS.load(Ordering::Relaxed)
}

/// Instances made and not yet dropped: incremented when an instance is
/// made, decremented in its `Drop`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_live() -> u32 {
    LIVE.load(Ordering::Relaxed)
}

/// Pack instances made by `RV_PACK_NEW` and not yet freed by
/// `RV_PACK_DELETE`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_packs() -> u32 {
    PACKS.load(Ordering::Relaxed)
}

/// Tool calls that hold state between steps: made by a call's first step,
/// freed by its last step or by `RV_TOOL_CANCEL`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_calls() -> u32 {
    CALLS.load(Ordering::Relaxed)
}

/// `RV_TOOL_CALL` steps that named a tool since this copy of the library
/// was opened. A test reads it to see that a parked call got no step.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_steps() -> u32 {
    STEPPED.load(Ordering::Relaxed)
}

/// A type's size as the contract's `u32`. The contract's structs are a few
/// dozen bytes, so the fallback is unreachable; it is `u32::MAX` so that a
/// wrong size fails a check instead of panicking.
fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// `text` as the contract's pointer and length.
fn rv_str(text: &str) -> RvStr {
    RvStr {
        ptr: text.as_ptr().cast(),
        len: text.len(),
    }
}

/// The text `s` holds, or the empty string when it is not UTF-8.
///
/// # Safety
/// When `s.len` is nonzero, `s.ptr` must be valid for reads of `s.len`
/// bytes for the lifetime `'a`.
unsafe fn text<'a>(s: RvStr) -> &'a str {
    if s.len == 0 || s.ptr.is_null() {
        return "";
    }
    // SAFETY: `s.ptr` is not NULL, and the caller guarantees it is valid
    // for `s.len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
    std::str::from_utf8(bytes).unwrap_or_default()
}

/// The arguments struct of the verb `call` names: `RV_ERR_MISUSE` when the
/// call has none, and `RV_ERR_UNSUPPORTED` when it is shorter than `T`.
///
/// # Safety
/// `call` must point to a valid `RvCall` whose `args` is NULL or points to
/// one of the contract's arguments structs, each of which begins with its
/// `size`, as long as that `size` says, for the lifetime `'a`.
unsafe fn args<'a, T>(call: *mut RvCall) -> Result<&'a T, RvStatus> {
    // SAFETY: the caller guarantees `call` is valid.
    let args = unsafe { (*call).args }.cast::<T>();
    if args.is_null() {
        return Err(RV_ERR_MISUSE);
    }
    // SAFETY: a non-NULL `args` begins with its `size`, which is readable
    // on its own.
    if unsafe { args.cast::<u32>().read() } < size_of_u32::<T>() {
        return Err(RV_ERR_UNSUPPORTED);
    }
    // SAFETY: `size` says the loader wrote a whole `T`.
    Ok(unsafe { &*args })
}

/// The fixture's Plugin entry point, speaking contract 0.7.
///
/// # Safety
/// `call` must point to a valid `RvCall` made as the C contract requires:
/// its `host` points to a valid `RvHost`, its `args` is NULL or points to
/// the arguments struct of its verb, at least as long as its `size` says,
/// and its Plugin pointers are the ones this entry point set.
#[unsafe(no_mangle)]
#[expect(
    private_interfaces,
    reason = "the C types stay private; only the entry point is exported"
)]
pub unsafe extern "C" fn runvil_plugin(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` points to a valid `RvCall`.
    let verb = unsafe { (*call).verb };
    // SAFETY: in each arm, as above, and the caller vouches for the
    // pointers the verb reads.
    unsafe {
        match verb {
            RV_PLUGIN_LOAD => load(call),
            RV_PLUGIN_UNLOAD => unload(call),
            RV_PACK_NEW => pack_new(call),
            RV_PACK_DELETE => pack_delete(call),
            RV_TOOL_CALL => tool_call(call),
            RV_TOOL_CANCEL => tool_cancel(call),
            _ => RV_ERR_UNSUPPORTED,
        }
    }
}

/// Handles `RV_PLUGIN_LOAD`: checks the `RvHost` version, then the load
/// arguments, makes the instance, describes the Plugin once and inserts
/// its Pack through the load's `ctx`, and only then hands the instance to
/// the loader.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `host` and `args` are as the
/// entry point's own safety contract requires.
unsafe fn load(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let host = unsafe { (*call).host };
    // The version check comes first and reads only `size` until the table
    // is known to be long enough, so a short table is never read past its
    // end. It also runs before `args` is read and before anything is
    // allocated, because another version's `args` has another shape.
    // SAFETY: the caller guarantees `host` points to a valid `RvHost`.
    if unsafe { (*host).size } < size_of_u32::<RvHost>() {
        return RV_ERR_UNSUPPORTED;
    }
    // SAFETY: as above, and the table is long enough to hold the version.
    let version = unsafe { ((*host).api_major, (*host).api_minor) };
    if version != (RV_API_MAJOR, RV_API_MINOR) {
        return RV_ERR_UNSUPPORTED;
    }
    // SAFETY: for this verb `args` is NULL or an `RvLoadArgs`.
    let args = match unsafe { args::<RvLoadArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };

    let instance = Box::new(Instance::new());
    let info = RvPluginInfo {
        size: size_of_u32::<RvPluginInfo>(),
        reserved: 0,
        name: rv_str(NAME),
        version: rv_str(VERSION),
    };
    // SAFETY: `describe_plugin` and `ctx` are the load's own arguments,
    // and `info` and its strings outlive the call.
    let status = unsafe { (args.describe_plugin)(args.ctx, &raw const info) };
    if status != RV_OK {
        // `instance` drops here, so a failing load frees what it made.
        return status;
    }
    let pack = RvPackInfo {
        size: size_of_u32::<RvPackInfo>(),
        reserved: 0,
        name: rv_str(PACK),
        description: rv_str(PACK_DESCRIPTION),
    };
    // SAFETY: as above, for `insert_pack`.
    let status = unsafe { (args.insert_pack)(args.ctx, &raw const pack) };
    if status != RV_OK {
        return status;
    }
    // SAFETY: the caller guarantees `call` is valid, and nothing else
    // reads it meanwhile.
    unsafe { (*call).plugin = Box::into_raw(instance).cast() };
    LOADS.fetch_add(1, Ordering::Relaxed);
    RV_OK
}

/// Handles `RV_PLUGIN_UNLOAD`: frees the instance `RV_PLUGIN_LOAD` set.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `plugin` is NULL or the pointer
/// this entry point's `RV_PLUGIN_LOAD` set, not yet unloaded.
unsafe fn unload(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let plugin = unsafe { (*call).plugin };
    if plugin.is_null() {
        return RV_ERR_MISUSE;
    }
    // SAFETY: a non-NULL `plugin` is the `Box<Instance>` that
    // `RV_PLUGIN_LOAD` leaked, and the loader sends `RV_PLUGIN_UNLOAD`
    // once per load, so this is its only free.
    drop(unsafe { Box::from_raw(plugin.cast::<Instance>()) });
    UNLOADS.fetch_add(1, Ordering::Relaxed);
    RV_OK
}

/// The fixture's tools, in the order `RV_PACK_NEW` adds them: the name,
/// the description, the parameters, and the flags of each.
const TOOLS: [(&str, &str, &str, RvToolFlags); 15] = [
    (
        HELLO,
        "Answers hello and its arguments in one step.",
        NO_PARAMETERS,
        0,
    ),
    (STEPS, "Answers in its third step.", NO_PARAMETERS, 0),
    (NEVER, "Never answers.", NO_PARAMETERS, 0),
    (
        RESOURCES,
        "Answers the resources of the Host in its second step.",
        NO_PARAMETERS,
        0,
    ),
    (
        LATER,
        "Answers once a thread it started has woken its call.",
        NO_PARAMETERS,
        0,
    ),
    (STAT, "Says what a path names.", PATH_PARAMETERS, 0),
    (LIST, "Lists a directory.", PATH_PARAMETERS, 0),
    (READ, "Answers the text of a file.", PATH_PARAMETERS, 0),
    (
        WRITE,
        "Writes a text as the whole of a file.",
        TEXT_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        APPEND,
        "Adds a text to the end of a file.",
        TEXT_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        MKDIR,
        "Makes a directory.",
        RECURSIVE_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        REMOVE,
        "Removes a file or a directory.",
        RECURSIVE_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        RENAME,
        "Moves a file or a directory.",
        PAIR_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        COPY,
        "Copies a file.",
        PAIR_PARAMETERS,
        RV_TOOL_CHANGES_FILES,
    ),
    (
        READ_MANY,
        "Reads many files of a directory, one in each step.",
        MANY_PARAMETERS,
        0,
    ),
];

/// Handles `RV_PACK_NEW`: makes the Pack instance, adds each of its
/// [`TOOLS`] through the call's `ctx`, and only then hands the instance
/// to the loader.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `args` is NULL or an
/// `RvPackNewArgs` as long as its `size` says.
unsafe fn pack_new(call: *mut RvCall) -> RvStatus {
    // SAFETY: for this verb `args` is NULL or an `RvPackNewArgs`.
    let args = match unsafe { args::<RvPackNewArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };
    // SAFETY: the loader keeps the Pack's name valid for the call.
    if unsafe { text(args.pack) } != PACK {
        return RV_ERR_MISUSE;
    }
    let pack_run = Box::new(PackRun::new());
    for (name, description, parameters, flags) in TOOLS {
        let info = RvToolInfo {
            size: size_of_u32::<RvToolInfo>(),
            flags,
            name: rv_str(name),
            wire_name: rv_str(""),
            description: rv_str(description),
            parameters: rv_str(parameters),
        };
        // SAFETY: `add_tool` and `ctx` are the call's own arguments, and
        // `info` and its strings outlive the call.
        let status = unsafe { (args.add_tool)(args.ctx, &raw const info) };
        if status != RV_OK {
            // `pack_run` drops here, so a failing call frees what it made.
            return status;
        }
    }
    // SAFETY: the caller guarantees `call` is valid, and nothing else
    // reads it meanwhile.
    unsafe { (*call).pack_run = Box::into_raw(pack_run).cast() };
    RV_OK
}

/// Handles `RV_PACK_DELETE`: frees the instance `RV_PACK_NEW` set.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `pack_run` is NULL or the
/// pointer this entry point's `RV_PACK_NEW` set, not yet deleted.
unsafe fn pack_delete(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let pack_run = unsafe { (*call).pack_run };
    if pack_run.is_null() {
        return RV_ERR_MISUSE;
    }
    // SAFETY: a non-NULL `pack_run` is the `Box<PackRun>` that
    // `RV_PACK_NEW` leaked, and the loader deletes each instance once.
    drop(unsafe { Box::from_raw(pack_run.cast::<PackRun>()) });
    RV_OK
}

/// Handles `RV_TOOL_CALL`: runs one step of the tool the arguments name.
/// The state of a call is taken out of `call->tool_call` first, so every
/// return other than `RV_PENDING` leaves none behind. A step of `steps`
/// or `resources` that returns `RV_PENDING` wakes its call first, the
/// first step of `later` starts the thread that wakes its call, and a
/// step of `never` wakes nothing.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `tool_call` is NULL or the
/// state the call's last step left, and whose `args` is NULL or an
/// `RvToolCallArgs` as long as its `size` says.
unsafe fn tool_call(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid, and a non-NULL
    // `tool_call` is the `Box<CallState>` the last step leaked.
    let state = unsafe {
        let state = (*call).tool_call.cast::<CallState>();
        (*call).tool_call = std::ptr::null_mut();
        (!state.is_null()).then(|| Box::from_raw(state))
    };
    // SAFETY: for this verb `args` is NULL or an `RvToolCallArgs`.
    let args = match unsafe { args::<RvToolCallArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };
    STEPPED.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the loader keeps the three strings valid for the step.
    let (tool, arguments, resources) =
        unsafe { (text(args.tool), text(args.arguments), text(args.resources)) };
    let mut state = state.unwrap_or_else(|| Box::new(CallState::new()));
    state.steps = state.steps.saturating_add(1);
    // The result the call ends with in this step, or `None` for a call
    // that goes on.
    let ended = match tool {
        HELLO => Some((RV_RESULT_TRUSTED, format!("hello {arguments}"))),
        STEPS if state.steps == 3 => Some((RV_RESULT_TRUSTED, "3 steps".to_owned())),
        RESOURCES if state.steps == 2 => Some((RV_RESULT_TRUSTED, resources.to_owned())),
        STEPS | RESOURCES => {
            // No file request brings the call's next step, so the step
            // wakes the call before it returns `RV_PENDING`.
            // SAFETY: `wake` and `ctx` are the waker this step is lent,
            // which the Host keeps valid until `RV_PLUGIN_UNLOAD`
            // returns.
            unsafe { (args.waker.wake)(args.waker.ctx) };
            None
        }
        LATER => later_step(args.waker, &mut state).map(|said| (RV_RESULT_TRUSTED, said)),
        NEVER => None,
        STAT | LIST | READ | WRITE | APPEND | MKDIR | REMOVE | RENAME | COPY => {
            // SAFETY: `args` is the arguments of this step, which is open
            // on this thread.
            let said = unsafe { file_step(args, tool, arguments, state.steps) };
            said.map(|said| (RV_RESULT_TRUSTED, said))
        }
        READ_MANY => {
            // SAFETY: as above.
            let said = unsafe { read_many_step(args, arguments, &mut state) };
            said.map(|said| (RV_RESULT_TRUSTED, said))
        }
        other => Some((
            RV_RESULT_OTHER,
            format!("test-plugin has no tool named {other}"),
        )),
    };
    let Some((kind, answer)) = ended else {
        // SAFETY: the caller guarantees `call` is valid, and nothing else
        // reads it meanwhile.
        unsafe { (*call).tool_call = Box::into_raw(state).cast() };
        return RV_PENDING;
    };
    // The call ends here, so its state is freed before the step returns.
    drop(state);
    let result = RvToolResult {
        size: size_of_u32::<RvToolResult>(),
        kind,
        text: rv_str(&answer),
    };
    // SAFETY: `finish` and `ctx` are the step's own arguments, and
    // `result` and its text outlive the call.
    unsafe { (args.finish)(args.ctx, &raw const result) }
}

/// Runs one step of a `later` call, which is lent `waker`. The first step
/// starts a thread that wakes the call after [`LATER_DELAY`], and the
/// call goes on. The second step joins that thread and ends the call
/// with whether the thread's wake had come when the step began. Returns
/// the text the call ends with, or `None` for a call that goes on.
fn later_step(waker: RvWaker, state: &mut CallState) -> Option<String> {
    if let Some(waking) = state.waking.take() {
        let woke = waking.woke.load(Ordering::SeqCst);
        let _joined = waking.thread.join();
        let said = if woke {
            "woken"
        } else {
            "stepped before the wake"
        };
        return Some(said.to_owned());
    }
    let woke = Arc::new(AtomicBool::new(false));
    let (flag, waker) = (Arc::clone(&woke), ThreadWaker(waker));
    // `Builder::spawn` reports a thread that cannot start, where
    // `thread::spawn` would panic.
    let started = std::thread::Builder::new().spawn(move || {
        std::thread::sleep(LATER_DELAY);
        // The flag is set first, so the step that this wake asks for
        // finds it set.
        flag.store(true, Ordering::SeqCst);
        waker.wake();
    });
    match started {
        Ok(thread) => {
            state.waking = Some(Waking { woke, thread });
            None
        }
        Err(error) => Some(format!("the waking thread did not start: {error}")),
    }
}

/// The value of the string `key` in `arguments`, the JSON text of an
/// object, or the empty string when it has none. It reads up to the next
/// `"`, so it serves only a value with no `"` and no `\` in it, which is
/// all the tests send.
fn string_of<'a>(arguments: &'a str, key: &str) -> &'a str {
    arguments
        .split_once(&format!(r#""{key}":""#))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map_or("", |(value, _)| value)
}

/// The value of the whole number `key` in `arguments`, the JSON text of
/// an object, or zero when it has none: the digits that follow the key.
fn number_of(arguments: &str, key: &str) -> u64 {
    arguments
        .split_once(&format!(r#""{key}":"#))
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// Whether `arguments`, the JSON text of an object, sets the boolean
/// `key` to true.
fn flag_of(arguments: &str, key: &str) -> bool {
    arguments.contains(&format!(r#""{key}":true"#))
}

/// Runs one step of a call of the file tool `tool`, a call that is in
/// its step numbered `step`, from 1. The first step makes the tool's one
/// request and the call goes on. The second step ends the call with
/// what its answer says. Returns the text the call ends with, or `None`
/// for a call that goes on.
///
/// # Safety
/// `args` must be the arguments of the step open on this thread.
unsafe fn file_step(
    args: &RvToolCallArgs,
    tool: &str,
    arguments: &str,
    step: u64,
) -> Option<String> {
    if step > 1 {
        // SAFETY: the caller guarantees `args`.
        return Some(unsafe { answer_text(args, tool) });
    }
    // SAFETY: as above.
    match unsafe { ask(args, tool, arguments) } {
        RV_OK => None,
        other => Some(format!("the file function returned status {other}")),
    }
}

/// Makes the one request of the file tool `tool` from what `arguments`
/// holds, and returns the status of its file function.
///
/// # Safety
/// `args` must be the arguments of the step open on this thread.
unsafe fn ask(args: &RvToolCallArgs, tool: &str, arguments: &str) -> RvStatus {
    let path = rv_str(string_of(arguments, "path"));
    let (from, to) = (
        rv_str(string_of(arguments, "from")),
        rv_str(string_of(arguments, "to")),
    );
    let written = string_of(arguments, "text");
    let bytes = RvBytes {
        ptr: written.as_ptr(),
        len: written.len(),
    };
    // The contract spells `recursive` as 1 for on and 0 for off.
    let recursive = u32::from(flag_of(arguments, "recursive"));
    let (reads, changes) = (&args.reads, &args.changes);
    // SAFETY: in each arm, the file function and `ctx` are the step's
    // own, and each path and the bytes outlive the call.
    unsafe {
        match tool {
            STAT => (reads.stat)(args.ctx, path),
            LIST => (reads.list)(args.ctx, path),
            READ => (reads.read)(args.ctx, path, 0, u64::MAX),
            WRITE => (changes.write)(args.ctx, path, bytes),
            APPEND => (changes.append)(args.ctx, path, bytes),
            MKDIR => (changes.mkdir)(args.ctx, path, recursive),
            REMOVE => (changes.remove)(args.ctx, path, recursive),
            RENAME => (changes.rename)(args.ctx, from, to),
            _ => (changes.copy)(args.ctx, from, to),
        }
    }
}

/// What the file tool `tool` says of the answer its step is lent: what
/// the answer holds, as text, `done` for a change, or `failed (kind):
/// text` for an operation that failed.
///
/// # Safety
/// `args` must be the arguments of the step open on this thread.
unsafe fn answer_text(args: &RvToolCallArgs, tool: &str) -> String {
    // SAFETY: the loader keeps the answer, and what it points to, valid
    // for the step, and this function copies all of it it reads.
    let Some(answer) = (unsafe { args.answer.as_ref() }) else {
        return "the step was lent no answer".to_owned();
    };
    match answer.status {
        RV_OK => match tool {
            STAT => stat_text(answer.stat),
            // SAFETY: as above, for the entries and their names.
            LIST => unsafe { list_text(lent(answer.entries.ptr, answer.entries.len)) },
            // SAFETY: as above, for the bytes.
            READ => String::from_utf8_lossy(unsafe { lent(answer.bytes.ptr, answer.bytes.len) })
                .into_owned(),
            _ => "done".to_owned(),
        },
        // SAFETY: as above, for the text of the error.
        RV_ERR_FILE => format!("failed ({}): {}", answer.error.kind, unsafe {
            text(answer.error.text)
        }),
        other => format!("the answer has status {other}"),
    }
}

/// Runs one step of a `read_many` call. It counts the answer to the read
/// of the step before, when there was one. Then it asks for the next
/// file and the call goes on, or, once the call has read as many files
/// as its `count` says, it ends the call with how many files it read,
/// how many bytes they held, and how many reads failed. Returns the text
/// the call ends with, or `None` for a call that goes on.
///
/// # Safety
/// `args` must be the arguments of the step open on this thread.
unsafe fn read_many_step(
    args: &RvToolCallArgs,
    arguments: &str,
    state: &mut CallState,
) -> Option<String> {
    // SAFETY: the loader keeps the answer valid for the step.
    if let Some(answer) = unsafe { args.answer.as_ref() } {
        if answer.status == RV_OK {
            let read = u64::try_from(answer.bytes.len).unwrap_or(u64::MAX);
            state.bytes = state.bytes.saturating_add(read);
        } else {
            state.failed = state.failed.saturating_add(1);
        }
    }
    let count = number_of(arguments, "count");
    // The step numbered n, from 1, asks for the file named n - 1.
    let next = state.steps.saturating_sub(1);
    if next >= count {
        return Some(format!(
            "read {count} files, {} bytes, {} failed",
            state.bytes, state.failed
        ));
    }
    let path = format!("{}/{next}", string_of(arguments, "path"));
    // SAFETY: `file_read` and `ctx` are the step's own, and `path`
    // outlives the call.
    match unsafe { (args.reads.read)(args.ctx, rv_str(&path), 0, u64::MAX) } {
        RV_OK => None,
        other => Some(format!("the file function returned status {other}")),
    }
}

/// `stat` as the `stat` tool words it.
fn stat_text(stat: RvStat) -> String {
    match stat.file_type {
        RV_FILE_TYPE_FILE => format!("file {}", stat.len),
        RV_FILE_TYPE_DIRECTORY => "directory".to_owned(),
        RV_FILE_TYPE_SYMLINK => "symlink".to_owned(),
        _ => "other".to_owned(),
    }
}

/// `entries` as the `list` tool words them: the names joined by `,`, each
/// directory with a `/` after its name.
///
/// # Safety
/// The name of each entry must be valid for reads of its length.
unsafe fn list_text(entries: &[RvDirEntry]) -> String {
    let mut listed = String::new();
    for entry in entries {
        if !listed.is_empty() {
            listed.push(',');
        }
        // SAFETY: the caller guarantees the name.
        listed.push_str(unsafe { text(entry.name) });
        if entry.stat.file_type == RV_FILE_TYPE_DIRECTORY {
            listed.push('/');
        }
    }
    listed
}

/// The `len` values an answer holds at `ptr`, or none when `ptr` is NULL,
/// as it is in a field the answer does not fill.
///
/// # Safety
/// When `ptr` is not NULL and `len` is nonzero, `ptr` must be valid for
/// reads of `len` values for the lifetime `'a`.
unsafe fn lent<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    // SAFETY: `ptr` is not NULL, and the caller guarantees it is valid
    // for `len` values.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Handles `RV_TOOL_CANCEL`: frees the state of an abandoned call.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `tool_call` is NULL or the
/// state the call's last step left.
unsafe fn tool_cancel(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let state = unsafe { (*call).tool_call }.cast::<CallState>();
    if !state.is_null() {
        // SAFETY: a non-NULL `tool_call` is the `Box<CallState>` the last
        // step leaked, and the loader cancels a call once.
        drop(unsafe { Box::from_raw(state) });
    }
    RV_OK
}
