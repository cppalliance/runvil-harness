//! The stock tokio timer: each sleep is one sleep on tokio's timer.

use std::time::Duration;

use runvil::BoxFuture;
use runvil::harness::Timer;

/// The timer of a Host that runs on tokio.
///
/// A Host passes it as the timer of each run's `RunSetup`. Each sleep is one
/// `tokio::time::sleep`. When a stop or a cancel ends a wait, the Harness
/// drops the future, and the drop removes the sleep from tokio's timer.
/// `now` is the trait's default, which reads the system clock.
///
/// A sleep needs a tokio runtime with its timer, from the call of `sleep`
/// to the end of the future. A Host that uses this timer must therefore
/// await `Harness::run` inside such a runtime.
#[derive(Debug)]
pub struct TokioTimer;

impl Timer for TokioTimer {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the stock tokio timer, each on a paused tokio clock: a
    //! sleep ends once the clock has advanced by its duration and is
    //! still pending one second earlier, and a prompt's timed wait ends
    //! on the stock timer of its run. A paused clock jumps to the next
    //! timer when the runtime is idle, so a sleep of one hour ends at
    //! once in real time, and only a sleep on tokio's timer does that.

    use std::sync::Arc;

    use runvil::harness::{FindTextPrompt, MemoryRecorder, OfflineBroker, Record, RecordKind};
    use runvil::{Harness, Host, RunSetup};
    use serde_json::json;
    use tokio::time::Instant;

    use super::*;

    /// One hour, far longer than a test may wait in real time.
    const HOUR: Duration = Duration::from_secs(3600);

    #[tokio::test(start_paused = true)]
    async fn a_sleep_ends_once_tokios_clock_has_advanced_by_its_duration() {
        let start = Instant::now();
        TokioTimer.sleep(HOUR).await;
        let slept = start.elapsed();
        assert!(slept >= HOUR, "the sleep ended early: {slept:?}");
        assert!(
            slept < Duration::from_secs(3601),
            "the sleep ended late: {slept:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_sleep_is_still_pending_one_second_before_its_duration_ends() {
        let early = tokio::time::timeout(Duration::from_secs(3599), TokioTimer.sleep(HOUR)).await;
        assert!(
            early.is_err(),
            "a sleep of one hour ended within 59 minutes and 59 seconds"
        );
    }

    /// The name the test's Host finds its prompt by.
    const PROMPT: &str = "waits.md";

    /// A prompt whose main section waits one hour for a task that waits
    /// on its inbox, so the wait's timer fires first. The main section
    /// then sends the task its message and waits for it again, untimed.
    const WAITS_AN_HOUR: &str = "---\nname: waits\ndescription: waits one hour for a task\n\
        runvil: 0\n---\n\n\
        ## Main\n\n```lua\n\
        local t = tasks.spawn('../Child')\n\
        local first = tasks.join_any({ t }, { timeout = 3600 })\n\
        tasks.send(t, 'go')\n\
        local _task, ok, result = tasks.join_any({ t })\n\
        return tostring(first == nil) .. '|' .. tostring(ok) .. '|' .. result\n\
        ```\n\n\
        ## Child\n\n```lua\nreturn tasks.receive()\n```\n";

    /// Whether `record` is the effect record of a timer.
    fn is_timer_effect(record: &Record) -> bool {
        record.kind == RecordKind::Effect && record.payload.get("Timer").is_some()
    }

    #[tokio::test(start_paused = true)]
    async fn a_prompts_timed_wait_ends_on_the_stock_timer_of_its_run() {
        let host = Host::builder(FindTextPrompt::new(PROMPT, WAITS_AN_HOUR)).build();
        let prompt = host.parse(PROMPT).expect("the prompt parses");
        let recorder = Arc::new(MemoryRecorder::new());
        let setup = RunSetup::new(
            recorder.clone(),
            Arc::new(OfflineBroker),
            Arc::new(TokioTimer),
        );
        let start = Instant::now();

        let report = Harness::new(host, prompt, setup)
            .run()
            .await
            .expect("the recorder takes the run");

        assert_eq!(
            report.result,
            Ok("true|true|go".to_owned()),
            "the timed wait returns nil when its timer fires first, and the untimed wait \
             then takes the task"
        );
        let waited = start.elapsed();
        assert!(
            waited >= HOUR,
            "the run ended before tokio's clock advanced by the wait's hour: {waited:?}"
        );
        let records = recorder.records(report.run_id);
        let timers: Vec<&Record> = records
            .iter()
            .filter(|record| is_timer_effect(record))
            .collect();
        let [timer] = timers.as_slice() else {
            panic!("the timed wait issues one timer effect: {timers:?}");
        };
        assert_eq!(timer.payload, json!({ "Timer": { "seconds": 3600.0 } }));
        let answer = records
            .iter()
            .find(|record| record.kind == RecordKind::Answer && record.effect_id == timer.effect_id)
            .expect("the timer effect is answered");
        assert_eq!(
            answer.payload,
            json!("Timer"),
            "a fired timer is answered as a timer, not dropped"
        );
    }
}
