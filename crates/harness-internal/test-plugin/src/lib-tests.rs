//! Tests for the fixture's private copy of the C contract and its entry
//! point. The layout and number tests pin the same values as
//! `runvil-plugin-sys`, because this crate cannot import it. The verb
//! tests call `runvil_plugin` directly with a hand-built `RvHost` table
//! and arguments structs, whose functions record what they were given.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::slice;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

#[cfg(target_pointer_width = "64")]
use std::mem::offset_of;

use super::*;

/// What one `describe_plugin` call was handed, copied.
struct Described {
    ctx: *mut c_void,
    size: u32,
    reserved: u32,
    name: String,
    version: String,
}

thread_local! {
    /// Every `describe_plugin` call this test's thread made.
    static DESCRIBED: RefCell<Vec<Described>> = const { RefCell::new(Vec::new()) };
    /// The status `describe_plugin` answers with.
    static DESCRIBE_STATUS: Cell<RvStatus> = const { Cell::new(RV_OK) };
    /// The name and description of every Pack this test's thread inserted.
    static INSERTED: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// The status `insert_pack` answers with.
    static INSERT_STATUS: Cell<RvStatus> = const { Cell::new(RV_OK) };
    /// The name, wire name, parameters, and flags of every tool this
    /// test's thread added.
    static ADDED: RefCell<Vec<(String, String, String, RvToolFlags)>> =
        const { RefCell::new(Vec::new()) };
    /// How many `add_tool` calls answer `RV_OK` before one answers
    /// `RV_ERR_MISUSE`.
    static ADDS_ALLOWED: Cell<usize> = const { Cell::new(usize::MAX) };
    /// The kind and text of every result this test's thread finished with.
    static FINISHED: RefCell<Vec<(RvResultKind, String)>> = const { RefCell::new(Vec::new()) };
    /// How many times the Plugin called `refuse`, which it never should.
    static REFUSALS: Cell<usize> = const { Cell::new(0) };
    /// Every file request this test's thread took: the name of the file
    /// function and its arguments, as one line.
    static ASKED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The status every file function answers with.
    static ASK_STATUS: Cell<RvStatus> = const { Cell::new(RV_OK) };
    /// How many times the Plugin woke a call on this test's thread.
    static WAKES: Cell<usize> = const { Cell::new(0) };
}

/// Copies the bytes of `text` into a `String`.
fn copy(text: RvStr) -> String {
    if text.len == 0 {
        return String::new();
    }
    // SAFETY: the entry point under test hands over a string that lives
    // for the whole call and holds `len` bytes.
    let bytes = unsafe { slice::from_raw_parts(text.ptr.cast::<u8>(), text.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// The load arguments' `describe_plugin`: records what it was given and
/// answers with `DESCRIBE_STATUS`.
unsafe extern "C" fn describe_plugin(ctx: *mut c_void, info: *const RvPluginInfo) -> RvStatus {
    // SAFETY: the entry point under test passes a live `RvPluginInfo`.
    let info = unsafe { &*info };
    let described = Described {
        ctx,
        size: info.size,
        reserved: info.reserved,
        name: copy(info.name),
        version: copy(info.version),
    };
    DESCRIBED.with_borrow_mut(|all| all.push(described));
    DESCRIBE_STATUS.get()
}

/// The load arguments' `insert_pack`: records the Pack and answers with
/// `INSERT_STATUS`.
unsafe extern "C" fn insert_pack(ctx: *mut c_void, info: *const RvPackInfo) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "insert_pack is handed the load's own ctx");
    // SAFETY: the entry point under test passes a live `RvPackInfo`.
    let info = unsafe { &*info };
    assert_eq!(info.size, size_of_u32::<RvPackInfo>());
    assert_eq!(info.reserved, 0);
    INSERTED.with_borrow_mut(|all| all.push((copy(info.name), copy(info.description))));
    INSERT_STATUS.get()
}

/// The `refuse` of every arguments struct: counts itself, because this
/// Plugin never refuses.
unsafe extern "C" fn refuse(_ctx: *mut c_void, _reason: RvStr) -> RvStatus {
    REFUSALS.set(REFUSALS.get() + 1);
    RV_ERR_REFUSED
}

/// The Pack arguments' `add_tool`: records the tool, and answers `RV_OK`
/// until `ADDS_ALLOWED` runs out.
unsafe extern "C" fn add_tool(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "add_tool is handed the call's own ctx");
    // SAFETY: the entry point under test passes a live `RvToolInfo`.
    let info = unsafe { &*info };
    assert_eq!(info.size, size_of_u32::<RvToolInfo>());
    assert!(info.description.len > 0, "each tool has a description");
    if ADDS_ALLOWED.get() == 0 {
        return RV_ERR_MISUSE;
    }
    ADDS_ALLOWED.set(ADDS_ALLOWED.get() - 1);
    ADDED.with_borrow_mut(|all| {
        all.push((
            copy(info.name),
            copy(info.wire_name),
            copy(info.parameters),
            info.flags,
        ));
    });
    RV_OK
}

/// The step arguments' `finish`: records the result.
unsafe extern "C" fn finish(ctx: *mut c_void, result: *const RvToolResult) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "finish is handed the step's own ctx");
    // SAFETY: the entry point under test passes a live `RvToolResult`.
    let result = unsafe { &*result };
    assert_eq!(result.size, size_of_u32::<RvToolResult>());
    FINISHED.with_borrow_mut(|all| all.push((result.kind, copy(result.text))));
    RV_OK
}

/// Records one file request as `line` and answers with `ASK_STATUS`, as
/// every file function of the test Host does: it takes the request and
/// fills nothing.
fn take(ctx: *mut c_void, line: String) -> RvStatus {
    assert_eq!(
        ctx,
        test_ctx(),
        "a file function is handed the step's own ctx"
    );
    ASKED.with_borrow_mut(|all| all.push(line));
    ASK_STATUS.get()
}

/// Copies `bytes` into a `String`.
fn copy_bytes(bytes: RvBytes) -> String {
    copy(RvStr {
        ptr: bytes.ptr.cast(),
        len: bytes.len,
    })
}

/// The step arguments' `file_stat`.
unsafe extern "C" fn file_stat(ctx: *mut c_void, path: RvStr) -> RvStatus {
    take(ctx, format!("stat {}", copy(path)))
}

/// The step arguments' `file_list`.
unsafe extern "C" fn file_list(ctx: *mut c_void, path: RvStr) -> RvStatus {
    take(ctx, format!("list {}", copy(path)))
}

/// The step arguments' `file_read`.
unsafe extern "C" fn file_read(ctx: *mut c_void, path: RvStr, offset: u64, len: u64) -> RvStatus {
    take(ctx, format!("read {} {offset} {len}", copy(path)))
}

/// The step arguments' `file_write`.
unsafe extern "C" fn file_write(ctx: *mut c_void, path: RvStr, bytes: RvBytes) -> RvStatus {
    take(ctx, format!("write {} {}", copy(path), copy_bytes(bytes)))
}

/// The step arguments' `file_append`.
unsafe extern "C" fn file_append(ctx: *mut c_void, path: RvStr, bytes: RvBytes) -> RvStatus {
    take(ctx, format!("append {} {}", copy(path), copy_bytes(bytes)))
}

/// The step arguments' `file_mkdir`.
unsafe extern "C" fn file_mkdir(ctx: *mut c_void, path: RvStr, recursive: u32) -> RvStatus {
    take(ctx, format!("mkdir {} {recursive}", copy(path)))
}

/// The step arguments' `file_remove`.
unsafe extern "C" fn file_remove(ctx: *mut c_void, path: RvStr, recursive: u32) -> RvStatus {
    take(ctx, format!("remove {} {recursive}", copy(path)))
}

/// The step arguments' `file_rename`.
unsafe extern "C" fn file_rename(ctx: *mut c_void, from: RvStr, to: RvStr) -> RvStatus {
    take(ctx, format!("rename {} {}", copy(from), copy(to)))
}

/// The step arguments' `file_copy`.
unsafe extern "C" fn file_copy(ctx: *mut c_void, from: RvStr, to: RvStr) -> RvStatus {
    take(ctx, format!("copy {} {}", copy(from), copy(to)))
}

/// The file requests this test's thread took, in order.
fn asked() -> Vec<String> {
    ASKED.with_borrow(Clone::clone)
}

/// The `ctx` of the waker the tests lend each step. It is another address
/// than [`test_ctx`], because a waker's `ctx` is its own and not the
/// step's.
fn waker_ctx() -> *mut c_void {
    ptr::without_provenance_mut(0x3a3a)
}

/// The `wake` of the waker the tests lend each step: counts itself.
unsafe extern "C" fn wake(ctx: *mut c_void) {
    assert_eq!(ctx, waker_ctx(), "wake is handed the waker's own ctx");
    WAKES.set(WAKES.get() + 1);
}

/// How many times the Plugin woke a call on this test's thread.
fn wakes() -> usize {
    WAKES.get()
}

/// The resources a step is lent when its test names none.
const NO_RESOURCES: &str = "{}";

/// The text of the one file the answers below hold.
const FILE_TEXT: &str = "hello";

/// The entries the answer to a listing below holds: the file `a.md`, the
/// directory `sub`, and the link `link`.
const ENTRIES: [RvDirEntry; 3] = [
    RvDirEntry {
        name: RvStr {
            ptr: "a.md".as_ptr().cast(),
            len: 4,
        },
        stat: RvStat {
            file_type: RV_FILE_TYPE_FILE,
            reserved: 0,
            len: 5,
        },
    },
    RvDirEntry {
        name: RvStr {
            ptr: "sub".as_ptr().cast(),
            len: 3,
        },
        stat: RvStat {
            file_type: RV_FILE_TYPE_DIRECTORY,
            reserved: 0,
            len: 0,
        },
    },
    RvDirEntry {
        name: RvStr {
            ptr: "link".as_ptr().cast(),
            len: 4,
        },
        stat: RvStat {
            file_type: RV_FILE_TYPE_SYMLINK,
            reserved: 0,
            len: 0,
        },
    },
];

/// An answer whose status is `RV_OK` and whose every other field is
/// zero, as the answer to a change is.
fn done() -> RvFileAnswer {
    RvFileAnswer {
        size: size_of_u32::<RvFileAnswer>(),
        status: RV_OK,
        stat: RvStat {
            file_type: 0,
            reserved: 0,
            len: 0,
        },
        entries: RvDirEntries {
            ptr: ptr::null(),
            len: 0,
        },
        bytes: RvBytes {
            ptr: ptr::null(),
            len: 0,
        },
        error: RvFileError {
            kind: 0,
            reserved: 0,
            text: RvStr {
                ptr: ptr::null(),
                len: 0,
            },
        },
    }
}

/// The answer to a `file_stat` that found `stat`.
fn stat_answer(stat: RvStat) -> RvFileAnswer {
    RvFileAnswer { stat, ..done() }
}

/// The answer to a `file_list` that found `entries`.
fn list_answer(entries: &'static [RvDirEntry]) -> RvFileAnswer {
    RvFileAnswer {
        entries: RvDirEntries {
            ptr: entries.as_ptr(),
            len: entries.len(),
        },
        ..done()
    }
}

/// The answer to a `file_read` that read `bytes`.
fn read_answer(bytes: &'static str) -> RvFileAnswer {
    RvFileAnswer {
        bytes: RvBytes {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        },
        ..done()
    }
}

/// The answer of an operation that found nothing at its path.
fn failed_answer() -> RvFileAnswer {
    RvFileAnswer {
        status: RV_ERR_FILE,
        error: RvFileError {
            kind: 1,
            reserved: 0,
            text: rv_str("nothing is there"),
        },
        ..done()
    }
}

/// A table that reports `size` and version `api_major`.`api_minor`.
fn host_with(size: u32, api_major: u32, api_minor: u32) -> RvHost {
    RvHost {
        size,
        api_major,
        api_minor,
        reserved: 0,
    }
}

/// A table the way the loader builds it.
fn host() -> RvHost {
    host_with(size_of_u32::<RvHost>(), 0, 7)
}

/// The `ctx` the tests hand the Plugin. The Plugin never reads through it,
/// so any address that is not NULL will do.
fn test_ctx() -> *mut c_void {
    ptr::without_provenance_mut(0xc7c7)
}

/// Load arguments that report `size`.
fn args_with(size: u32) -> RvLoadArgs {
    RvLoadArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        refuse,
        describe_plugin,
        insert_pack,
    }
}

/// Load arguments the way the loader builds them.
fn args() -> RvLoadArgs {
    args_with(size_of_u32::<RvLoadArgs>())
}

/// Pack arguments that report `size` and name the Pack `pack`.
fn pack_args(size: u32, pack: &str) -> RvPackNewArgs {
    RvPackNewArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        pack: rv_str(pack),
        refuse,
        add_tool,
    }
}

/// Step arguments that report `size`, for the tool `tool` with
/// `arguments`, which lend the step `answer`, the resources `resources`,
/// and the tests' waker.
fn step_args(
    size: u32,
    tool: &str,
    arguments: &str,
    answer: *const RvFileAnswer,
    resources: &str,
) -> RvToolCallArgs {
    RvToolCallArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        tool: rv_str(tool),
        arguments: rv_str(arguments),
        finish,
        reads: RvFileReads {
            stat: file_stat,
            list: file_list,
            read: file_read,
        },
        answer,
        changes: RvFileChanges {
            write: file_write,
            append: file_append,
            mkdir: file_mkdir,
            remove: file_remove,
            rename: file_rename,
            copy: file_copy,
        },
        resources: rv_str(resources),
        waker: RvWaker {
            ctx: waker_ctx(),
            wake,
        },
    }
}

/// A call that sends `verb` and carries `plugin`, with `host` as its table
/// and `args` as its arguments.
fn call_for<T>(verb: RvVerb, host: &RvHost, args: *const T, plugin: *mut c_void) -> RvCall {
    RvCall {
        size: size_of_u32::<RvCall>(),
        verb,
        host: ptr::from_ref(host),
        plugin,
        pack_run: ptr::null_mut(),
        args: args.cast(),
        tool_call: ptr::null_mut(),
    }
}

/// Sends `call` to the entry point.
fn send(call: &mut RvCall) -> RvStatus {
    // SAFETY: `call` is a live `RvCall` whose table is live and as long as
    // its `size` says, and whose `args` is NULL or live and as long as its
    // `size` says.
    unsafe { runvil_plugin(call) }
}

/// Sends `verb` with no instance, the table `host`, and the arguments
/// `args`, and returns the status and the call.
fn send_verb(verb: RvVerb, host: &RvHost, args: *const RvLoadArgs) -> (RvStatus, RvCall) {
    let mut call = call_for(verb, host, args, ptr::null_mut());
    let status = send(&mut call);
    (status, call)
}

/// Sends `RV_PLUGIN_LOAD` with the table `host` and well-formed arguments.
fn send_load_with(host: &RvHost) -> (RvStatus, RvCall) {
    let args = args();
    send_verb(RV_PLUGIN_LOAD, host, &raw const args)
}

/// Loads an instance with a well-formed table, returning the call that
/// carries it. The call's `args` is dangling afterward, which an unload
/// never reads.
fn loaded(host: &RvHost) -> RvCall {
    let (status, call) = send_load_with(host);
    assert_eq!(status, RV_OK, "the load succeeds");
    call
}

/// Unloads the instance `call` carries.
fn unload(call: &mut RvCall) -> RvStatus {
    call.verb = RV_PLUGIN_UNLOAD;
    call.args = ptr::null();
    call.pack_run = ptr::null_mut();
    call.tool_call = ptr::null_mut();
    send(call)
}

/// Sends `RV_PACK_NEW` for the fixture's Pack on the loaded `call`, which
/// then carries the Pack instance.
fn new_pack(call: &mut RvCall) -> RvStatus {
    let args = pack_args(size_of_u32::<RvPackNewArgs>(), PACK);
    call.verb = RV_PACK_NEW;
    call.args = (&raw const args).cast();
    send(call)
}

/// Sends `RV_PACK_DELETE` for the Pack instance `call` carries.
fn delete_pack(call: &mut RvCall) -> RvStatus {
    call.verb = RV_PACK_DELETE;
    call.args = ptr::null();
    let status = send(call);
    call.pack_run = ptr::null_mut();
    status
}

/// Sends one `RV_TOOL_CALL` step of `tool` with `arguments` on the Pack
/// instance that `call` holds, a step that follows no file request and
/// is lent the resources `resources`.
fn step_lent(call: &mut RvCall, tool: &str, arguments: &str, resources: &str) -> RvStatus {
    let args = step_args(
        size_of_u32::<RvToolCallArgs>(),
        tool,
        arguments,
        ptr::null(),
        resources,
    );
    call.verb = RV_TOOL_CALL;
    call.args = (&raw const args).cast();
    send(call)
}

/// Sends one `RV_TOOL_CALL` step of `tool` with `arguments` on the Pack
/// instance `call` carries, a step that follows no file request.
fn step(call: &mut RvCall, tool: &str, arguments: &str) -> RvStatus {
    step_lent(call, tool, arguments, NO_RESOURCES)
}

/// Sends one `RV_TOOL_CALL` step of `tool` with `arguments` on the Pack
/// instance `call` carries, a step that is lent `answer`.
fn step_with(call: &mut RvCall, tool: &str, arguments: &str, answer: &RvFileAnswer) -> RvStatus {
    let args = step_args(
        size_of_u32::<RvToolCallArgs>(),
        tool,
        arguments,
        ptr::from_ref(answer),
        NO_RESOURCES,
    );
    call.verb = RV_TOOL_CALL;
    call.args = (&raw const args).cast();
    send(call)
}

/// Sends `RV_TOOL_CANCEL` for the call state `call` carries.
fn cancel(call: &mut RvCall) -> RvStatus {
    call.verb = RV_TOOL_CANCEL;
    call.args = ptr::null();
    let status = send(call);
    call.tool_call = ptr::null_mut();
    status
}

/// How many times the Plugin called `describe_plugin`.
fn describes() -> usize {
    DESCRIBED.with_borrow(Vec::len)
}

/// How many instances this test's thread has made and not yet freed.
fn live_instances() -> isize {
    LIVE_INSTANCES.get()
}

/// The results this test's thread finished with, in order.
fn finished() -> Vec<(RvResultKind, String)> {
    FINISHED.with_borrow(Clone::clone)
}

#[test]
#[cfg(target_pointer_width = "64")]
fn a_string_is_a_pointer_then_a_length() {
    assert_eq!(size_of::<RvStr>(), 16);
    assert_eq!(offset_of!(RvStr, ptr), 0);
    assert_eq!(offset_of!(RvStr, len), 8);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_infos_and_the_result_keep_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvPluginInfo>(), 40);
    assert_eq!(offset_of!(RvPluginInfo, size), 0);
    assert_eq!(offset_of!(RvPluginInfo, reserved), 4);
    assert_eq!(offset_of!(RvPluginInfo, name), 8);
    assert_eq!(offset_of!(RvPluginInfo, version), 24);

    assert_eq!(size_of::<RvPackInfo>(), 40);
    assert_eq!(offset_of!(RvPackInfo, size), 0);
    assert_eq!(offset_of!(RvPackInfo, reserved), 4);
    assert_eq!(offset_of!(RvPackInfo, name), 8);
    assert_eq!(offset_of!(RvPackInfo, description), 24);

    assert_eq!(size_of::<RvToolInfo>(), 72);
    assert_eq!(offset_of!(RvToolInfo, size), 0);
    assert_eq!(offset_of!(RvToolInfo, flags), 4);
    assert_eq!(offset_of!(RvToolInfo, name), 8);
    assert_eq!(offset_of!(RvToolInfo, wire_name), 24);
    assert_eq!(offset_of!(RvToolInfo, description), 40);
    assert_eq!(offset_of!(RvToolInfo, parameters), 56);

    assert_eq!(size_of::<RvToolResult>(), 24);
    assert_eq!(offset_of!(RvToolResult, size), 0);
    assert_eq!(offset_of!(RvToolResult, kind), 4);
    assert_eq!(offset_of!(RvToolResult, text), 8);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn what_a_file_answer_holds_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvBytes>(), 16);
    assert_eq!(offset_of!(RvBytes, ptr), 0);
    assert_eq!(offset_of!(RvBytes, len), 8);

    assert_eq!(size_of::<RvStat>(), 16);
    assert_eq!(offset_of!(RvStat, file_type), 0);
    assert_eq!(offset_of!(RvStat, reserved), 4);
    assert_eq!(offset_of!(RvStat, len), 8);

    assert_eq!(size_of::<RvDirEntry>(), 32);
    assert_eq!(offset_of!(RvDirEntry, name), 0);
    assert_eq!(offset_of!(RvDirEntry, stat), 16);

    assert_eq!(size_of::<RvDirEntries>(), 16);
    assert_eq!(offset_of!(RvDirEntries, ptr), 0);
    assert_eq!(offset_of!(RvDirEntries, len), 8);

    assert_eq!(size_of::<RvFileError>(), 24);
    assert_eq!(offset_of!(RvFileError, kind), 0);
    assert_eq!(offset_of!(RvFileError, reserved), 4);
    assert_eq!(offset_of!(RvFileError, text), 8);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_file_answer_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvFileAnswer>(), 80);
    assert_eq!(offset_of!(RvFileAnswer, size), 0);
    assert_eq!(offset_of!(RvFileAnswer, status), 4);
    assert_eq!(offset_of!(RvFileAnswer, stat), 8);
    assert_eq!(offset_of!(RvFileAnswer, entries), 24);
    assert_eq!(offset_of!(RvFileAnswer, bytes), 40);
    assert_eq!(offset_of!(RvFileAnswer, error), 56);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_host_table_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvHost>(), 16);
    assert_eq!(offset_of!(RvHost, size), 0);
    assert_eq!(offset_of!(RvHost, api_major), 4);
    assert_eq!(offset_of!(RvHost, api_minor), 8);
    assert_eq!(offset_of!(RvHost, reserved), 12);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_arguments_structs_keep_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvLoadArgs>(), 40);
    assert_eq!(offset_of!(RvLoadArgs, size), 0);
    assert_eq!(offset_of!(RvLoadArgs, reserved), 4);
    assert_eq!(offset_of!(RvLoadArgs, ctx), 8);
    assert_eq!(offset_of!(RvLoadArgs, refuse), 16);
    assert_eq!(offset_of!(RvLoadArgs, describe_plugin), 24);
    assert_eq!(offset_of!(RvLoadArgs, insert_pack), 32);

    assert_eq!(size_of::<RvPackNewArgs>(), 48);
    assert_eq!(offset_of!(RvPackNewArgs, size), 0);
    assert_eq!(offset_of!(RvPackNewArgs, reserved), 4);
    assert_eq!(offset_of!(RvPackNewArgs, ctx), 8);
    assert_eq!(offset_of!(RvPackNewArgs, pack), 16);
    assert_eq!(offset_of!(RvPackNewArgs, refuse), 32);
    assert_eq!(offset_of!(RvPackNewArgs, add_tool), 40);

    assert_eq!(size_of::<RvWaker>(), 16);
    assert_eq!(offset_of!(RvWaker, ctx), 0);
    assert_eq!(offset_of!(RvWaker, wake), 8);

    assert_eq!(size_of::<RvToolCallArgs>(), 168);
    assert_eq!(offset_of!(RvToolCallArgs, size), 0);
    assert_eq!(offset_of!(RvToolCallArgs, reserved), 4);
    assert_eq!(offset_of!(RvToolCallArgs, ctx), 8);
    assert_eq!(offset_of!(RvToolCallArgs, tool), 16);
    assert_eq!(offset_of!(RvToolCallArgs, arguments), 32);
    assert_eq!(offset_of!(RvToolCallArgs, finish), 48);
    // The two structs of file functions leave each function where the
    // contract's one struct has it.
    assert_eq!(offset_of!(RvToolCallArgs, reads.stat), 56);
    assert_eq!(offset_of!(RvToolCallArgs, reads.list), 64);
    assert_eq!(offset_of!(RvToolCallArgs, reads.read), 72);
    assert_eq!(offset_of!(RvToolCallArgs, answer), 80);
    assert_eq!(offset_of!(RvToolCallArgs, changes.write), 88);
    assert_eq!(offset_of!(RvToolCallArgs, changes.append), 96);
    assert_eq!(offset_of!(RvToolCallArgs, changes.mkdir), 104);
    assert_eq!(offset_of!(RvToolCallArgs, changes.remove), 112);
    assert_eq!(offset_of!(RvToolCallArgs, changes.rename), 120);
    assert_eq!(offset_of!(RvToolCallArgs, changes.copy), 128);
    assert_eq!(offset_of!(RvToolCallArgs, resources), 136);
    assert_eq!(offset_of!(RvToolCallArgs, waker), 152);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_call_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvCall>(), 48);
    assert_eq!(offset_of!(RvCall, size), 0);
    assert_eq!(offset_of!(RvCall, verb), 4);
    assert_eq!(offset_of!(RvCall, host), 8);
    assert_eq!(offset_of!(RvCall, plugin), 16);
    assert_eq!(offset_of!(RvCall, pack_run), 24);
    assert_eq!(offset_of!(RvCall, args), 32);
    assert_eq!(offset_of!(RvCall, tool_call), 40);
}

#[test]
fn the_status_codes_the_result_kinds_and_the_tool_flag_keep_their_numbers() {
    assert_eq!(RV_OK, 0);
    assert_eq!(RV_ERR_MISUSE, 4);
    assert_eq!(RV_ERR_REFUSED, 5);
    assert_eq!(RV_ERR_UNSUPPORTED, 6);
    assert_eq!(RV_PENDING, 7);
    assert_eq!(RV_ERR_FILE, 8);
    assert_eq!(RV_RESULT_TRUSTED, 1);
    assert_eq!(RV_RESULT_OTHER, 5);
    assert_eq!(RV_FILE_TYPE_FILE, 1);
    assert_eq!(RV_FILE_TYPE_DIRECTORY, 2);
    assert_eq!(RV_FILE_TYPE_SYMLINK, 3);
    assert_eq!(RV_TOOL_CHANGES_FILES, 1);
}

#[test]
fn the_verbs_and_the_version_keep_their_numbers() {
    assert_eq!(RV_PLUGIN_LOAD, 1);
    assert_eq!(RV_PLUGIN_UNLOAD, 2);
    assert_eq!(RV_PACK_NEW, 3);
    assert_eq!(RV_PACK_DELETE, 4);
    assert_eq!(RV_TOOL_CALL, 5);
    assert_eq!(RV_TOOL_CANCEL, 6);
    assert_eq!(RV_API_MAJOR, 0);
    assert_eq!(RV_API_MINOR, 7);
}

#[test]
fn a_load_describes_test_plugin_once_inserts_its_pack_and_sets_the_instance() {
    let host = host();
    let mut call = loaded(&host);
    assert!(
        !call.plugin.is_null(),
        "a successful load sets the instance pointer"
    );
    DESCRIBED.with_borrow(|all| {
        assert_eq!(all.len(), 1, "the load describes the Plugin exactly once");
        let described = &all[0];
        assert_eq!(described.name, "test-plugin");
        assert_eq!(described.version, "0.1.0");
        assert_eq!(described.size, size_of_u32::<RvPluginInfo>());
        assert_eq!(described.reserved, 0);
        assert_eq!(
            described.ctx,
            test_ctx(),
            "describe_plugin is handed the load's own ctx"
        );
    });
    INSERTED.with_borrow(|all| {
        assert_eq!(
            *all,
            [(
                "fixture/test".to_owned(),
                "The loader's test fixture".to_owned()
            )],
            "the load inserts its one Pack exactly once"
        );
    });
    assert_eq!(REFUSALS.get(), 0, "the Plugin never refuses");
    assert_eq!(live_instances(), 1, "a successful load keeps its instance");
    assert_eq!(unload(&mut call), RV_OK);
    assert_eq!(live_instances(), 0, "the unload frees that instance");
}

#[test]
fn an_unload_frees_the_instance_the_load_set_and_describes_nothing() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(live_instances(), 1, "the load made one instance");
    let before = describes();
    assert_eq!(unload(&mut call), RV_OK);
    assert_eq!(
        live_instances(),
        0,
        "the unload drops the instance the load set, exactly once"
    );
    assert_eq!(
        describes(),
        before,
        "an unload names nobody: the load functions answer only a load"
    );
    assert_eq!(REFUSALS.get(), 0);
}

#[test]
fn an_unload_with_no_instance_is_misuse() {
    let (status, call) = send_verb(RV_PLUGIN_UNLOAD, &host(), ptr::null());
    assert_eq!(status, RV_ERR_MISUSE, "a NULL plugin has nothing to free");
    assert!(call.plugin.is_null());
}

#[test]
fn the_reserved_and_unknown_verbs_are_unsupported() {
    for verb in [0, 7, 8, u32::MAX] {
        let (status, call) = send_verb(verb, &host(), ptr::null());
        assert_eq!(status, RV_ERR_UNSUPPORTED, "verb {verb}");
        assert!(call.plugin.is_null(), "verb {verb} sets no instance");
    }
    assert_eq!(describes(), 0, "no other verb names the Plugin");
}

#[test]
fn a_host_reporting_version_0_2_is_unsupported_and_leaves_no_instance() {
    let (status, call) = send_load_with(&host_with(size_of_u32::<RvHost>(), 0, 2));
    assert_eq!(status, RV_ERR_UNSUPPORTED);
    assert!(call.plugin.is_null());
    assert_eq!(describes(), 0, "a 0.2 Host is never called back");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_table_that_is_too_short_or_not_version_0_7_is_unsupported_and_leaves_no_instance() {
    let full = size_of_u32::<RvHost>();
    let short = [0, 4, 8, 12, full - 1].map(|size| (size, 0, 7));
    let versions = [
        (1, 7),
        (2, 7),
        (u32::MAX, 7),
        (0, 0),
        (0, 5),
        (0, 6),
        (0, 8),
        (0, u32::MAX),
    ]
    .map(|(major, minor)| (full, major, minor));
    for (size, api_major, api_minor) in short.into_iter().chain(versions) {
        let (status, call) = send_load_with(&host_with(size, api_major, api_minor));
        assert_eq!(
            status, RV_ERR_UNSUPPORTED,
            "a table of size {size} and version {api_major}.{api_minor}"
        );
        assert!(
            call.plugin.is_null(),
            "a table of size {size} and version {api_major}.{api_minor} leaves call.plugin NULL"
        );
    }
    assert_eq!(describes(), 0, "an unsupported table is never called back");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_table_longer_than_the_one_the_fixture_knows_still_loads() {
    let host = host_with(size_of_u32::<RvHost>() + 32, 0, 7);
    let mut call = loaded(&host);
    assert!(!call.plugin.is_null());
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_load_with_null_args_is_misuse_and_leaves_no_instance() {
    let (status, call) = send_verb(RV_PLUGIN_LOAD, &host(), ptr::null());
    assert_eq!(status, RV_ERR_MISUSE, "a load needs its RvLoadArgs");
    assert!(call.plugin.is_null());
    assert_eq!(describes(), 0);
    assert_eq!(live_instances(), 0);
}

#[test]
fn short_load_arguments_are_unsupported_and_leave_no_instance() {
    let full = size_of_u32::<RvLoadArgs>();
    for size in [0, 4, 8, 16, 24, 32, full - 1] {
        let args = args_with(size);
        let (status, call) = send_verb(RV_PLUGIN_LOAD, &host(), &raw const args);
        assert_eq!(status, RV_ERR_UNSUPPORTED, "load arguments of size {size}");
        assert!(
            call.plugin.is_null(),
            "load arguments of size {size} leave call.plugin NULL"
        );
    }
    assert_eq!(describes(), 0, "short load arguments are never called");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_host_that_is_not_0_7_is_rejected_before_null_args_are_noticed() {
    let (status, _call) = send_verb(
        RV_PLUGIN_LOAD,
        &host_with(size_of_u32::<RvHost>(), 0, 2),
        ptr::null(),
    );
    assert_eq!(
        status, RV_ERR_UNSUPPORTED,
        "the version is checked before args is read"
    );
}

#[test]
fn a_describe_plugin_that_fails_fails_the_load_with_its_status_and_no_instance() {
    for failing in [RV_ERR_MISUSE, 77] {
        DESCRIBE_STATUS.set(failing);
        let (status, call) = send_load_with(&host());
        assert_eq!(status, failing, "the load returns describe_plugin's status");
        assert!(
            call.plugin.is_null(),
            "a failing load hands the loader no instance, because it made one and freed it"
        );
        assert_eq!(
            live_instances(),
            0,
            "a failing load frees the instance it made with status {failing}"
        );
    }
    assert_eq!(describes(), 2, "each failing load still described once");
}

#[test]
fn an_insert_pack_that_fails_fails_the_load_with_its_status_and_no_instance() {
    INSERT_STATUS.set(RV_ERR_MISUSE);
    let (status, call) = send_load_with(&host());
    assert_eq!(
        status, RV_ERR_MISUSE,
        "the load returns insert_pack's status"
    );
    assert!(call.plugin.is_null());
    assert_eq!(
        live_instances(),
        0,
        "a failing load frees the instance it made"
    );
}

#[test]
fn a_pack_instance_adds_its_tools_with_a_flag_on_each_that_changes_files_and_is_deleted_once() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert!(!call.pack_run.is_null(), "a successful call sets pack_run");
    ADDED.with_borrow(|all| {
        let tools: Vec<(&str, &str, &str, RvToolFlags)> = all
            .iter()
            .map(|(name, wire, parameters, flags)| {
                (name.as_str(), wire.as_str(), parameters.as_str(), *flags)
            })
            .collect();
        assert_eq!(
            tools,
            [
                ("hello", "", NO_PARAMETERS, 0),
                ("steps", "", NO_PARAMETERS, 0),
                ("never", "", NO_PARAMETERS, 0),
                ("resources", "", NO_PARAMETERS, 0),
                ("later", "", NO_PARAMETERS, 0),
                ("stat", "", PATH_PARAMETERS, 0),
                ("list", "", PATH_PARAMETERS, 0),
                ("read", "", PATH_PARAMETERS, 0),
                ("write", "", TEXT_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("append", "", TEXT_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("mkdir", "", RECURSIVE_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("remove", "", RECURSIVE_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("rename", "", PAIR_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("copy", "", PAIR_PARAMETERS, RV_TOOL_CHANGES_FILES),
                ("read_many", "", MANY_PARAMETERS, 0),
            ]
        );
    });
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(REFUSALS.get(), 0, "the Plugin never refuses");
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_pack_instance_whose_tool_is_rejected_is_freed_and_never_handed_over() {
    let host = host();
    let mut call = loaded(&host);
    ADDS_ALLOWED.set(1);
    assert_eq!(new_pack(&mut call), RV_ERR_MISUSE);
    assert!(call.pack_run.is_null());
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_pack_verb_with_the_wrong_arguments_or_no_instance_makes_nothing() {
    let host = host();
    let mut call = loaded(&host);
    call.verb = RV_PACK_NEW;
    call.args = ptr::null();
    assert_eq!(
        send(&mut call),
        RV_ERR_MISUSE,
        "RV_PACK_NEW needs its arguments"
    );

    let short = pack_args(size_of_u32::<RvPackNewArgs>() - 1, PACK);
    call.args = (&raw const short).cast();
    assert_eq!(send(&mut call), RV_ERR_UNSUPPORTED, "short arguments");

    let other = pack_args(size_of_u32::<RvPackNewArgs>(), "fixture/other");
    call.args = (&raw const other).cast();
    assert_eq!(send(&mut call), RV_ERR_MISUSE, "a Pack it never inserted");

    assert!(call.pack_run.is_null());
    assert_eq!(delete_pack(&mut call), RV_ERR_MISUSE, "nothing to delete");
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_hello_tool_finishes_in_one_step_with_its_arguments() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "hello", r#"{"a":1}"#), RV_OK);
    assert_eq!(
        finished(),
        [(RV_RESULT_TRUSTED, r#"hello {"a":1}"#.to_owned())]
    );
    assert!(
        call.tool_call.is_null(),
        "a call that ended leaves no state"
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_steps_tool_wakes_its_call_before_each_pending_step_and_finishes_in_its_third_step() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "steps", "{}"), RV_PENDING);
    assert_eq!(wakes(), 1, "the first step woke its call");
    let state = call.tool_call;
    assert!(!state.is_null(), "a pending call keeps its state");
    assert_eq!(step(&mut call, "steps", "{}"), RV_PENDING);
    assert_eq!(wakes(), 2, "the second step woke its call");
    assert_eq!(call.tool_call, state, "the same state comes back");
    assert_eq!(finished(), [], "a pending step finishes nothing");
    assert_eq!(step(&mut call, "steps", "{}"), RV_OK);
    assert_eq!(wakes(), 2, "the step that ends the call wakes nothing");
    assert_eq!(finished(), [(RV_RESULT_TRUSTED, "3 steps".to_owned())]);
    assert!(call.tool_call.is_null(), "the last step frees the state");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_never_tool_stays_pending_and_wakes_nothing_until_a_cancel_frees_its_state() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    for _ in 0..5 {
        assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    }
    assert_eq!(
        wakes(),
        0,
        "its call stays parked until the Host abandons it"
    );
    assert!(!call.tool_call.is_null());
    assert_eq!(cancel(&mut call), RV_OK);
    assert_eq!(
        cancel(&mut call),
        RV_OK,
        "a cancel with no state frees nothing"
    );
    assert_eq!(finished(), []);
    assert_eq!(wakes(), 0, "the cancel wakes nothing either");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_resources_tool_wakes_its_call_and_answers_what_its_second_step_is_lent() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    let lent = r#"{"runvil/web-search":{"version":1}}"#;
    assert_eq!(step_lent(&mut call, "resources", "{}", lent), RV_PENDING);
    assert_eq!(wakes(), 1, "no file request brings the second step");
    assert_eq!(finished(), [], "the first step finishes nothing");
    assert!(!call.tool_call.is_null(), "a pending call keeps its state");
    // A Host lends every step of a call the same text. The answer is the
    // text of the second step, so a test sees that a later step is lent
    // it too.
    assert_eq!(step_lent(&mut call, "resources", "{}", lent), RV_OK);
    assert_eq!(finished(), [(RV_RESULT_TRUSTED, lent.to_owned())]);
    assert_eq!(wakes(), 1, "the step that ends the call wakes nothing");
    assert!(call.tool_call.is_null(), "the last step frees the state");

    FINISHED.with_borrow_mut(Vec::clear);
    assert_eq!(step(&mut call, "resources", "{}"), RV_PENDING);
    assert_eq!(step(&mut call, "resources", "{}"), RV_OK);
    assert_eq!(
        finished(),
        [(RV_RESULT_TRUSTED, "{}".to_owned())],
        "a Host that offers nothing lends an empty object"
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

/// Sends one `RV_TOOL_CALL` step of the `later` tool on the Pack instance
/// that `call` holds, a step that is lent a waker whose `wake` is `wake`.
/// The tool wakes from a thread of its own, where the counter of the
/// tests' own waker does not reach, so each test lends a waker that
/// counts in a static of that test.
fn step_later(call: &mut RvCall, wake: unsafe extern "C" fn(*mut c_void)) -> RvStatus {
    let args = RvToolCallArgs {
        waker: RvWaker {
            ctx: waker_ctx(),
            wake,
        },
        ..step_args(
            size_of_u32::<RvToolCallArgs>(),
            "later",
            "{}",
            ptr::null(),
            NO_RESOURCES,
        )
    };
    call.verb = RV_TOOL_CALL;
    call.args = (&raw const args).cast();
    send(call)
}

#[test]
fn the_later_tool_is_woken_by_a_thread_it_started_and_then_answers_woken() {
    /// How many times the thread of this test's call woke it.
    static WOKEN: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn count(ctx: *mut c_void) {
        assert_eq!(ctx, waker_ctx(), "wake is handed the waker's own ctx");
        WOKEN.fetch_add(1, Ordering::SeqCst);
    }

    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step_later(&mut call, count), RV_PENDING);
    assert!(!call.tool_call.is_null(), "a pending call keeps its state");
    assert_eq!(finished(), [], "the first step finishes nothing");

    // A Host sends the second step once the call is woken.
    let started = Instant::now();
    while WOKEN.load(Ordering::SeqCst) == 0 {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the thread of the `later` tool did not wake its call within 60 seconds"
        );
        std::thread::yield_now();
    }
    assert_eq!(step_later(&mut call, count), RV_OK);
    assert_eq!(finished(), [(RV_RESULT_TRUSTED, "woken".to_owned())]);
    assert_eq!(WOKEN.load(Ordering::SeqCst), 1, "the thread woke once");
    assert!(call.tool_call.is_null(), "the last step frees the state");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_cancel_of_a_later_call_waits_for_the_thread_the_call_started() {
    /// How many times the thread of this test's call woke it.
    static WOKEN: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn count(ctx: *mut c_void) {
        assert_eq!(ctx, waker_ctx(), "wake is handed the waker's own ctx");
        WOKEN.fetch_add(1, Ordering::SeqCst);
    }

    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step_later(&mut call, count), RV_PENDING);
    assert_eq!(cancel(&mut call), RV_OK);
    assert_eq!(
        WOKEN.load(Ordering::SeqCst),
        1,
        "the cancel joined the thread, so the thread's wake had come when it returned, and no \
         thread of the Plugin is left"
    );
    assert_eq!(finished(), [], "an abandoned call finishes nothing");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_tool_the_pack_does_not_have_fails_the_call() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "absent", "{}"), RV_OK);
    assert_eq!(
        finished(),
        [(
            RV_RESULT_OTHER,
            "test-plugin has no tool named absent".to_owned()
        )]
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

/// The JSON text of the arguments that name `path`.
fn path_arguments(path: &str) -> String {
    format!(r#"{{"path":"{path}"}}"#)
}

/// Runs a call of the file tool `tool` with `arguments` to its end: the
/// first step must make the one request `asked_for`, and the second is
/// lent `answer`. Returns the text the call finished with, which must be
/// a trusted result.
fn file_tool(tool: &str, arguments: &str, asked_for: &str, answer: &RvFileAnswer) -> String {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, tool, arguments), RV_PENDING, "{tool}");
    assert_eq!(asked(), [asked_for], "{tool} makes its one request");
    assert_eq!(
        finished(),
        [],
        "{tool}: the step that asks finishes nothing"
    );
    assert!(
        !call.tool_call.is_null(),
        "{tool}: a call that waits for an answer keeps its state"
    );
    assert_eq!(
        step_with(&mut call, tool, arguments, answer),
        RV_OK,
        "{tool}"
    );
    assert_eq!(asked().len(), 1, "{tool}: the second step asks for nothing");
    assert!(call.tool_call.is_null(), "{tool}: the call left no state");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
    let mut finished = finished();
    assert_eq!(finished.len(), 1, "{tool} finishes once");
    let (kind, text) = finished.remove(0);
    assert_eq!(kind, RV_RESULT_TRUSTED, "{tool}");
    ASKED.with_borrow_mut(Vec::clear);
    FINISHED.with_borrow_mut(Vec::clear);
    text
}

#[test]
fn the_stat_tool_asks_what_its_path_names_and_words_the_stat_of_its_answer() {
    let said = file_tool(
        "stat",
        &path_arguments("/notes/a.md"),
        "stat /notes/a.md",
        &stat_answer(ENTRIES[0].stat),
    );
    assert_eq!(said, "file 5");
}

#[test]
fn the_stat_tool_names_a_directory_a_link_and_anything_else() {
    let pipe = RvStat {
        file_type: 0,
        reserved: 0,
        len: 0,
    };
    for (stat, said) in [
        (ENTRIES[1].stat, "directory"),
        (ENTRIES[2].stat, "symlink"),
        (pipe, "other"),
    ] {
        assert_eq!(
            file_tool("stat", &path_arguments("/x"), "stat /x", &stat_answer(stat)),
            said
        );
    }
}

#[test]
fn the_list_tool_joins_the_names_of_its_answer_and_marks_each_directory() {
    // A borrow of a constant is promoted to a static, so the entries
    // outlive the answer that points at them.
    let entries: &'static [RvDirEntry] = &ENTRIES;
    let said = file_tool(
        "list",
        &path_arguments("/notes"),
        "list /notes",
        &list_answer(entries),
    );
    assert_eq!(said, "a.md,sub/,link");
}

#[test]
fn the_list_tool_answers_nothing_for_an_empty_directory() {
    assert_eq!(
        file_tool("list", &path_arguments("/"), "list /", &done()),
        ""
    );
}

#[test]
fn the_read_tool_asks_for_the_whole_file_and_answers_the_text_of_its_answer() {
    let said = file_tool(
        "read",
        &path_arguments("/notes/a.md"),
        &format!("read /notes/a.md 0 {}", u64::MAX),
        &read_answer(FILE_TEXT),
    );
    assert_eq!(said, FILE_TEXT);
}

#[test]
fn each_tool_that_changes_files_makes_its_request_and_answers_done() {
    for (tool, arguments, asked_for) in [
        (
            "write",
            r#"{"path":"/a","text":"one two"}"#,
            "write /a one two",
        ),
        ("append", r#"{"path":"/a","text":"more"}"#, "append /a more"),
        ("mkdir", r#"{"path":"/d","recursive":true}"#, "mkdir /d 1"),
        ("mkdir", r#"{"path":"/d","recursive":false}"#, "mkdir /d 0"),
        ("mkdir", r#"{"path":"/d"}"#, "mkdir /d 0"),
        ("remove", r#"{"path":"/d","recursive":true}"#, "remove /d 1"),
        ("remove", r#"{"path":"/d"}"#, "remove /d 0"),
        ("rename", r#"{"from":"/a","to":"/b"}"#, "rename /a /b"),
        ("copy", r#"{"from":"/a","to":"/b"}"#, "copy /a /b"),
    ] {
        assert_eq!(
            file_tool(tool, arguments, asked_for, &done()),
            "done",
            "{tool} {arguments}"
        );
    }
}

#[test]
fn an_operation_that_failed_ends_the_call_with_a_text_that_says_so() {
    for (tool, arguments, asked_for) in [
        ("stat", r#"{"path":"/absent"}"#, "stat /absent"),
        ("list", r#"{"path":"/absent"}"#, "list /absent"),
        (
            "read",
            r#"{"path":"/absent"}"#,
            "read /absent 0 18446744073709551615",
        ),
        (
            "write",
            r#"{"path":"/absent","text":"x"}"#,
            "write /absent x",
        ),
        ("remove", r#"{"path":"/absent"}"#, "remove /absent 0"),
        ("copy", r#"{"from":"/absent","to":"/b"}"#, "copy /absent /b"),
    ] {
        assert_eq!(
            file_tool(tool, arguments, asked_for, &failed_answer()),
            "failed (1): nothing is there",
            "{tool}"
        );
    }
}

#[test]
fn an_answer_with_another_status_is_named_with_it() {
    let odd = RvFileAnswer {
        status: 77,
        ..done()
    };
    assert_eq!(
        file_tool("stat", &path_arguments("/x"), "stat /x", &odd),
        "the answer has status 77"
    );
}

#[test]
fn a_file_function_that_returns_another_status_ends_the_call_at_once_and_names_it() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    ASK_STATUS.set(RV_ERR_MISUSE);
    assert_eq!(step(&mut call, "stat", &path_arguments("/x")), RV_OK);
    assert_eq!(
        finished(),
        [(
            RV_RESULT_TRUSTED,
            "the file function returned status 4".to_owned()
        )]
    );
    assert!(
        call.tool_call.is_null(),
        "a call that ended leaves no state"
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_second_step_that_is_lent_no_answer_says_so() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    let arguments = path_arguments("/x");
    assert_eq!(step(&mut call, "stat", &arguments), RV_PENDING);
    assert_eq!(step(&mut call, "stat", &arguments), RV_OK);
    assert_eq!(
        finished(),
        [(RV_RESULT_TRUSTED, "the step was lent no answer".to_owned())]
    );
    assert_eq!(asked(), ["stat /x"], "the second step asks for nothing");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_file_tool_without_a_path_asks_for_the_empty_path() {
    assert_eq!(
        file_tool("stat", "{}", "stat ", &failed_answer()),
        "failed (1): nothing is there"
    );
}

#[test]
fn a_file_call_that_waits_for_its_answer_is_freed_by_a_cancel() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(
        step(&mut call, "write", r#"{"path":"/a","text":"x"}"#),
        RV_PENDING
    );
    assert!(!call.tool_call.is_null());
    assert_eq!(cancel(&mut call), RV_OK);
    assert_eq!(finished(), [], "an abandoned call finishes nothing");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_read_many_tool_reads_one_file_in_each_step_and_counts_what_its_answers_hold() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    let arguments = r#"{"count":3,"path":"/many"}"#;
    let whole = u64::MAX;

    assert_eq!(step(&mut call, "read_many", arguments), RV_PENDING);
    assert_eq!(asked(), [format!("read /many/0 0 {whole}")]);
    let two = read_answer("ab");
    assert_eq!(
        step_with(&mut call, "read_many", arguments, &two),
        RV_PENDING
    );
    let absent = failed_answer();
    assert_eq!(
        step_with(&mut call, "read_many", arguments, &absent),
        RV_PENDING
    );
    assert_eq!(finished(), [], "the call goes on until it read every file");
    let three = read_answer("cde");
    assert_eq!(step_with(&mut call, "read_many", arguments, &three), RV_OK);

    assert_eq!(
        asked(),
        [
            format!("read /many/0 0 {whole}"),
            format!("read /many/1 0 {whole}"),
            format!("read /many/2 0 {whole}"),
        ],
        "each step but the last asks for the next file"
    );
    assert_eq!(
        finished(),
        [(
            RV_RESULT_TRUSTED,
            "read 3 files, 5 bytes, 1 failed".to_owned()
        )]
    );
    assert!(call.tool_call.is_null(), "the last step frees the state");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_read_many_tool_with_no_count_ends_in_its_first_step() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "read_many", r#"{"path":"/many"}"#), RV_OK);
    assert_eq!(asked(), [] as [String; 0]);
    assert_eq!(
        finished(),
        [(
            RV_RESULT_TRUSTED,
            "read 0 files, 0 bytes, 0 failed".to_owned()
        )]
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_string_is_the_text_between_the_quotes_after_its_key() {
    assert_eq!(
        string_of(r#"{"path":"/notes/a.md"}"#, "path"),
        "/notes/a.md"
    );
    assert_eq!(string_of(r#"{"limit":3,"path":"a b"}"#, "path"), "a b");
    assert_eq!(string_of(r#"{"from":"/a","to":"/b"}"#, "from"), "/a");
    assert_eq!(string_of(r#"{"from":"/a","to":"/b"}"#, "to"), "/b");
    assert_eq!(string_of(r#"{"path":""}"#, "path"), "");
    assert_eq!(string_of("{}", "path"), "");
    assert_eq!(string_of(r#"{"path":"unclosed"#, "path"), "");
}

#[test]
fn a_number_is_the_digits_after_its_key_and_a_flag_is_set_only_by_true() {
    assert_eq!(
        number_of(r#"{"count":100000,"path":"/many"}"#, "count"),
        100_000
    );
    assert_eq!(number_of(r#"{"count":7}"#, "count"), 7);
    assert_eq!(number_of(r#"{"count":2.5}"#, "count"), 2);
    assert_eq!(number_of(r#"{"count":"7"}"#, "count"), 0);
    assert_eq!(number_of(r#"{"count":-1}"#, "count"), 0);
    assert_eq!(number_of("{}", "count"), 0);

    assert!(flag_of(r#"{"path":"/d","recursive":true}"#, "recursive"));
    assert!(!flag_of(r#"{"path":"/d","recursive":false}"#, "recursive"));
    assert!(!flag_of(r#"{"path":"/d"}"#, "recursive"));
}

#[test]
fn a_step_with_the_wrong_arguments_ends_the_call_and_frees_its_state() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    call.verb = RV_TOOL_CALL;
    call.args = ptr::null();
    assert_eq!(send(&mut call), RV_ERR_MISUSE, "a step needs its arguments");
    assert!(
        call.tool_call.is_null(),
        "a step that ends the call leaves no state, whatever its status"
    );

    assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    let short = step_args(
        size_of_u32::<RvToolCallArgs>() - 1,
        "never",
        "{}",
        ptr::null(),
        NO_RESOURCES,
    );
    call.args = (&raw const short).cast();
    assert_eq!(send(&mut call), RV_ERR_UNSUPPORTED, "short arguments");
    assert!(call.tool_call.is_null());
    assert_eq!(finished(), []);
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}
