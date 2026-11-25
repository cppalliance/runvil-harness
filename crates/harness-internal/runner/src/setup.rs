//! What one run gets on top of its Host: [`RunSetup`], and the forms in
//! which a setup takes a service, [`IntoShared`].

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use runvil_engine::RunLimits;
use runvil_engine::effect::Round;
use runvil_engine::vfs::VfsRef;

use crate::performers::{Delta, InferenceBroker, Timer};
use crate::recorder::RunRecorder;

/// A value that becomes a service that a run shares: the service itself,
/// an `Arc` of it, or an `Arc<dyn ..>` of its trait.
///
/// `D` is the trait of the service as a trait object: `dyn RunRecorder`,
/// `dyn InferenceBroker`, or `dyn Timer`. [`RunSetup::new`] takes each
/// service through this trait, so a Host passes a service in the form in
/// which it holds it:
///
/// - The service itself, such as `MemoryRecorder::new()`. It goes into a
///   new `Arc`.
/// - An `Arc` of the service, such as a clone of the handle that a Host
///   keeps to read its recorder after the run. That `Arc` is kept as it
///   is, so the Host and the run hold the same object.
/// - An `Arc<dyn ..>`, such as the broker that a function of the Host
///   picks at run time. That `Arc` is kept as it is.
///
/// A function of a Host takes a service in the same forms through a
/// parameter such as `impl IntoShared<dyn RunRecorder>`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be handed over as a shared `{D}`",
    label = "this is not a service of that kind",
    note = "pass a value that implements the trait of the service, an `Arc` of one, or an `Arc<dyn ..>` of that trait"
)]
pub trait IntoShared<D: ?Sized> {
    /// Returns the service inside the `Arc` through which a run shares
    /// it.
    fn into_shared(self) -> Arc<D>;
}

impl<T: RunRecorder + 'static> IntoShared<dyn RunRecorder> for T {
    fn into_shared(self) -> Arc<dyn RunRecorder> {
        Arc::new(self)
    }
}

impl<T: RunRecorder + 'static> IntoShared<dyn RunRecorder> for Arc<T> {
    fn into_shared(self) -> Arc<dyn RunRecorder> {
        self
    }
}

impl IntoShared<dyn RunRecorder> for Arc<dyn RunRecorder> {
    fn into_shared(self) -> Arc<dyn RunRecorder> {
        self
    }
}

impl<T: InferenceBroker + 'static> IntoShared<dyn InferenceBroker> for T {
    fn into_shared(self) -> Arc<dyn InferenceBroker> {
        Arc::new(self)
    }
}

impl<T: InferenceBroker + 'static> IntoShared<dyn InferenceBroker> for Arc<T> {
    fn into_shared(self) -> Arc<dyn InferenceBroker> {
        self
    }
}

impl IntoShared<dyn InferenceBroker> for Arc<dyn InferenceBroker> {
    fn into_shared(self) -> Arc<dyn InferenceBroker> {
        self
    }
}

impl<T: Timer + 'static> IntoShared<dyn Timer> for T {
    fn into_shared(self) -> Arc<dyn Timer> {
        Arc::new(self)
    }
}

impl<T: Timer + 'static> IntoShared<dyn Timer> for Arc<T> {
    fn into_shared(self) -> Arc<dyn Timer> {
        self
    }
}

impl IntoShared<dyn Timer> for Arc<dyn Timer> {
    fn into_shared(self) -> Arc<dyn Timer> {
        self
    }
}

/// The tool calls a run has in flight at once unless its setup says
/// otherwise.
const DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT: NonZeroUsize = NonZeroUsize::MIN.saturating_add(9);

/// A Host's callback for the live pieces of a section's own model rounds.
///
/// The Harness calls it with each piece and the round the piece belongs
/// to, in the order the broker produces the pieces, one call at a time,
/// from inside the future that drives the run. It passes every piece on,
/// and it neither buffers nor skips one. These rules bind the callback:
///
/// - **It returns at once.** It never waits, sleeps, or blocks.
/// - **It never calls into the Harness.** The Harness is not reentrant.
///   The callback must not call [`RunControl::send`](crate::RunControl::send),
///   [`RunControl::stop_round`](crate::RunControl::stop_round), or
///   [`RunControl::cancel`](crate::RunControl::cancel), and it must not
///   start, poll, or drop a run. The Harness promises nothing about the
///   result of such a call.
/// - **A Host that reacts to a piece does so from its own code.** The
///   callback hands the piece to a channel, a flag, or a broadcast and
///   returns. The Host's own task then stops the round or cancels the
///   run, from outside the callback.
///
/// A Host that lags decides for itself what to do with the pieces it
/// gets: keep them, stop taking them, or ignore them. The finished reply,
/// which the recorder receives, is whole either way. A callback that
/// panics ends its round as a panic in a broker does: the Harness answers
/// the round `Dropped`.
pub type HostSink = Arc<dyn Fn(Round, Delta) + Send + Sync>;

/// Everything one run gets on top of its Host. The launcher builds one
/// per run, and the Harness reads each field once and keeps it for the
/// run.
pub struct RunSetup {
    /// Where the run's history is written.
    pub recorder: Arc<dyn RunRecorder>,
    /// Lists the models and performs model rounds.
    pub broker: Arc<dyn InferenceBroker>,
    /// Stamps the run's start and performs the run's `Timer` effects.
    pub timer: Arc<dyn Timer>,
    /// The filesystem the run's tool calls work in. No store of the run is
    /// part of it: the Engine makes each run an empty store of its own,
    /// and one more for each prompt the run executes, which no setup fills
    /// and no report hands back.
    pub vfs: VfsRef,
    /// The model id the user selected, if any.
    pub selected_model: Option<String>,
    /// The run's arguments: the text of one JSON object, which the run's
    /// first step checks against the prompt's `args` declaration.
    pub args: String,
    /// The run's resource limits.
    pub limits: RunLimits,
    /// The most tool calls in flight at once; the
    /// rest wait and start oldest first.
    pub max_tool_calls_in_flight: NonZeroUsize,
    /// A user answers the main chain's inbox, so the
    /// run holds `runvil/user`, which is unavailable
    /// otherwise.
    pub has_user: bool,
    /// The Host's callback for the live pieces of a section's own model
    /// rounds, when streaming is on for the run. It returns at once and
    /// never calls into the Harness: not `RunControl::send`, `stop_round`,
    /// or `cancel`, and not a call that starts, polls, or drops a run. A
    /// Host that reacts to a piece does so from its own code, outside the
    /// callback.
    pub streaming: Option<HostSink>,
}

impl RunSetup {
    /// A setup over `recorder`, `broker`, and `timer`, the three services
    /// a run waits on. Every other field starts
    /// empty or at its default: an empty memory filesystem, no selection,
    /// the empty object `{}` as its args, `RunLimits::default()`, at most
    /// 10 tool calls in flight, no user, and no streaming. Each of those
    /// fields has a method of its name that sets it and returns the
    /// setup.
    ///
    /// Each service is taken in the form in which the Host holds it, as
    /// [`IntoShared`] lists them: the service itself, which goes into a
    /// new `Arc`, or an `Arc` of it, which is kept as it is. So a Host
    /// that reads its recorder after the run keeps a handle and passes a
    /// clone of it, and the Host and the run hold the same recorder.
    #[must_use]
    pub fn new(
        recorder: impl IntoShared<dyn RunRecorder>,
        broker: impl IntoShared<dyn InferenceBroker>,
        timer: impl IntoShared<dyn Timer>,
    ) -> RunSetup {
        RunSetup {
            recorder: recorder.into_shared(),
            broker: broker.into_shared(),
            timer: timer.into_shared(),
            vfs: VfsRef::default(),
            selected_model: None,
            args: "{}".to_owned(),
            limits: RunLimits::default(),
            max_tool_calls_in_flight: DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT,
            has_user: false,
            streaming: None,
        }
    }

    /// Sets the filesystem the run's tool calls work in.
    #[must_use]
    pub fn vfs(mut self, vfs: VfsRef) -> RunSetup {
        self.vfs = vfs;
        self
    }

    /// Sets the model id the user selected.
    #[must_use]
    pub fn selected_model(mut self, model: impl Into<String>) -> RunSetup {
        self.selected_model = Some(model.into());
        self
    }

    /// Sets the run's arguments: the text of one JSON object.
    #[must_use]
    pub fn args(mut self, args: impl Into<String>) -> RunSetup {
        self.args = args.into();
        self
    }

    /// Sets the run's resource limits.
    #[must_use]
    pub fn limits(mut self, limits: RunLimits) -> RunSetup {
        self.limits = limits;
        self
    }

    /// Sets the most tool calls in flight at once.
    #[must_use]
    pub fn max_tool_calls_in_flight(mut self, max: NonZeroUsize) -> RunSetup {
        self.max_tool_calls_in_flight = max;
        self
    }

    /// Sets whether a user answers the main chain's inbox (`true`), so the
    /// run holds `runvil/user`, or the run has no user (`false`).
    #[must_use]
    pub fn has_user(mut self, has_user: bool) -> RunSetup {
        self.has_user = has_user;
        self
    }

    /// Turns streaming on for the run: `sink` receives each live piece of
    /// a section's own model rounds, with its round.
    ///
    /// The rounds of a section's own `models.loop` stream. A round that a
    /// nested `models.infer` runs hands the Host its finished reply alone.
    /// The Harness calls `sink` from inside the future that drives the
    /// run, in the middle of a round, so three rules bind it:
    ///
    /// - **It returns at once.** It never waits, sleeps, or blocks.
    /// - **It never calls into the Harness.** The Harness is not
    ///   reentrant. `sink` must not call
    ///   [`RunControl::send`](crate::RunControl::send),
    ///   [`RunControl::stop_round`](crate::RunControl::stop_round), or
    ///   [`RunControl::cancel`](crate::RunControl::cancel), and it must
    ///   not start, poll, or drop a run. The Harness promises nothing
    ///   about the result of such a call.
    /// - **A Host that reacts to a piece does so from its own code.**
    ///   `sink` hands the piece to a channel, a flag, or a broadcast and
    ///   returns. The Host's own task then stops the round or cancels the
    ///   run, from outside `sink`.
    ///
    /// A Host that writes to a terminal does so inside `sink`, since the
    /// write returns at once:
    ///
    /// ```text
    /// let setup = RunSetup::new(recorder, broker, timer).streaming(|_round, piece| {
    ///     if let Delta::Text(text) = piece {
    ///         let _ = write!(std::io::stdout(), "{text}");
    ///     }
    /// });
    /// ```
    ///
    /// A Host that buffers owns its queue. This `sink` hands each piece to
    /// a bounded channel with `try_send`, and stops pushing for a round
    /// once the channel is full, so the Host keeps a prefix of that
    /// round's reply. The Host's reading task, which runs outside the
    /// run's future, is the code that calls `RunControl::cancel` when the
    /// Host decides to end the run:
    ///
    /// ```text
    /// let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    /// let gave_up = AtomicU64::new(u64::MAX); // the round the Host stopped pushing for
    /// let setup = RunSetup::new(recorder, broker, timer).streaming(move |round, piece| {
    ///     let id = round.id.get();
    ///     if gave_up.load(Ordering::Relaxed) == id {
    ///         return;
    ///     }
    ///     if tx.try_send((round, piece)).is_err() {
    ///         gave_up.store(id, Ordering::Relaxed); // the channel is full or closed
    ///     }
    /// });
    /// ```
    #[must_use]
    pub fn streaming(mut self, sink: impl Fn(Round, Delta) + Send + Sync + 'static) -> RunSetup {
        self.streaming = Some(Arc::new(sink));
        self
    }
}

impl fmt::Debug for RunSetup {
    /// Shows every field but the recorder, the broker, and the timer. The
    /// streaming callback shows as whether it is set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunSetup")
            .field("vfs", &self.vfs)
            .field("selected_model", &self.selected_model)
            .field("args", &self.args)
            .field("limits", &self.limits)
            .field("max_tool_calls_in_flight", &self.max_tool_calls_in_flight)
            .field("has_user", &self.has_user)
            .field("streaming", &self.streaming.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the forms in which a setup takes a service: the service
    //! itself, an `Arc` of it, and an `Arc<dyn ..>` each give the setup
    //! the service that was handed over, and an `Arc` that is handed
    //! over stays the one `Arc` around its service. A setup starts
    //! without streaming, and the setter turns streaming on.

    use std::time::Duration;

    use runvil_engine::{BoxFuture, Timestamp};

    use super::*;
    use crate::performers::OfflineBroker;
    use crate::recorder::MemoryRecorder;

    /// The instant every clock here reads, far from the time of any test
    /// run.
    const INSTANT: Timestamp = Timestamp::from_unix_millis(1_700_000_000_000);

    /// A clock pinned at one instant. No test here reaches its sleep.
    struct Pinned(Timestamp);

    impl Timer for Pinned {
        fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
            unreachable!("no test here issues a Timer effect")
        }

        fn now(&self) -> Timestamp {
            self.0
        }
    }

    #[test]
    fn a_setup_takes_a_service_as_itself_inside_an_arc_and_as_an_erased_arc() {
        let itself = RunSetup::new(MemoryRecorder::new(), OfflineBroker, Pinned(INSTANT));
        let inside_an_arc = RunSetup::new(
            Arc::new(MemoryRecorder::new()),
            Arc::new(OfflineBroker),
            Arc::new(Pinned(INSTANT)),
        );
        let recorder: Arc<dyn RunRecorder> = Arc::new(MemoryRecorder::new());
        let broker: Arc<dyn InferenceBroker> = Arc::new(OfflineBroker);
        let timer: Arc<dyn Timer> = Arc::new(Pinned(INSTANT));
        let erased = RunSetup::new(recorder, broker, timer);

        // The trait's default `now` reads the system clock, so a setup
        // that reads the pinned instant holds the clock it was handed.
        for (form, setup) in [
            ("itself", itself),
            ("inside an `Arc`", inside_an_arc),
            ("as an `Arc<dyn Timer>`", erased),
        ] {
            assert_eq!(
                setup.timer.now(),
                INSTANT,
                "a clock handed over {form} reads its own time"
            );
        }
    }

    #[test]
    fn a_handle_handed_to_a_setup_stays_the_one_arc() {
        let recorder = Arc::new(MemoryRecorder::new());
        let setup = RunSetup::new(recorder.clone(), OfflineBroker, Pinned(INSTANT));
        assert_eq!(
            Arc::strong_count(&recorder),
            2,
            "the setup holds one share of the handle's `Arc`"
        );
        assert!(
            std::ptr::addr_eq(Arc::as_ptr(&setup.recorder), Arc::as_ptr(&recorder)),
            "the setup's `Arc` points at the recorder itself, with no second `Arc` around it"
        );

        let erased: Arc<dyn RunRecorder> = recorder;
        let setup = RunSetup::new(erased.clone(), OfflineBroker, Pinned(INSTANT));
        assert!(
            Arc::ptr_eq(&setup.recorder, &erased),
            "an `Arc<dyn RunRecorder>` is kept as it is too"
        );
    }

    #[test]
    fn a_setup_starts_without_streaming_and_the_setter_turns_it_on() {
        let setup = RunSetup::new(MemoryRecorder::new(), OfflineBroker, Pinned(INSTANT));
        assert!(
            setup.streaming.is_none(),
            "a setup starts without streaming"
        );
        assert!(
            format!("{setup:?}").contains("streaming: false"),
            "`Debug` shows that streaming is off: {setup:?}"
        );

        let setup = setup.streaming(|_round, _piece| {});
        assert!(setup.streaming.is_some(), "the setter stores the callback");
        assert!(
            format!("{setup:?}").contains("streaming: true"),
            "`Debug` shows that streaming is on: {setup:?}"
        );
    }
}
