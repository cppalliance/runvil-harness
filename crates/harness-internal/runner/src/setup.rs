//! What one run gets on top of its Host: [`RunSetup`], and the forms in
//! which a setup takes a service, [`IntoShared`].

use std::fmt;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
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

/// A Host callback that receives each live piece of a section's own model
/// round and answers whether to keep receiving pieces for that round.
///
/// The Harness keeps this callback for the run and calls it synchronously,
/// one piece at a time, only while the run's future is polled. For each
/// section round, the Harness gives the broker a separate round-scoped
/// [`PieceSink`](crate::performers::PieceSink), which the broker must not
/// retain beyond that round.
pub type HostSink = Arc<dyn Fn(Round, Delta) -> ControlFlow<()> + Send + Sync>;

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
    /// The Host's callback for live pieces of a section's own model round,
    /// when streaming is turned on for the run.
    pub streaming: Option<HostSink>,
}

impl RunSetup {
    /// A setup over `recorder`, `broker`, and `timer`, the three services
    /// a run waits on. Every other field starts
    /// empty or at its default: an empty memory filesystem, no selection,
    /// the empty object `{}` as its args, `RunLimits::default()`, at most
    /// 10 tool calls in flight, and no user. Each of those fields has a
    /// method of its name that sets it and returns the setup.
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

    /// Turns streaming on for the run. `sink` is called for each piece of a
    /// section's own model round, in order, and must not block. It is called
    /// synchronously, one piece at a time, only while the run's future is
    /// polled. The Harness keeps this callback for the run; the separate
    /// broker-facing sink is round-scoped and must not be retained beyond
    /// that round. Returning [`ControlFlow::Break`] stops the pieces for the rest of that round;
    /// the next round starts open again.
    #[must_use]
    pub fn streaming(
        mut self,
        sink: impl Fn(Round, Delta) -> ControlFlow<()> + Send + Sync + 'static,
    ) -> RunSetup {
        self.streaming = Some(Arc::new(sink));
        self
    }
}

impl fmt::Debug for RunSetup {
    /// Shows every field but the recorder, the broker, and the timer.
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
    //! over stays the one `Arc` around its service.

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
}
