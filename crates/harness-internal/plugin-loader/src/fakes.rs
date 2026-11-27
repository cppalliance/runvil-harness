//! Fake entry points for the unit tests: plain `extern "C" fn`s written
//! against `runvil-plugin-sys`, the helpers that call the Host's
//! functions the way a Plugin does, and the helpers that drive a tool
//! call's future the way an executor does, waiting for a wake after each
//! `Pending`. Each test defines its own fake with `fake!` or
//! `pack_fake!`, so its statics belong to it alone.

use std::ffi::c_void;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    BoxFuture, Error, Pack, PackRegistry, ToolContext, ToolFiles, ToolId, ToolOutput, VfsError,
    VfsOp, VfsOutcome,
};
pub(crate) use runvil_plugin_sys::{
    PluginEntry, RV_ERR_FILE, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_ERR_UNSUPPORTED,
    RV_FILE_ERROR_IS_A_DIRECTORY, RV_FILE_ERROR_NOT_A_DIRECTORY, RV_FILE_ERROR_NOT_FOUND,
    RV_FILE_ERROR_OTHER, RV_FILE_TYPE_DIRECTORY, RV_FILE_TYPE_FILE, RV_OK, RV_PACK_DELETE,
    RV_PACK_NEW, RV_PENDING, RV_PLUGIN_LOAD, RV_PLUGIN_UNLOAD, RV_RESULT_BACKEND,
    RV_RESULT_INVALID_ARGUMENTS, RV_RESULT_OTHER, RV_RESULT_TRANSPORT, RV_RESULT_TRUSTED,
    RV_RESULT_UNTRUSTED, RV_TOOL_CALL, RV_TOOL_CANCEL, RV_TOOL_CHANGES_FILES, RvBytes, RvCall,
    RvFileAnswer, RvHost, RvLoadArgs, RvPackInfo, RvPackNewArgs, RvPluginInfo, RvResultKind,
    RvStatus, RvStr, RvToolCallArgs, RvToolInfo, RvToolResult, RvVerb, RvWaker,
};
use serde_json::Value;

use crate::host::{self, HOST};
use crate::plugin::{Plugin, insert_plugin};
use crate::resources::HostResources;

/// A status no verb returns, for a fake that fails with its own code.
pub(crate) const ODD_STATUS: RvStatus = 77;

/// The name of the Pack a `pack_fake!` inserts.
pub(crate) const FAKE_PACK: &str = "acme/fake";

/// The parameters of a fake's tools: an object with no properties.
pub(crate) const NO_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;

/// Defines `mod $name` holding a fake entry point and its statics. Its
/// `RV_PLUGIN_LOAD` stores the `ctx` it was handed in `LOAD_CTX`, then runs
/// `$load`, which sees the call as `$call` and returns the load's status.
/// Its `RV_PLUGIN_UNLOAD` counts itself in `UNLOADS`, stores the instance
/// pointer it was sent in `UNLOADED`, records whether the call carried the
/// contract's fields in `UNLOAD_WELL_FORMED`, and stores the status of a
/// `describe_plugin` it then tries through `LOAD_CTX` in `UNLOAD_STATUS`.
/// The second form adds the items a test needs the load body to share with
/// it, such as a `pub(super) static`.
macro_rules! fake {
    ($name:ident, |$call:ident| $load:expr) => {
        $crate::fakes::fake!($name, {}, |$call| $load);
    };
    ($name:ident, { $($items:item)* }, |$call:ident| $load:expr) => {
        mod $name {
            use std::ffi::c_void;
            use std::ptr;
            use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

            use $crate::fakes::*;

            /// How many RV_PLUGIN_UNLOAD calls this fake got.
            pub(super) static UNLOADS: AtomicUsize = AtomicUsize::new(0);
            /// The instance pointer its last RV_PLUGIN_UNLOAD carried.
            pub(super) static UNLOADED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// Whether its last RV_PLUGIN_UNLOAD carried the contract's fields.
            pub(super) static UNLOAD_WELL_FORMED: AtomicBool = AtomicBool::new(false);
            /// The status a `describe_plugin` during RV_PLUGIN_UNLOAD got,
            /// plus one, so zero means it never tried.
            pub(super) static UNLOAD_STATUS: AtomicUsize = AtomicUsize::new(0);
            /// The `ctx` its last RV_PLUGIN_LOAD was handed, which a test
            /// reads to call a load function after that load returned.
            pub(super) static LOAD_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

            $($items)*

            pub(super) extern "C" fn entry($call: *mut RvCall) -> RvStatus {
                match verb($call) {
                    RV_PLUGIN_LOAD => {
                        LOAD_CTX.store(ctx($call), Ordering::SeqCst);
                        $load
                    }
                    RV_PLUGIN_UNLOAD => {
                        UNLOADS.fetch_add(1, Ordering::SeqCst);
                        UNLOADED.store(plugin($call), Ordering::SeqCst);
                        UNLOAD_WELL_FORMED
                            .store(contract_holds($call, RV_PLUGIN_UNLOAD), Ordering::SeqCst);
                        let status =
                            describe_with(LOAD_CTX.load(Ordering::SeqCst), "acme-url", "1.0.0");
                        UNLOAD_STATUS.store(status as usize + 1, Ordering::SeqCst);
                        RV_OK
                    }
                    _ => RV_ERR_UNSUPPORTED,
                }
            }
        }
    };
}
pub(crate) use fake;

/// Defines `mod $name` holding a fake entry point whose Plugin, named
/// `fake`, inserts the one Pack [`FAKE_PACK`] and sets the instance
/// [`PLUGIN_TAG`]. Its `RV_PACK_NEW` runs `$new` and its `RV_TOOL_CALL`
/// runs `$step`, each seeing the call under the name it gives and
/// returning the call's status. It counts its `RV_PLUGIN_UNLOAD` calls in
/// `UNLOADS`, its `RV_PACK_DELETE` calls in `DELETES`, and its
/// `RV_TOOL_CANCEL` calls in `CANCELS`, keeps the pointer the last delete
/// and the last cancel carried in `DELETED` and `CANCELED`, and keeps in
/// `ORDER` the verbs of those three kinds, in the order they came. The
/// items in the braces are what a test needs the bodies to share with it.
/// A `cancel:` body runs in its `RV_TOOL_CANCEL` and an `unload:` body in
/// its `RV_PLUGIN_UNLOAD`, each once the fake has counted the verb, as a
/// Plugin's destructors run there.
macro_rules! pack_fake {
    (
        $name:ident,
        { $($items:item)* },
        new: |$new_call:ident| $new:expr,
        step: |$step_call:ident| $step:expr
        $(, cancel: |$cancel_call:ident| $cancel:expr)?
        $(, unload: |$unload_call:ident| $unload:expr)?
    ) => {
        mod $name {
            use std::ffi::c_void;
            use std::ptr;
            use std::sync::Mutex;
            use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

            use $crate::fakes::*;

            /// How many RV_PLUGIN_UNLOAD calls this fake got.
            pub(super) static UNLOADS: AtomicUsize = AtomicUsize::new(0);
            /// How many RV_PACK_DELETE calls this fake got.
            pub(super) static DELETES: AtomicUsize = AtomicUsize::new(0);
            /// The `pack_run` its last RV_PACK_DELETE carried.
            pub(super) static DELETED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// How many RV_TOOL_CANCEL calls this fake got.
            pub(super) static CANCELS: AtomicUsize = AtomicUsize::new(0);
            /// The `tool_call` its last RV_TOOL_CANCEL carried.
            pub(super) static CANCELED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// The cancels, deletes, and unloads it got, in order.
            pub(super) static ORDER: Mutex<Vec<RvVerb>> = Mutex::new(Vec::new());

            $($items)*

            /// Notes that `verb` came.
            fn note(verb: RvVerb) {
                if let Ok(mut order) = ORDER.lock() {
                    order.push(verb);
                }
            }

            pub(super) extern "C" fn entry(call: *mut RvCall) -> RvStatus {
                match verb(call) {
                    RV_PLUGIN_LOAD => {
                        set_plugin(call, instance(PLUGIN_TAG));
                        describe(call, "fake", "1.0.0");
                        insert_pack(call, FAKE_PACK, "A fake Pack")
                    }
                    RV_PLUGIN_UNLOAD => {
                        UNLOADS.fetch_add(1, Ordering::SeqCst);
                        note(RV_PLUGIN_UNLOAD);
                        $(
                            let $unload_call = call;
                            $unload;
                        )?
                        RV_OK
                    }
                    RV_PACK_NEW => {
                        let $new_call = call;
                        $new
                    }
                    RV_PACK_DELETE => {
                        DELETES.fetch_add(1, Ordering::SeqCst);
                        DELETED.store(pack_run(call), Ordering::SeqCst);
                        note(RV_PACK_DELETE);
                        RV_OK
                    }
                    RV_TOOL_CALL => {
                        let $step_call = call;
                        $step
                    }
                    RV_TOOL_CANCEL => {
                        CANCELS.fetch_add(1, Ordering::SeqCst);
                        CANCELED.store(tool_call(call), Ordering::SeqCst);
                        note(RV_TOOL_CANCEL);
                        $(
                            let $cancel_call = call;
                            $cancel;
                        )?
                        RV_OK
                    }
                    _ => RV_ERR_UNSUPPORTED,
                }
            }
        }
    };
}
pub(crate) use pack_fake;

/// The instance a `pack_fake!` Plugin sets.
pub(crate) const PLUGIN_TAG: usize = 0x100;

/// The `pack_run` a well-behaved fake `RV_PACK_NEW` sets.
pub(crate) const PACK_TAG: usize = 0x200;

/// Inserts the linked Plugin `entry` into `packs`, as a Host that offers
/// `resources` does, and reads the report: the Plugin that loaded, or
/// the report's first failure, which drops the report and its hold on a
/// Plugin that loaded all the same.
pub(crate) fn insert_into(
    packs: &mut PackRegistry,
    entry: PluginEntry,
    resources: &HostResources,
) -> Result<Arc<Plugin>, Error> {
    let report = insert_plugin(packs, entry, resources);
    match report.failures().first() {
        Some(failure) => Err(failure.clone()),
        None => Ok(Arc::clone(
            report
                .plugins()
                .first()
                .expect("a report with no failure holds its Plugin"),
        )),
    }
}

/// Inserts the linked Plugin `entry` into a fresh Pack registry, as a
/// Host that offers no resources does.
pub(crate) fn insert(entry: PluginEntry) -> Result<Arc<Plugin>, Error> {
    insert_into(&mut PackRegistry::new(), entry, &HostResources::new())
}

/// Inserts `entry`, which must load.
pub(crate) fn loaded(entry: PluginEntry) -> Arc<Plugin> {
    insert(entry).expect("the Plugin loads")
}

/// Inserts `entry`, which must fail to load.
pub(crate) fn failure(entry: PluginEntry) -> Error {
    insert(entry).expect_err("the Plugin fails to load")
}

/// The `Reason` fact of a load failure.
pub(crate) fn reason(failure: &Error) -> Option<&str> {
    failure
        .facts
        .get(&runvil_plugin::ErrorFact::Reason)?
        .as_str()
}

/// Asks the factory of [`FAKE_PACK`] in `packs` for one run's instance.
fn fake_pack(packs: PackRegistry) -> Result<Arc<dyn Pack>, Error> {
    let (_info, id, factory) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn == FAKE_PACK)
        .expect("the Plugin inserted its Pack");
    factory.expect("the Pack is usable").new_pack(&id)
}

/// Inserts the `pack_fake!` Plugin `entry` into a fresh registry, drops
/// the registry's hold on the Plugin's `Arc`, and asks the factory of
/// [`FAKE_PACK`] for one run's instance.
pub(crate) fn new_pack(entry: PluginEntry) -> Result<Arc<dyn Pack>, Error> {
    let mut packs = PackRegistry::new();
    drop(insert_into(&mut packs, entry, &HostResources::new()).expect("the Plugin loads"));
    fake_pack(packs)
}

/// One run's instance of the Pack the `pack_fake!` Plugin `entry` inserts,
/// which must be made.
pub(crate) fn made(entry: PluginEntry) -> Arc<dyn Pack> {
    new_pack(entry).expect("the Pack instance is made")
}

/// The `pack_fake!` Plugin `entry`, inserted by a Host that offers
/// `resources`, beside one run's instance of its Pack. The test holds
/// the Plugin, so the Plugin stays loaded until the test drops it.
pub(crate) fn made_with(
    entry: PluginEntry,
    resources: &HostResources,
) -> (Arc<Plugin>, Arc<dyn Pack>) {
    let mut packs = PackRegistry::new();
    let plugin = insert_into(&mut packs, entry, resources).expect("the Plugin loads");
    let pack = fake_pack(packs).expect("the Pack instance is made");
    (plugin, pack)
}

/// A call of the fake Pack's tool `name`, for a test to lend its context.
pub(crate) fn test_call(name: &str) -> TestCall {
    TestCall::new(ToolId::parse(&format!("fake/{name}")).expect("a two-segment tool id parses"))
}

/// How long a test waits for a wake, or for a call into the loader to
/// return. What is coming comes far sooner in a test, so what has not
/// come by then is missing.
const WAIT_LIMIT: Duration = Duration::from_secs(60);

/// The waker [`block_on`] polls with: it lets the thread that waits in
/// [`Signal::wait`] go on.
#[derive(Default)]
struct Signal {
    /// Whether a wake has come since the last wait ended.
    woken: Mutex<bool>,
    /// Notified at each wake.
    wakes: Condvar,
}

impl Signal {
    /// Blocks until a wake has come since the last wait ended, for at
    /// most `limit`. Returns whether one came, and forgets it.
    fn wait(&self, limit: Duration) -> bool {
        let woken = self.woken.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut woken, _timed_out) = self
            .wakes
            .wait_timeout_while(woken, limit, |woken| !*woken)
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *woken)
    }
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        *self.woken.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.wakes.notify_all();
    }
}

/// Drives `future` to its output as an executor does: polls it, and after
/// each `Pending` waits until something wakes the future's task before it
/// polls again. A future that is pending and that nothing wakes within 60
/// seconds fails the test, with a message that says so, and does not hang
/// it.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    block_on_within(future, WAIT_LIMIT)
}

/// [`block_on`], with `limit` as the time a wake may take.
pub(crate) fn block_on_within<F: Future>(future: F, limit: Duration) -> F::Output {
    let mut future = pin!(future);
    let signal = Arc::new(Signal::default());
    let waker = Waker::from(Arc::clone(&signal));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        assert!(
            signal.wait(limit),
            "the future returned Pending and nothing woke its task within {limit:?}: a tool call \
             whose step returned RV_PENDING with no file request gets its next step only after \
             the Plugin wakes the call"
        );
    }
}

/// Counts how often it is woken, for a test that polls a future by hand
/// and checks each wake.
#[derive(Default)]
pub(crate) struct Wakes(AtomicUsize);

impl Wakes {
    /// A counter, beside the waker that counts in it.
    pub(crate) fn new() -> (Arc<Wakes>, Waker) {
        let counter = Arc::new(Wakes::default());
        let waker = Waker::from(Arc::clone(&counter));
        (counter, waker)
    }

    /// How often the waker was woken so far.
    pub(crate) fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Runs `work` on a thread of its own and returns what it returns. When
/// `work` has not returned within 60 seconds, the test fails with a
/// message that names `what`, and does not hang. A call into the loader
/// blocks for good when the loader holds a lock of its own across a call
/// into a Plugin that then wakes a call on that thread.
pub(crate) fn unblocked<T: Send + 'static>(
    what: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _sent = done.send(work());
    });
    match finished.recv_timeout(WAIT_LIMIT) {
        Ok(output) => {
            worker.join().expect("the worker thread finishes");
            output
        }
        Err(RecvTimeoutError::Timeout) => panic!(
            "{what} did not return within {WAIT_LIMIT:?}: a call into the loader blocks like \
             this when the loader holds a lock of its own across a call into a Plugin that wakes \
             a call on the same thread"
        ),
        // The worker dropped its sender without sending, so `work`
        // panicked, and the test fails with that panic.
        Err(RecvTimeoutError::Disconnected) => match worker.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => panic!("{what} ended without a result"),
        },
    }
}

/// Runs one call of the tool `name` on `pack` with `args` to its end.
pub(crate) fn call_tool(pack: &dyn Pack, name: &str, args: Value) -> Result<ToolOutput, Error> {
    let call = test_call(name);
    block_on(pack.call(call.context(), args))
}

/// `size_of::<T>()` as a contract struct's `u32` size field.
pub(crate) fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(0)
}

/// Whether `call` carries the fields the contract gives a call that sends
/// `verb` in version 0.7, and its table says the contract's version. Each
/// verb carries the Plugin's pointers the contract names for it and no
/// others, and its own arguments struct, with a `ctx`, or no arguments.
pub(crate) fn contract_holds(call: *mut RvCall, verb: RvVerb) -> bool {
    // SAFETY: a fake runs only inside an entry point call, where `call` is
    // the loader's live `RvCall` and its table is the loader's static.
    let (call, table) = unsafe { (&*call, &*(*call).host) };
    let has_plugin = verb != RV_PLUGIN_LOAD;
    let has_pack = matches!(verb, RV_PACK_DELETE | RV_TOOL_CALL | RV_TOOL_CANCEL);
    // The first field of each arguments struct is its size, then a zero
    // `reserved`, then the `ctx`, as `RvLoadArgs` lays them out.
    let args_size = match verb {
        RV_PLUGIN_LOAD => Some(size_u32::<RvLoadArgs>()),
        RV_PACK_NEW => Some(size_u32::<RvPackNewArgs>()),
        RV_TOOL_CALL => Some(size_u32::<RvToolCallArgs>()),
        _ => None,
    };
    let args_hold = match args_size {
        Some(size) if !call.args.is_null() => {
            // SAFETY: each arguments struct begins with the three fields
            // of an `RvLoadArgs` this reads, and is live for the call.
            let args = unsafe { &*call.args.cast::<RvLoadArgs>() };
            args.size == size && args.reserved == 0 && !args.ctx.is_null()
        }
        Some(_) => false,
        None => call.args.is_null(),
    };
    call.size == size_u32::<RvCall>()
        && call.verb == verb
        && ptr::eq(call.host, &raw const HOST)
        && call.plugin.is_null() != has_plugin
        && (has_pack || call.pack_run.is_null())
        && (matches!(verb, RV_TOOL_CALL | RV_TOOL_CANCEL) || call.tool_call.is_null())
        && args_hold
        && table.size == size_u32::<RvHost>()
        && table.api_major == 0
        && table.api_minor == 7
        && table.reserved == 0
}

/// The verb a call carries.
pub(crate) fn verb(call: *mut RvCall) -> RvVerb {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).verb }
}

/// The instance pointer a call carries.
pub(crate) fn plugin(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).plugin }
}

/// Sets the instance pointer, as RV_PLUGIN_LOAD may.
pub(crate) fn set_plugin(call: *mut RvCall, plugin: *mut c_void) {
    // SAFETY: as in `contract_holds`, and nothing else reads the call
    // meanwhile.
    unsafe { (*call).plugin = plugin };
}

/// The Pack instance pointer a call carries.
pub(crate) fn pack_run(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).pack_run }
}

/// Sets the Pack instance pointer, as RV_PACK_NEW may.
pub(crate) fn set_pack_run(call: *mut RvCall, pack_run: *mut c_void) {
    // SAFETY: as in `set_plugin`.
    unsafe { (*call).pack_run = pack_run };
}

/// The tool call state a call carries.
pub(crate) fn tool_call(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).tool_call }
}

/// Sets the tool call state, as a step may.
pub(crate) fn set_tool_call(call: *mut RvCall, state: *mut c_void) {
    // SAFETY: as in `set_plugin`.
    unsafe { (*call).tool_call = state };
}

/// An address that is never null, which a fake hands out as its instance.
pub(crate) fn instance(tag: usize) -> *mut c_void {
    ptr::without_provenance_mut(tag)
}

/// The `RvLoadArgs` a load call carries.
fn args<'call>(call: *mut RvCall) -> &'call RvLoadArgs {
    // SAFETY: as in `contract_holds`, and a load call's `args` is the
    // loader's `RvLoadArgs`, live for the whole call, which is as long as
    // a fake holds this reference.
    unsafe { &*(*call).args.cast::<RvLoadArgs>() }
}

/// The `RvPackNewArgs` an RV_PACK_NEW call carries.
pub(crate) fn pack_args<'call>(call: *mut RvCall) -> &'call RvPackNewArgs {
    // SAFETY: as in `args`, for the verb's own arguments struct.
    unsafe { &*(*call).args.cast::<RvPackNewArgs>() }
}

/// The `RvToolCallArgs` an RV_TOOL_CALL step carries.
pub(crate) fn step_args<'call>(call: *mut RvCall) -> &'call RvToolCallArgs {
    // SAFETY: as in `args`, for the verb's own arguments struct.
    unsafe { &*(*call).args.cast::<RvToolCallArgs>() }
}

/// The `ctx` a load call's `RvLoadArgs` carries.
pub(crate) fn ctx(call: *mut RvCall) -> *mut c_void {
    args(call).ctx
}

/// Wakes through `waker`, as a Plugin does, on any thread.
pub(crate) fn wake_through(waker: RvWaker) {
    // SAFETY: a fake wakes only through a waker the loader lent one of
    // its steps, and only while its Plugin is loaded, so the loader
    // still keeps what the waker's `ctx` points at.
    unsafe { (waker.wake)(waker.ctx) };
}

/// Wakes the call of the step `call`, through the waker the step is lent.
pub(crate) fn wake_call(call: *mut RvCall) {
    wake_through(step_args(call).waker);
}

/// The waker a fake kept past the step that lent it, as a Plugin keeps
/// one for a thread of its own or for a destructor. A fake's `static` of
/// it starts empty.
pub(crate) struct Kept(Mutex<Option<Lent>>);

/// A waker a step was lent.
struct Lent(RvWaker);

// SAFETY: the contract lets a Plugin call `wake` with its waker's `ctx`
// on any thread until RV_PLUGIN_UNLOAD returns, and nothing here reads
// through `ctx`, so the pair may move to any thread.
unsafe impl Send for Lent {}

impl Kept {
    /// No waker yet.
    pub(crate) const fn new() -> Kept {
        Kept(Mutex::new(None))
    }

    /// The kept waker, if there is one.
    fn kept(&self) -> Option<RvWaker> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|lent| lent.0)
    }

    /// Keeps the waker the step `call` is lent.
    pub(crate) fn keep(&self, call: *mut RvCall) {
        let lent = Lent(step_args(call).waker);
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(lent);
    }

    /// The `ctx` of the kept waker, or NULL when none is kept.
    pub(crate) fn ctx(&self) -> *mut c_void {
        self.kept().map_or(ptr::null_mut(), |waker| waker.ctx)
    }

    /// Wakes through the kept waker, and returns whether one was kept.
    /// The fake's own lock is let go before the wake.
    pub(crate) fn wake(&self) -> bool {
        let kept = self.kept();
        kept.map(wake_through).is_some()
    }
}

/// A string for the C side, borrowed from `text`.
pub(crate) fn rv_str(text: &str) -> RvStr {
    RvStr {
        ptr: text.as_ptr().cast(),
        len: text.len(),
    }
}

/// A string for the C side, borrowed from `bytes`.
pub(crate) fn rv_str_bytes(bytes: &[u8]) -> RvStr {
    RvStr {
        ptr: bytes.as_ptr().cast(),
        len: bytes.len(),
    }
}

/// The text a string from the loader holds.
pub(crate) fn text(s: RvStr) -> String {
    if s.len == 0 {
        return String::new();
    }
    // SAFETY: the loader hands a fake strings that are valid for their
    // length during the call.
    let bytes = unsafe { slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// A well-formed `RvPluginInfo`.
pub(crate) fn info(name: RvStr, version: RvStr) -> RvPluginInfo {
    RvPluginInfo {
        size: size_u32::<RvPluginInfo>(),
        reserved: 0,
        name,
        version,
    }
}

/// Calls the call's `describe_plugin` with its own `ctx` and `info`.
pub(crate) fn describe_info(call: *mut RvCall, info: &RvPluginInfo) -> RvStatus {
    describe_as(call, ctx(call), info)
}

/// Calls the `describe_plugin` of `args_from`'s `RvLoadArgs`, handing it
/// `as_ctx`.
pub(crate) fn describe_as(
    args_from: *mut RvCall,
    as_ctx: *mut c_void,
    info: &RvPluginInfo,
) -> RvStatus {
    // SAFETY: `info` is a live `RvPluginInfo` whose strings outlive the
    // call, and the function reads through `as_ctx` only when it is the
    // `ctx` of the load open on this thread.
    unsafe { (args(args_from).describe_plugin)(as_ctx, info) }
}

/// Describes the Plugin with `name` and `version`.
pub(crate) fn describe(call: *mut RvCall, name: &str, version: &str) -> RvStatus {
    describe_info(call, &info(rv_str(name), rv_str(version)))
}

/// Calls the `refuse` of `args_from`'s `RvLoadArgs`, handing it `as_ctx`.
pub(crate) fn refuse_as(args_from: *mut RvCall, as_ctx: *mut c_void, reason: RvStr) -> RvStatus {
    // SAFETY: `reason` outlives the call, and the function reads through
    // `as_ctx` only when it is the `ctx` of the load open on this thread.
    unsafe { (args(args_from).refuse)(as_ctx, reason) }
}

/// Calls the call's `refuse` with its own `ctx` and `reason`.
pub(crate) fn refuse_str(call: *mut RvCall, reason: RvStr) -> RvStatus {
    refuse_as(call, ctx(call), reason)
}

/// Refuses the load with `reason`.
pub(crate) fn refuse(call: *mut RvCall, reason: &str) -> RvStatus {
    refuse_str(call, rv_str(reason))
}

/// Calls the loader's `describe_plugin` with `ctx`, as a Plugin that kept
/// the load functions and a `ctx` past its load would, from any thread.
pub(crate) fn describe_with(ctx: *mut c_void, name: &str, version: &str) -> RvStatus {
    let info = info(rv_str(name), rv_str(version));
    // SAFETY: `info` is live, and the function reads through `ctx` only
    // when it is the `ctx` of the load open on this thread.
    unsafe { host::describe_plugin(ctx, &raw const info) }
}

/// Calls the loader's `refuse` with `ctx`, as `describe_with` does.
pub(crate) fn refuse_with(ctx: *mut c_void, reason: &str) -> RvStatus {
    // SAFETY: as in `describe_with`.
    unsafe { host::refuse(ctx, rv_str(reason)) }
}

/// A well-formed `RvPackInfo`.
pub(crate) fn pack_info(name: RvStr, description: RvStr) -> RvPackInfo {
    RvPackInfo {
        size: size_u32::<RvPackInfo>(),
        reserved: 0,
        name,
        description,
    }
}

/// Calls the load call's `insert_pack` with its own `ctx` and `info`.
pub(crate) fn insert_pack_info(call: *mut RvCall, info: &RvPackInfo) -> RvStatus {
    // SAFETY: as in `describe_as`.
    unsafe { (args(call).insert_pack)(ctx(call), info) }
}

/// Inserts the Pack `name` with `description`.
pub(crate) fn insert_pack(call: *mut RvCall, name: &str, description: &str) -> RvStatus {
    insert_pack_info(call, &pack_info(rv_str(name), rv_str(description)))
}

/// A well-formed `RvToolInfo` for the tool `name`, with no flag, no wire
/// name, a one-line description, and [`NO_PARAMETERS`].
pub(crate) fn tool_info(name: &str) -> RvToolInfo {
    RvToolInfo {
        size: size_u32::<RvToolInfo>(),
        flags: 0,
        name: rv_str(name),
        wire_name: rv_str(""),
        description: rv_str("A fake tool."),
        parameters: rv_str(NO_PARAMETERS),
    }
}

/// Calls the `add_tool` of an RV_PACK_NEW call, handing it `as_ctx`.
pub(crate) fn add_tool_as(call: *mut RvCall, as_ctx: *mut c_void, info: &RvToolInfo) -> RvStatus {
    // SAFETY: `info` is a live `RvToolInfo` whose strings outlive the
    // call, and the function reads through `as_ctx` only when it is the
    // `ctx` of the call open on this thread.
    unsafe { (pack_args(call).add_tool)(as_ctx, info) }
}

/// Calls the call's `add_tool` with its own `ctx` and `info`.
pub(crate) fn add_tool_info(call: *mut RvCall, info: &RvToolInfo) -> RvStatus {
    add_tool_as(call, pack_args(call).ctx, info)
}

/// Adds the tool `name` to the instance being made.
pub(crate) fn add_tool(call: *mut RvCall, name: &str) -> RvStatus {
    add_tool_info(call, &tool_info(name))
}

/// Declines the RV_PACK_NEW call with `reason`.
pub(crate) fn refuse_pack(call: *mut RvCall, reason: &str) -> RvStatus {
    let args = pack_args(call);
    // SAFETY: `reason` outlives the call, and `ctx` is the open call's.
    unsafe { (args.refuse)(args.ctx, rv_str(reason)) }
}

/// A well-formed `RvToolResult`.
pub(crate) fn result(kind: RvResultKind, text: RvStr) -> RvToolResult {
    RvToolResult {
        size: size_u32::<RvToolResult>(),
        kind,
        text,
    }
}

/// Calls the `finish` of an RV_TOOL_CALL step, handing it `as_ctx`.
pub(crate) fn finish_as(
    call: *mut RvCall,
    as_ctx: *mut c_void,
    result: *const RvToolResult,
) -> RvStatus {
    // SAFETY: `result` is NULL, misaligned on purpose, or a live
    // `RvToolResult` whose text outlives the call, and the function reads
    // through `as_ctx` only when it is the `ctx` of the step open on this
    // thread.
    unsafe { (step_args(call).finish)(as_ctx, result) }
}

/// Calls the step's `finish` with its own `ctx` and `result`.
pub(crate) fn finish_result(call: *mut RvCall, result: &RvToolResult) -> RvStatus {
    finish_as(call, step_args(call).ctx, result)
}

/// Finishes the call with `kind` and `text`.
pub(crate) fn finish(call: *mut RvCall, kind: RvResultKind, text: &str) -> RvStatus {
    finish_result(call, &result(kind, rv_str(text)))
}

/// A run of bytes for the C side, borrowed from `bytes`.
pub(crate) fn rv_bytes(bytes: &[u8]) -> RvBytes {
    RvBytes {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

/// Calls the step's `file_stat` with `as_ctx`, as a Plugin would or as it
/// must not.
pub(crate) fn ask_stat_as(call: *mut RvCall, as_ctx: *mut c_void, path: RvStr) -> RvStatus {
    // SAFETY: the function refuses a `ctx` that is not the open step's
    // and a path it cannot read before it reads through either.
    unsafe { (step_args(call).file_stat)(as_ctx, path) }
}

/// Asks through the step's `file_stat` what `path` names.
pub(crate) fn ask_stat(call: *mut RvCall, path: &str) -> RvStatus {
    ask_stat_as(call, step_args(call).ctx, rv_str(path))
}

/// Asks through the step's `file_list` for the entries of `path`.
pub(crate) fn ask_list(call: *mut RvCall, path: &str) -> RvStatus {
    let args = step_args(call);
    // SAFETY: the open step's own `ctx`, and a path that outlives the
    // call.
    unsafe { (args.file_list)(args.ctx, rv_str(path)) }
}

/// Asks through the step's `file_read` for the bytes of `path` in the
/// range `offset` and `len`.
pub(crate) fn ask_read(call: *mut RvCall, path: &str, range: (u64, u64)) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_list`.
    unsafe { (args.file_read)(args.ctx, rv_str(path), range.0, range.1) }
}

/// Asks through the step's `file_write` to write `bytes` to `path`.
pub(crate) fn ask_write(call: *mut RvCall, path: &str, bytes: RvBytes) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_list`, and the function refuses bytes it cannot
    // read before it reads through them.
    unsafe { (args.file_write)(args.ctx, rv_str(path), bytes) }
}

/// Asks through the step's `file_append` to add `bytes` to `path`.
pub(crate) fn ask_append(call: *mut RvCall, path: &str, bytes: RvBytes) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_write`.
    unsafe { (args.file_append)(args.ctx, rv_str(path), bytes) }
}

/// Asks through the step's `file_mkdir` to make the directory `path`.
pub(crate) fn ask_mkdir(call: *mut RvCall, path: &str, recursive: u32) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_list`.
    unsafe { (args.file_mkdir)(args.ctx, rv_str(path), recursive) }
}

/// Asks through the step's `file_remove` to remove `path`.
pub(crate) fn ask_remove(call: *mut RvCall, path: &str, recursive: u32) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_list`.
    unsafe { (args.file_remove)(args.ctx, rv_str(path), recursive) }
}

/// Asks through the step's `file_rename` to move `from` to `to`.
pub(crate) fn ask_rename(call: *mut RvCall, from: RvStr, to: RvStr) -> RvStatus {
    let args = step_args(call);
    // SAFETY: the open step's own `ctx`, and the function refuses a path
    // it cannot read before it reads through it.
    unsafe { (args.file_rename)(args.ctx, from, to) }
}

/// Asks through the step's `file_copy` to copy `from` to `to`.
pub(crate) fn ask_copy(call: *mut RvCall, from: RvStr, to: RvStr) -> RvStatus {
    let args = step_args(call);
    // SAFETY: as in `ask_rename`.
    unsafe { (args.file_copy)(args.ctx, from, to) }
}

/// What the answer a step is lent holds for the file function that
/// asked: what the function asked for, or the kind and the text of the
/// failure. `None` for a step that is lent no answer.
pub(crate) type Answered<T> = Option<Result<T, (u32, String)>>;

/// The answer the step is lent, or `None` when the step follows no
/// request.
pub(crate) fn lent<'call>(call: *mut RvCall) -> Option<&'call RvFileAnswer> {
    // SAFETY: the loader keeps the answer of a step valid until the step
    // returns, which is as long as a fake holds this reference.
    unsafe { step_args(call).answer.as_ref() }
}

/// What `held` reads out of the answer the step is lent, or the answer's
/// failure.
fn answered<T>(call: *mut RvCall, held: impl FnOnce(&RvFileAnswer) -> T) -> Answered<T> {
    let answer = lent(call)?;
    Some(match answer.status {
        RV_OK => Ok(held(answer)),
        _ => Err((answer.error.kind, text(answer.error.text))),
    })
}

/// The answer to a `file_stat`: the file type and the length.
pub(crate) fn stat_answer(call: *mut RvCall) -> Answered<(u32, u64)> {
    answered(call, |answer| (answer.stat.file_type, answer.stat.len))
}

/// The answer to a `file_list`: each entry's name, file type, and length.
pub(crate) fn list_answer(call: *mut RvCall) -> Answered<Vec<(String, u32, u64)>> {
    answered(call, |answer| {
        if answer.entries.len == 0 {
            return Vec::new();
        }
        // SAFETY: the loader keeps the entries and their names valid
        // until the step returns.
        let listed = unsafe { slice::from_raw_parts(answer.entries.ptr, answer.entries.len) };
        listed
            .iter()
            .map(|entry| (text(entry.name), entry.stat.file_type, entry.stat.len))
            .collect()
    })
}

/// The answer to a `file_read`: the bytes.
pub(crate) fn read_answer(call: *mut RvCall) -> Answered<Vec<u8>> {
    answered(call, |answer| {
        if answer.bytes.len == 0 {
            return Vec::new();
        }
        // SAFETY: the loader keeps the bytes valid until the step
        // returns.
        unsafe { slice::from_raw_parts(answer.bytes.ptr, answer.bytes.len) }.to_vec()
    })
}

/// The answer to a file function that changes the files: nothing but
/// whether the change was made.
pub(crate) fn change_answer(call: *mut RvCall) -> Answered<()> {
    answered(call, |_answer| ())
}

/// The operation that writes `text` to `path`.
pub(crate) fn write_op(path: &str, text: &str) -> VfsOp {
    VfsOp::Write {
        path: path.to_owned(),
        contents: text.as_bytes().to_vec(),
    }
}

/// A call of the fake Pack's tool `name` over an in-memory filesystem
/// that holds each of `files`, a path beside its text.
pub(crate) fn seeded_call(name: &str, files: &[(&str, &str)]) -> TestCall {
    let call = test_call(name);
    for (path, text) in files {
        block_on(call.context().files().request(write_op(path, text)))
            .expect("a seeded file is written");
    }
    call
}

/// The context `call` lends, with `files` in place of the call's own
/// files.
pub(crate) fn context_over<'a>(call: &'a TestCall, files: &'a dyn ToolFiles) -> ToolContext<'a> {
    let lent = call.context();
    ToolContext::new(lent.tool(), lent.origin(), files)
}

/// The whole text of the file at `path` in the filesystem of `call`.
pub(crate) fn text_at(call: &TestCall, path: &str) -> Result<String, VfsError> {
    let read = VfsOp::Read {
        path: path.to_owned(),
        start: None,
        end: None,
    };
    match block_on(call.context().files().request(read))? {
        VfsOutcome::Text(text) => Ok(text),
        other => panic!("a read answered {other:?}"),
    }
}

/// Files that take each request and hold it until a test answers it.
/// They take a request at the first poll of its future, which is when a
/// request joins its call's queue at the Harness.
#[derive(Default)]
pub(crate) struct HeldFiles {
    held: Mutex<Held>,
}

/// The requests a [`HeldFiles`] took.
#[derive(Default)]
struct Held {
    /// The operations asked for, in the order the requests were made.
    asked: Vec<VfsOp>,
    /// The outcome a test gave each request, until its future takes it.
    outcomes: Vec<Option<Result<VfsOutcome, VfsError>>>,
    /// The waker of the last poll of each request's future.
    wakers: Vec<Option<Waker>>,
}

impl HeldFiles {
    /// The requests, locked.
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The operations asked for so far, in the order the requests were
    /// made.
    pub(crate) fn asked(&self) -> Vec<VfsOp> {
        self.held().asked.clone()
    }

    /// Answers the request numbered `index`, counted from 0 in the order
    /// the requests were made, with `outcome`, and wakes its future.
    pub(crate) fn answer(&self, index: usize, outcome: Result<VfsOutcome, VfsError>) {
        let waker = {
            let mut held = self.held();
            held.outcomes[index] = Some(outcome);
            held.wakers[index].take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl ToolFiles for HeldFiles {
    fn request(&self, op: VfsOp) -> BoxFuture<'_, Result<VfsOutcome, VfsError>> {
        let mut unsent = Some(op);
        let mut index = 0;
        Box::pin(poll_fn(move |cx| {
            let mut held = self.held();
            if let Some(op) = unsent.take() {
                index = held.asked.len();
                held.asked.push(op);
                held.outcomes.push(None);
                held.wakers.push(None);
            }
            if let Some(outcome) = held.outcomes[index].take() {
                return Poll::Ready(outcome);
            }
            held.wakers[index] = Some(cx.waker().clone());
            Poll::Pending
        }))
    }
}
