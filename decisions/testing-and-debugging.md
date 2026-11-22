# Testing and debugging: design decisions

This page covers test support, fixtures, scripted models, the facade suites, the rules for test code, logs and traces, and dev tools. A step of a run returns effects: requests for outside work, such as a model reply, that the caller performs ([The Engine](engine.md)). A test steps a run on its own thread and answers every effect itself, so the clock, the seed, and the model's replies are values the test supplies. Each internal Engine crate has its own test support. The test support of `runvil-runtime` defines a scenario, which holds one test's setup, and a scripted model, which answers from a list of replies. The facades `runvil-engine` and `runvil`, two crates of re-exports, each have a suite written against public paths. For debugging, the Harness writes every effect, answer, and event of a run to the Host's recorder.

## 1. The Engine's test support

### Engine tests step a run on the calling thread with the serial driver

The serial driver, in the test support of `runvil-runtime`, steps a run on the calling thread and resumes it with the answer that the test's closure returns for each effect. Every Engine test is a plain `#[test]` function.

The owner's rule: "tokio is not allowed anywhere in the engine". Tests need only the serial driver, because the Engine leaves the order and the timing of effects to whoever answers them.

### A scenario is one test's setup, and the test's answers express concurrency, time, and a cancel

A scenario holds one run's setup as data. Running it steps the run and returns a trace with the result, the events, and each effect paired with its answer. The scenario answers a `Timer` effect at once. A test postpones an answer, cancels from a second thread, and asks for a seeded order of answers. A test that inspects something below `Run` uses a second stepper, which steps a scenario's run and returns the scheduler to the test between steps.

### A scripted model answers each `Chat` effect from a list of replies, in the test process

The scripted model takes a list of replies: text replies, batches of tool calls, and failures. For each `Chat` effect it returns the next reply and records the round's messages, tools, and options. After the last reply the script repeats it.

A scripted model in the test process is enough, because the Engine leaves I/O to its caller, and the code that sends a round over HTTP is outside the Engine's crates.

### Each internal Engine crate has its own test support, under the same names

Each internal Engine crate has its own test support module. The names repeat from crate to crate: a builder for prompt source, prepared answers (a text reply, a batch of tool calls, a failure, and a tool whose call is a closure), and a driver that returns a record.

The names follow the Engine facade's suite, so that tests read alike across the workspace. Each crate has its own module because one shared test crate would need a dependency of its own on each crate and would cross the `test-support` boundary, which the next entry describes.

### The `test-support` feature of `runvil-runtime` compiles the serial driver for a bench

`runvil-runtime` declares the empty feature `test-support`, which compiles the serial driver and the scripted model for a bench. `runvil-lua`, `runvil-prompt`, and `runvil-plugin` declare a feature of the same name for the tests of sibling crates. A structural check fails when a production dependency enables the feature.

The feature exists because `#[cfg(test)]` code is invisible to a bench, and this bench needs the serial driver and the scripted model.

## 2. The facade suites

### The Engine facade's suite uses public paths, and a test that needs more moves into the Engine

The suite of `runvil-engine` names `runvil_engine` paths for every Engine item. A test that needs a private item moves into the Engine's unit tests.

A re-export made for a test grows a public surface that is hard to shrink later, because removing a public item breaks the crates that depend on it. The owner's rule: "I want ZERO public API surface growth just for tests."

### The Engine facade's suite answers each effect by a fixed rule and fails a test that would wait forever

The suite has its own support code: a scenario, a driver that steps a run, and a trace. The driver performs a `Vfs` effect with `perform_vfs_op` and answers a `Chat` effect with the next scripted reply. It fails the test when the script runs out, when replies are left over at the end, and when the run waits only on postponed effects.

The support code exists to reduce the number of lines of code in the suite.

### The Engine facade's suite contains the tests that add coverage, each a function named with a sentence

The facade re-exports items of the internal crates, so those crates test behavior. The facade's suite contains a test only when it checks something beyond what an internal test or the facade's checked-in API listing covers. The owner's rule: "If it's not doing us any good, we should remove it, because all that does is create a tax on the vibe coding."

Each test is a separate function with a sentence for its name, so that one failure names one test.

### Harness tests write their own scripted brokers, and the Harness exports `OfflineBroker` and `MemoryRecorder`

A Harness test that needs a model's replies supplies them through an `InferenceBroker` that the test code defines. A test whose prompt asks no model uses `OfflineBroker`, the stock broker of a Host that runs prompts without a model ([The Harness and the Host API](harness-and-host.md)). `MemoryRecorder` is public and stores each run's records in a vector. `harness-runner` keeps the function that drives the effect loop public, so that tests have one entry point to the effect loop.

A public scripted broker would be test API that only the Harness's own tests use, so the Harness leaves it out. `OfflineBroker` is public because Hosts use it, and the tests use it because it is there.

## 3. Rules for test code

### Every clock value, seed, and answer in an Engine-crate test is a value the test supplies

The clippy configuration bans sleeps, clock reads, timed waits, and `RandomState::new` in `runvil-engine`, `runvil-plugin`, and the internal Engine crates, tests included. The test runner, nextest, ends a hung test after three 60-second periods.

The owner's rule: "my instinct tells me that no runvil test should ever sleep or be non-deterministic". The rule covers the Engine crates only, because some Gateway tests are timing-based by nature. The Engine's own determinism rules are on [The Engine](engine.md) and [Tasks and fanout](tasks-and-fanout.md).

### Random test input uses a fixed seed, and a temp folder is named after the test and the process id

`AGENTS.md` states both rules. The ban in the entry above covers only entropy from the operating system and the clock, so a test may draw input from a fixed seed, and a failure prints its seed. The Engine crates name and remove their temp folders through their own test support because the `tempfile` crate depends on the entropy crates `fastrand` and `getrandom`, and the Engine crates exclude entropy crates even as transitive dependencies.

### Some async tests outside the Engine crates pause tokio's clock, and tests on real sockets wait in real time

Some async tests run with tokio's clock paused: in the Harness runner and the Gateway client, in Workshop, and in the Gateway. A test that uses a real socket, a subprocess, or an operating-system thread waits in real time, because paused time with real sockets fires the timeouts of the code under test early.

### A test checks behavior, and its pass condition is a true-or-false property

`AGENTS.md` states the rule in its Engineering section. One test shows the reason: a test of the shipped research prompt runs the prompt and inspects every `Chat` effect for a tool id. A scan of the prompt files would check only the shape of the source, and the test on `Chat` effects protects the actual contract.

Two build rules apply to tests: a change that removes a feature also deletes the feature's tests, and a test file beside its module takes the name that `AGENTS.md` gives it.

## 4. Fixtures and stand-ins

### Whole prompt files are fixtures for the Engine's suite and for the facade's parser tests

Complete prompt files are fixtures. The Engine's suite has valid, invalid, and execution fixtures. A test that uses a fixture names the file by hand in an `include_str!`, and one test also walks the `valid` and `execution` folders and parses every prompt there, so a fixture that the other tests leave out still has to parse. The facade's parser tests have invalid fixtures of their own. A fixture is a complete prompt, and inline tests cover the narrow cases.

The fixtures exist because the inline tests leave a gap: a complete document, written the way an author writes one.

### A Plugin crate tests its tools through the test support of its API crate, and the loader's tests load a fixture library

The `test-support` feature of `runvil-plugin` exports a helper for one test call, which owns a tool id, a fresh in-memory file system, and an origin, and lends a `ToolContext`. The files of that context perform each request on that file system themselves and answer it at once, and they are a test's one way to it. The Rust Plugin `plugin-mcp` uses it. It exists because the dependency rules refuse a `plugin-*` crate that names the facade or the file-system crate.

A Plugin that follows the C contract tests through the test support of the Plugin SDK, which the SDK's own `test-support` feature exports. It is a fake Host: it sends `RV_PLUGIN_LOAD` to the Plugin's entry point, sends `RV_PACK_NEW`, and runs one tool call to its end. It holds one file tree for the calls of a run, performs each file request of a call on that tree once the step that asked has returned, and lends the tree back, so a test reads what a call left there. The fake Host checks each call that the Plugin makes back the way the loader does, so a Plugin that passes there loads in the Harness. The `std` Plugin, the bashkit Plugin, and the web Plugin test their tools this way.

The loader's end-to-end tests load `harness-test-plugin`, a dynamic library with a private copy of the C types. The private copy proves that a library Plugin builds independently of every Runvil crate. The loader's unit tests load entry points that are compiled into the test binary, as a Host that names one with `with_plugin` does.

## 5. Logs, traces, and dev tools

### The Harness writes every effect, answer, and event of a run to the Host's recorder

The Harness calls `RunRecorder::append` for each effect and answer and `RunRecorder::append_event` for each event, in the order of its effect loop (see [The Harness and the Host API](harness-and-host.md)). Workshop's recorder writes them to a database in its state directory, and a developer who debugs a run reads that database. Workshop runs prompts, and each facade has an example program.

Stored JSON reads back as the identical value, so that a replay can compare the results of a second run against the stored ones. The replay is still to be built.

### A prompt writes a trace line with `log`, and raw model bodies are opt-in events

`log(message)` accepts one single-line string of at most 256 characters, and the Engine reports it as an `Event::Lua` under the current section (see [The prompt language](prompt-language.md)). The execution fixtures assert the exact list of these checkpoints.

`RunParams::with_report_debug(true)` adds two events to each model round: `Request`, the JSON body sent, and `Response`, the body returned. The events exist so that a developer can diagnose an empty reply, and they are optional so that only a run that asks for them pays their cost. A Harness run leaves them off.

### The Harness reports operational problems through `tracing`, and Workshop discards those lines

The Harness reports operational problems with `tracing` macros, such as the error `Pack factory panicked; the Pack is unavailable to this run` and the warning `Plugin failed` for a Plugin that failed to load. The build of a `Host` also logs each Plugin that it loaded, at the info level. Workshop lacks a `tracing` subscriber, so its process discards these lines.

## 6. Planned work

Each of these items is still to be built.

- **A replay driver for run logs.** A driver that steps a recorded run through the Engine again and compares the results. Today Workshop's recorder stores every effect, answer, and event of a run, and a developer reads them there.
- **A CI job with the Windows symlink privilege.** A job on a Windows runner that has the privilege to create a file link. Today Windows runs skip the file-link tests of `runvil-vfs`.
- **Retiring `tools.call_as_model`.** `tools.call_as_model` is a test hook through which a fixture section calls a tool as a model would. Today test support installs it, and the Lua shim has a function that exists only for it.
- **The determinism rule for Harness, Gateway, and Workshop tests.** An extension to these tests of the rule that a test supplies every clock value, seed, and answer, which depends on deterministic drivers for those crates. Today the rule covers the Engine crates only.
