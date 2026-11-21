//! What one run gets on top of its Host: [`RunSetup`].

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use runvil_engine::RunLimits;
use runvil_engine::vfs::VfsRef;

use crate::performers::{InferenceBroker, Timer};
use crate::recorder::RunRecorder;

/// The tool calls a run has in flight at once unless its setup says
/// otherwise.
const DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT: NonZeroUsize = NonZeroUsize::MIN.saturating_add(9);

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
}

impl RunSetup {
    /// A setup over `recorder`, `broker`, and `timer`, the three services
    /// a run waits on. Every other field starts
    /// empty or at its default: an empty memory filesystem, no selection,
    /// the empty object `{}` as its args, `RunLimits::default()`, at most
    /// 10 tool calls in flight, and no user. Each of those fields has a
    /// method of its name that sets it and returns the setup.
    #[must_use]
    pub fn new(
        recorder: Arc<dyn RunRecorder>,
        broker: Arc<dyn InferenceBroker>,
        timer: Arc<dyn Timer>,
    ) -> RunSetup {
        RunSetup {
            recorder,
            broker,
            timer,
            vfs: VfsRef::default(),
            selected_model: None,
            args: "{}".to_owned(),
            limits: RunLimits::default(),
            max_tool_calls_in_flight: DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT,
            has_user: false,
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
            .finish_non_exhaustive()
    }
}
