# The Harness and the Host API: design decisions

This page covers the `runvil` crate, `Host`, `RunSetup`, one Harness for each run, the effect loop, inference brokers, Host services, the recorder and run events, the run inbox from the Host's side, and stop and cancel. A Host is an application that runs prompts, and it depends on one crate, `runvil`. At startup it configures one `Host` with Packs, a prompt finder, a timer, and optional broker and recorder overrides. `Host::run` builds each named run with its arguments and settings; a Host may also construct a `Harness` from a shared parsed prompt and an explicit `RunSetup`. `Harness::run` is one future. It steps the Engine, performs each effect, writes every event, effect, and answer to the recorder, and returns a `RunReport`. A `RunControl` sends messages to the run, stops the work in flight, or cancels the run.

## 1. The three layers and the `runvil` crate

### The Engine emits effects, the Harness performs them, and the Host decides policy

The root `AGENTS.md` defines the terms. The Engine, the `runvil-*` crates, parses a prompt, steps a run, and emits an effect each time the run needs a model reply, a Pack's render of the tools a section offers it, a Pack tool's result, a timer, or a file operation. The Harness, the `runvil` crate and the `harness-*` crates, steps the Engine, performs every effect, and records every run. A Host is an application such as Workshop, and it "makes every policy decision".

The Harness owns the effect loop because one ordered record of a run exists only if the Harness sees every effect and every answer. Replay and resume, which are planned work, depend on the same condition.

### A prompt names roles and Pack ids, and the Host binds both

A prompt file names a model by a role, such as `strong`, and a set of tools by a Pack id under `uses:`. The Host supplies the rest: its selected model fills the roles, its `PackRegistry` supplies the Packs, and `RunSetup.vfs` is the file system that the run's tool calls work in.

### A Host depends on one crate, `runvil`

`runvil` re-exports the Harness, the Engine items a Host reads and builds, and the Rust Plugin API. `Harness`, `Host`, `RunBuilder`, `RunSetup`, `RunReport`, and `RunControl` are at the crate root, and the rest of the Harness is in `runvil::harness`, which includes the loader of the C contract and the stock prompt finders. `runvil::plugin` has the Rust Plugin API of the crate `runvil-plugin`, such as `PackRegistry`, `Pack`, and `HostServices`. The types of the C contract are in `runvil-plugin-sys`, outside `runvil`. Each re-export is at one path.

A Host fills its `PackRegistry` in two ways. A Plugin that follows the C contract enters through the loader: `insert_plugin_dir` loads each library Plugin of a directory and returns a `PluginReport`, and `insert_plugin` loads a Plugin that is compiled into the Host. The Plugins `plugin-web` and `plugin-mcp` are each a Rust function named `install`, which the Host calls with the registry, a configuration value, and the Host's services.

The owner's goal: "I want the outside world to only see one runvil crate, not runvil-*."

### `runvil` leaves out the Engine's stepping API

`runvil` omits the Engine items that step a run and perform its effects, among them `Run`, `Step`, `RunParams`, `Effect`, `EffectAnswer`, and the `cancel` module. Code that steps a run itself imports them from `runvil-engine`, as the Harness does, and a Host cancels through `RunControl`. `runvil` re-exports every other Engine name, including names that the existing Hosts leave unused.

### `harness-gateway-client` is the Harness family's second public crate

`harness-gateway-client` contains what a Host needs to call the Gateway: `GatewayBroker`, which implements `InferenceBroker`; `GatewayChat`, the HTTP client it uses; `fetch_model_catalog`; and `GatewaySearch`, the search provider for the `web` Pack. It is the one Harness crate besides `runvil` that an outside crate may depend on.

The client is a crate of its own for two reasons. Inference is a Host service and `runvil` excludes tokio, so `runvil` leaves HTTP to the Host. Every Host still needs a standard Gateway client. [The Gateway](https://github.com/cppalliance/runvil-design/blob/master/decisions/gateway.md) covers the routes it calls.

### The Harness's async code uses `futures-util`, and any executor can poll a run

`harness-runner`, the crate with the effect loop, uses `futures-util` for its async work, and a structural check fails when `runvil` or an internal Harness crate lists `tokio` under `[dependencies]`. A Host therefore awaits `Harness::run` on whichever executor it has.

A Host's own parts may still need a runtime: the docs of `GatewayBroker` state that its futures need tokio.

## 2. `Host`, `RunSetup`, and one Harness per run

### A Host builds one `Host` at startup and one `RunSetup` for each run

`Host::new(folder)` finds prompts relative to a folder, installs the built-in Packs, uses an executor-independent timer, and starts without models. Its `with_packs`, `with_prompt_finder`, `with_timer`, `with_broker`, and `with_recorder` methods replace those services. A clone shares the services and Pack factories. `RunSetup` keeps public fields for explicit per-run control.

`Host::run(name)` returns a `RunBuilder` that owns a snapshot of that configuration. `args` passes a JSON object as text, `model` selects a model, `interactive` enables the user inbox, and `configure` adjusts any setup field. Awaiting the builder yields a full report; `text` returns the prompt's result and attaches its run id to an execution error. `prepare` parses synchronously and returns an unstarted Harness whose control the Host can take. Dropping an unprepared builder reads no prompt and starts no record.

These defaults let a model-free prompt run without custom service implementations. A model call without a configured broker fails as unavailable, and a Host that needs persistent history configures a recorder. Each default run has its own memory recorder, released when the run ends.

The two structs sort every input by lifetime: `Host` contains what every run of one Host shares, and `RunSetup` contains what one run gets on top of the Host.

### `Harness::new` takes the `Host`, a parsed `Prompt`, and the `RunSetup`, and `run` consumes the Harness

`Harness::new(host, prompt, setup)` takes an owned Host snapshot and a shared parsed prompt to create the Harness for one run. `Harness::control()` returns a `RunControl`. `Harness::run()` takes the Harness by value and returns a `Send` future, so the Host takes the control first. A chat conversation is Workshop's own code around one run; see [Workshop: how the app is built](https://github.com/cppalliance/runvil-design/blob/master/decisions/workshop-app.md).

### `Host::parse` parses the prompt before a run exists, and a parse failure returns to the caller unrecorded

`Host::parse(name)` calls `Prompt::parse` with the Host's `PromptFinder`, which parses the named prompt and every prompt that the `prompts:` lists reach from it ([The Engine](engine.md)). It then checks the `uses:` list of each of those prompts against the installed Packs. It returns the parse error unchanged, or a `RequirementsUnmet` error that names each declared Pack that the Host lacks or that a Plugin inserted as unavailable. The line of a Pack that only a linked prompt declares starts with that prompt's name, as in `- review.md: web is unavailable: runvil/web is not installed`. A parse failure has no record: the recorder's first call for a run is `begin_run`, inside `Harness::run`.

Only a Rust `install` inserts a Pack as unavailable. The loader drops every Pack of a contract Plugin whose load fails, so `Host::parse` reports each of them with the text `is not installed`, and the reason is in the `PluginReport` that `insert_plugin_dir` returned.

### `Host::parse` returns the prompt behind an `Arc`, and every run of the prompt shares it

`Host::parse` returns an `Arc<Prompt>`, and `Harness::new` takes one. A Host that runs a prompt more than once parses it once and passes a clone of that `Arc` to each `Harness::new`. Runs that are in progress at the same time then hold the same parsed prompt, and a later run finds and parses nothing.

A parsed prompt is shared because a run only reads it. One parse compiles every Lua block and reads every prompt that the `prompts:` lists reach, so a parse or a copy for each run would repeat that work and hold the same prompts in memory again. A Host that wants a changed prompt file to take effect calls `Host::parse` again, and the runs that hold the earlier parse are unaffected.

### A prompt that lists other prompts needs a finder that serves each listed name

The Harness calls the Host's finder only inside `Host::parse`. One parse finds the named prompt and then each prompt that the `prompts:` lists reach, so the finder has to serve every one of those names. A listed name that the finder lacks fails the parse, and the failure is placed on the `prompts:` line of the prompt that lists the name.

`runvil::harness` has stock finders. `FindTextPrompt` serves one prompt from text, and `FindFilePrompt` serves one from a file. Each fails a find for any other name with the code `not_found` and the message `no prompt named {name}`, so the two fit a prompt that lists no prompt, or only itself. `PromptSet` serves the prompts that a Host inserts as text, as files, and as directories, each file of a directory under its file name, such as `greet.md`. A Host whose prompts execute each other therefore gives its `Host` a `PromptSet`, or a finder of its own that serves each name.

A run that has started holds its parsed prompt and every prompt that one links, and an `execute` runs one of those. `PromptSet::rescan` lists the set's directories again, which affects what later parses find and affects no run in progress.

Every find happens in `Host::parse` because a stock finder reads a file with a blocking call. An awaited named builder parses synchronously at its first poll before executing the Harness. A Host that needs to keep parsing off its executor calls `prepare` or `Host::parse` on its blocking pool. The rule holds for a prompt that executes others because the parse has read them all.

### `RunSetup::new` takes the recorder and the broker, and every other field has a default

`RunSetup::new(recorder, broker)` accepts concrete services or shared `Arc` values and sets those fields. `RunSetup::default()` supplies an offline broker and a fresh memory recorder. Every other field starts at a default: an empty in-memory file system in `vfs`, empty values in `selected_model` and `workspace_roots`, the empty object `{}` in `args`, the default `RunLimits` in `limits`, 10 in `max_tool_calls_in_flight`, and `false` in `has_user`. The default limits include 24 loop rounds and 8 concurrent tasks.

The default of `args` is `{}`, which is what a caller with nothing to pass sends. A Host that leaves the field alone therefore starts a valid run of every prompt that requires no argument.

A model-free run needs no inference service. The default broker lists no models and refuses model calls, so a Host only provides a broker when its prompts need one.

### The Harness reads each `RunSetup` field once, and the run's tool calls work in the one file system that the setup holds

The Harness reads each field once per run. This matters most for `vfs`, the file system that the run's tool calls work in: the Harness performs every file request of the run's tool calls on that one file system. The run's store is no part of it. The Engine makes each run an empty store of its own, which no setup fills and no report returns. The `Policy` of that file system, such as a `ModePolicy` in its Ask, Plan, or Agent mode, sees the file operations of tool calls. So does the operation sink that `VfsRefBuilder::on_op` installs. Neither sees a store operation. A prompt reads `selected_model` and the first of `workspace_roots` through `ui()`.

The Host chooses where the files of a run's tool calls are stored, because the Engine only executes the prompt, and each file request of a tool call works on the file system that the Engine's caller supplies. No Host policy applies to the store, because a store is no part of that file system: a policy decides what a run's tools may do to the Host's files, and the store is the run's own memory. [Files and the VFS](files-and-the-vfs.md) covers the file system.

### The Harness passes `args` to the Engine as the Host set it, and the run's result is the report's `result`

`RunSetup.args` is the text of one JSON object, and the Harness passes it to the Engine unread. The Engine parses and checks the text at the run's first step ([The prompt language](prompt-language.md)), so text that fails the check ends the run with the code `args_invalid`, and the Host reads that error in `RunReport.result`. When the run ends with no error, `RunReport.result` is its final text: the value that the prompt returned, or the text `done` when no section returned a value. A prompt that built a file returns the file's text, and the Harness reads no file after a run.

The Harness leaves the check to the Engine so that one piece of code decides what arguments a prompt takes, whichever caller starts the run.

A run has one way in, its arguments, and one way out, its result. The owner's rule: "If a prompt wants to return a file, it should return it as the string of its return value."

### `Harness::run` returns a `RunReport` for every run, and its one error is `RecorderFailed`

`RunReport` has the fields `run_id`, `result`, and `unread`. `result` is the final text or the `Error` that ended the run, and a cancel is the error whose code is `canceled`. The record begins before the model lookup, so a failed lookup and an early cancel each produce a report. `Harness::run` returns `Err` in one case, a recorder that refused a write, with the code `RecorderFailed`.

The Harness delivers a failed prompt as a result that says why the prompt failed, the same way it delivers a text that the prompt produced through inference.

### The Harness draws each run's seed from the operating system

The Harness draws 32 random bytes for each run, builds a `RunSeed`, and passes it to the recorder in `RunMeta` and to the Engine.

The Engine derives the nonce of the untrusted-output envelope from the seed, and a structural check forbids random sources in every Engine crate, so the Harness makes the draw. A seed read from a clock would be guessable, and a guessable seed makes a guessable nonce. [Tools and Packs](tools-and-packs.md) describes the envelope.

### The Harness waits for each Pack's `ready`, and a run that fails its start checks ends at the first step

Before the first step the Harness creates the run's Pack instances and awaits every instance's `ready` together, until all resolve or the Host cancels the run. Each Pack bounds its own `ready`, because a timeout in the Harness would be a policy that belongs to each Pack. `ready` is a method of the Rust trait `Pack`, and its default resolves at once. The C contract omits a readiness call, so a Pack of a contract Plugin is ready when `RV_PACK_NEW` returns, and the Packs that ship keep the default.

The run's first step checks the run's requirements. For a shortfall, such as a Pack whose factory refused this run, it ends the run with a `RequirementsUnmet` error that lists every gap. When the cancel flag is set, the step ends the run with the canceled error. A cancel that ends the wait early leaves a Pack that is still starting with zero tools, and the check would otherwise end a canceled run as refused. [The Engine](engine.md) lists the checks.

## 3. The effect loop

### `Harness::run` is one future that polls every effect

The effect loop polls the future of every effect in flight inside the one future that `Harness::run` returned, on whichever thread polls that future.

The design sets a rule for every performer, which is a broker, a Pack, or a timer: return to the poll quickly, and send blocking work to the Host's own runtime. A performer that blocks stalls every other effect of the run, and the run's stop and cancel with them. The rule covers a performer's own work, such as a network request or a long computation. A file operation is synchronous by design, and the run waits for each one ([Files and the VFS](files-and-the-vfs.md)).

### The Harness answers each `Vfs` effect inline, on the thread that polls the run

A `Vfs` effect is one file operation, and the effect says who asked for it: the prompt's Lua or a tool call ([The Engine](engine.md)). The loop performs the operation itself and resumes the run before it waits again. The operations run one at a time, in the order the Engine issued them, whichever chain or tool call asked for each.

The prompt's Lua asks with a `store` call, and the operation works on a store of the run. A run has the store that it starts with, and one more for each `execute` ([Files and the VFS](files-and-the-vfs.md)). The Harness reaches a run's store only to perform the one operation of a `Vfs` effect. It puts nothing in the store before a run and reads nothing from it after.

A tool call asks through the files of its context ([Tools and Packs](tools-and-packs.md)). The loop reads the requests of the calls in flight beside their answers and reports each one to the run with `Run::request_file`. The run issues the request as a `Vfs` effect that names the call, and the loop performs it on the file system of `RunSetup.vfs`, under the call's own identity and the Host's policy. The loop then resumes the run with a receipt of the outcome and hands the whole outcome to the call, which stays in flight throughout. A call that has landed or was dropped gets no further operation: the loop drops a request it still makes, and answers `Dropped`, unperformed, a `Vfs` effect that the run issued for it. A panic of the Host's policy or storage inside an operation reaches the tool as a failed operation, and the run goes on.

With a recorder that answers at once, the loop goes from one file request to the next without returning to the executor that polls the run. So after a bounded number of file requests it returns to that executor once, before it reports the next request. Which task the executor polls next is the executor's choice, so the return is one more chance for the Host's other tasks, one that stops or cancels the run among them, and no promise. An executor that takes its tasks in turn runs them then, so a tool call that keeps asking for files does not hold the thread for as long as it asks. A file operation is one blocking call, so the loop returns between operations and never in the middle of one, however long a backend takes.

The loop performs the operation itself because file operations are synchronous by design, so the call has nothing for the loop to wait on. The owner's reason: "all file I/O synchronizes the, the harness, and that's in line with our design. Our whole design of Runvil is a single-threaded. It's cooperative asynchrony." A store is memory. The owner's rule: "Every prompt starts with a, a memory store that's completely empty. And that's the only type of store there is. And it doesn't map to any file system path in the virtual file system." With no way in from outside, a run's result depends only on its prompt, its arguments, and its answers.

### The loop answers `Dropped` for a performer that panics

The loop catches a panic from a broker, a Pack, or a timer, logs it with the effect id, and answers the effect `Dropped`. The Lua call that waited raises the canceled error, which a `pcall` can catch, and an uncaught one ends the run as canceled.

### The Harness sends each tool call to the Pack named by the id's first segment, and at most 10 run at once

The run's Pack instances perform every `ToolCall` and `Render` effect. The Harness finds the Pack from the first segment of the tool id and calls `Pack::call`. The lookup skips the run's tool catalog, because a model can call a model-only tool, which a Pack's `render` adds outside the catalog.

At most `RunSetup.max_tool_calls_in_flight` tool calls run at once, 10 by default, and the loop starts waiting calls lowest effect id first. The count is one for the whole run, so the tool calls of a prompt that an `execute` runs are in it with the calls of the run's own prompt. Chat, render, and timer effects start at once and are outside the count. [Tools and Packs](tools-and-packs.md) covers the cap's reasons and `Pack::call`.

### A pending run whose effects are all answered ends with an `Internal` error, unless its main chain waits for a message

A `Step::Pending` with an empty effect list, while every earlier effect is answered, shows that the loop lost an effect. The loop ends the run with an `Internal` error: "the effect loop has nothing to await for a pending run". The Engine ends its own stalls with the code `stalled` or `internal`; see [The Engine](engine.md).

When the main chain, the run's root task, waits on its inbox of sent messages, `Step::Pending` sets `waiting_for_input`, and the loop waits for a `RunControl::send` or a cancel. The same applies to a run whose `has_user` is `false`, since a Host may still call `RunControl::send` from code.

## 4. Inference, the clock, and Host services

### The Host supplies model inference as an `InferenceBroker`

`InferenceBroker` is a trait with the methods `models()` and `chat(binding, messages, tools, options, round)`. `models()` lists the models the Host offers. `chat` runs one model round and returns the finished reply, and the loop calls it for each `Chat` effect. The broker decides where it sends each round, so a Gateway change applies to the next round.

The Host supplies inference so that the Host's broker owns the Gateway's URL, its key, and its model catalog, and `RunSetup` needs only the broker. [Model calls and conversations](models-and-conversations.md) covers the request a round sends.

### The Harness picks the run's model once, and that model fills every role

Before it prepares the run, the Harness awaits `models()` until the broker answers or the Host cancels the run, so a broker may wait until it has a model. The Harness picks the model that `RunSetup.selected_model` names, or the first listed model when the field is `None`. An empty selection or one containing control characters ends the run with `InvalidValue` before fetching the catalog. A selection absent from the list ends the run with `RequirementsUnmet` and the reason `model_absent`. The Harness passes the Engine a role map with one entry, `default`, so the picked model fills every role. Workshop's broker uses the wait: it answers once its catalog has a chat-capable model.

The map has one entry because `RunSetup` has one `selected_model`. A model for each role is planned work.

### The broker returns each reply whole, and a Host streams inside its own broker

`InferenceBroker::chat` returns one finished `Completion`. A Host that shows a reply as it forms streams the pieces from inside its broker. `GatewayBroker::chat_streaming` runs the same round and passes each piece to a callback. The record contains the finished reply, as an `assistant_reply` event, and omits the pieces.

The owner's rule: "the engine, the harness would not know anything about streaming. only the host, through the inference broker. and for headless hosts, they dont need to stream at all".

### The Host's `Timer` performs each sleep and stamps each run's start

`Timer` is a trait with the methods `sleep(duration)` and `now()`. `sleep` returns the future that the loop polls for a `Timer` effect. `now` returns the time that the Harness records as the run's start and passes to the Engine, and its default reads the system clock.

The timer is a required field of `Host`. As an entry of `HostServices` it could be missing, and a missing timer would show only at a timed wait inside a run.

### The Host fills one `HostServices` map, and the `install` of each Rust Plugin reads it

`HostServices` is a map from a two-segment id, such as `runvil/tokio-runtime`, to a typed object. The Host fills one map at startup and passes it to the `install` function of each Rust Plugin, and a Pack factory stores what its instances need. The C contract lacks Host services: a contract Plugin receives the contract's version and the functions of its load call, so the bashkit Plugin owns a tokio runtime of its own. Services for contract Plugins are planned work. The recorder, the broker, and the timer are services outside the map; the Host can replace their defaults.

A `ServiceKey` binds each id to the type of its object, so a Host provides only an object of that type under the key. [Tools and Packs](tools-and-packs.md) covers a Pack's side.

## 5. The recorder and run events

### The Host supplies the `RunRecorder` and receives a run's events through it

`RunRecorder` is a trait with the methods `begin_run(meta)`, `append(run, record)`, and `end_run(run, result)`. `begin_run` returns a `RunId`, `append` adds one record, and `end_run` closes the run. The Host chooses the storage: `MemoryRecorder` stores runs in memory, and Workshop's recorder writes a Turso database. Calls for different runs may overlap, so each recorder locks its own state.

A Host sees a run's events through its recorder: `append` is the only call in which the Harness passes an event to the Host.

### The Harness writes every event, effect, and answer in loop order

The loop writes a step's events before it starts the step's effects, each effect before its performer starts, and each answer before the run resumes with it. Every effect record has exactly one answer record, and a dropped effect's answer is `Dropped`. The loop also records an effect that the same step issued and withdrew. The records are the only view of a run's store outside the run: each `Vfs` effect record holds its operation, and its answer record holds the outcome, which is enough to rebuild the store. The record of a `Vfs` effect that a tool call asked for names the call and holds the operation, with the bytes of a write. Its answer record holds a receipt, such as the number of bytes of a read, and the tool's own result is the answer record of its `ToolCall` ([The Engine](engine.md)). A recorder that returns an error stops its run.

The loop records a withdrawn effect because each `Chat` record names an earlier round by id, in its `after` field. With every effect recorded, the `Chat` records of one message list, in record order, are exactly its sends, and every `after` names a recorded round.

### Events are values that `Run::step` returns, and dropping them leaves the run unchanged

An `Event` is one thing that happened in a run, such as a section start, a model reply, or a tool result. `Run::step` returns the events beside the effects, and each has a `section` and a `provenance`, the task id with a sequence number. The Engine's docs state the contract: dropping every event "leaves a run's outputs, errors, and ordering" unchanged.

Events exist to show a run's progress: a run that reported only its result would be one silent call that lasts minutes. [The Engine](engine.md) lists the event kinds.

### A reply from `models.infer` is an `assistant_reply` event with `origin` set to `infer`

Every model reply produces one `assistant_reply` event. Its `origin` field is `chat` for a chat turn and `infer` for a `models.infer` call, so a Host reads one event kind and uses `origin` to separate the two.

A second event kind for `models.infer` replies would duplicate the reply payload, and every Host would have to learn it.

### `RunMeta` has the seed, a reserved flags field, and the start time

`begin_run` receives a `RunMeta` with the fields `seed`, `flags`, and `started_at`. `flags` is reserved for replay, and the Harness writes 0. The recorder issues the `RunId`, and every later record names the run by that id. The recorder receives every record under its run's id, so an event omits a name for its run. Workshop's recorder stores its own label for a run.

The `flags` field exists so that an Engine change that alters an exit rule or a protocol detail sets a flag, which a replay of the run reads.

### Recorded JSON reads back as the identical value

Every payload that the Harness passes to a recorder is JSON that parses back to the same value and prints as the same text. Object keys are sorted, numbers are finite, and the workspace enables the `float_roundtrip` feature of `serde_json`. `AGENTS.md` states the rule.

The rule exists for replay, which is planned work: a replay compares the results of a second run against the stored payloads, so each payload has to read back as the identical JSON value. With the feature off, the float `3.9078000000000004` reads back as `3.9078`.

### Record formats change in place, and every reader changes with them

A change to an event, a record, or `RunMeta` also changes every reader, and it leaves stored data as it is. `AGENTS.md` states the principle for the whole repository: "There are no users and no recorded runs, logs, or stored data to preserve, so backward compatibility does not matter".

## 6. The run inbox, stop, and cancel

### `RunControl` has the calls `send`, `stop_round`, and `cancel`

`RunControl` is a handle that the Host clones and calls from any thread. `send(text)` queues a message for the run's main chain. `stop_round()` drops the work in flight, and the run continues. `cancel()` ends the run.

A stop is a call of its own because a cancel ends the run. After a cancel, Workshop would have to start another run with an empty history, and the model would forget a conversation that Workshop still shows. The handle omits an unsend, since a message that `send` accepted is committed.

### `RunControl::send` queues text for the main chain, and `RunReport::unread` returns what the run left unread

`send` queues the text and returns `true`. The next time the loop waits, it delivers each queued message to the Engine, records the events of the delivery, and steps the run. Once the run has ended, `send` drops the text and returns `false`. `RunReport::unread` lists the messages still in the inbox at the end and the ones that the loop left undelivered, so that a Host can return them to the person who sent them.

`send` applies only to the main chain: a sub-agent is a task in the same run, and only a run has a `RunControl`. [Tasks and fanout](tasks-and-fanout.md) covers what the main chain does with a message.

### The events `inbox_message` and `inbox_waiting` tell the Host whether the main chain waits for input

The Engine emits `inbox_message` when a message enters an inbox and `inbox_waiting` when a chain starts to wait on an empty inbox. Both name the chain in their `provenance`, and the main chain is task `0`. A Host reads them as its recorder receives them: after `inbox_waiting` on task `0` the run waits for the person, and after the next `inbox_message` it continues.

### `RunSetup.has_user` says whether a person answers the main chain

A Host sets `has_user` to `true` when a person can answer the run's main chain, and the run then has the built-in `runvil/user` Pack. With `false`, the Harness marks that Pack unavailable with the reason "this run has no user", and the first step refuses a prompt that declares `uses: [user]`. [Tools and Packs](tools-and-packs.md) covers the `user` Pack.

### A stop drops the effects in flight, and the run continues

`stop_round` raises a flag. When the loop sees it, the loop aborts every effect in flight and every tool call waiting for a slot, answers each `Dropped`, and lowers the flag. A tool call whose `ToolInfo` sets `survives_stop` continues. The cancel flag is still clear, so each Lua call that waited raises the canceled error, and a `pcall` around it lets the prompt continue.

The loop lowers the flag even when the set of effects in flight is empty, so a stop leaves every later effect untouched. A stop drops tool calls as well as model rounds, because a tool call in flight inside `models.loop` would otherwise continue, and the agent would still be busy after the stop.

### A stop raised before a send takes effect first

For a Host that calls `stop_round` and then `send`, the stop applies first: the loop drops the round in flight, `models.loop` raises the canceled error, and the message waits in the inbox for the chain's next read. The loop checks for a stop before each step and before it delivers a message. The Engine withdraws a model round that is in flight when a message is delivered, and issues it again with the message. A stop that the loop first sees after that step drops the reissued round unstarted, because the reissued round is the request that the Host stopped.

A message leaves tool calls running. To cut a long tool call short, the Host stops the round and then sends.

### A cancel drops every effect and ends the run with the `canceled` error

`cancel` sets the run's `CancelHandle`. The loop answers every effect `Dropped`, including calls that survive stops and calls that wait for a slot, and steps the run to `Step::Done`. The Engine reads the same flag, and its `pcall` raises a caught error again while the flag is set, because a cancel is the only way to stop a runaway loop.

`CancelHandle::child()` returns a handle that its parent also cancels. It exists so that an orchestrator can cancel its sub-agents. The Harness creates one handle for each run.

## 7. Planned work

Each of these items is still to be built.

- **Replay and resume.** A driver that builds a run from a recorded seed and start time, passes the recorded answers to `Run::resume` and the messages to `Run::deliver`, and reports where the second run diverges. Today the recorder receives the seed, the start time, every answer, and every message of a run.
- **A model for each role.** Today `RunSetup` has one `selected_model`, which fills every role.
- **Small records for large answers.** Today an answer record stores the whole tool output or file text, so the record contains a copy of everything a script reads.
- **Request limits for a Host.** Today `GatewayBroker` always uses a receive timeout of 120 seconds and a reply cap of 16 MiB.
- **A step hook.** Pack code that rewrites an effect or its answer between `Run::step` and the performers. Today the loop starts each effect as `Run::step` returned it and resumes the run with each answer as its performer returned it.
- **More Host services.** Secrets, a schedule, a model client for Packs, the parent environment of a sub-run, and a way for a contract Plugin to read a service. Today `plugin-web` and `plugin-mcp` read `runvil/tokio-runtime`, the Host's runtime handle, `plugin-web` reads `runvil/search-provider`, the search provider for the `web` Pack, and a contract Plugin reads none.
- **One Plugin API.** `web` and `mcp` as Plugins that follow the C contract, which a Host inserts with `insert_plugin`, and with them contract functions for configuration, Host services, an unavailable Pack with its reason, the origin of a call, and a wait on outside work. Today the two Plugins are Rust functions named `install` ([Tools and Packs](tools-and-packs.md)).
- **Approvals.** An approval as an effect of its own, with the dialog in the Host. Today the effect kinds are `Chat`, `Render`, `ToolCall`, `Vfs`, and `Timer`. When the `Policy` of `RunSetup.vfs` returns `Ask` for a file operation of a tool call, the file system refuses it with `VfsError::ApprovalRequired`, and the `Policy` never sees a store operation.
- **Requirement gaps before a run.** A call that returns every gap as data before a run exists. Today `Host::parse` reports missing Packs, and every other gap ends a run with a `RequirementsUnmet` error.

## 8. Open questions

Each question asks for the reason behind a part of the design.

**The API**

1. Why does a prompt name roles and Pack ids while the Host binds them?
2. Why does a parse failure return to the caller, outside the run's record?
3. Why does `runvil` leave out the stepping API and still re-export Engine names that the existing Hosts leave unused?
4. What will set `RunMeta.flags`? The field is reserved, and the Harness writes 0 in it for every run.

**The effect loop**

5. Why do only tool calls count toward the cap of 10, while model rounds start at once?
6. Why does a performer that panics end the run as canceled?

**Inference and the clock**

7. Why does `GatewayBroker` fix the receive timeout and the reply cap, while `GatewayChat::with_request_limits` is public?
8. Why does the Host's `Timer` stamp the run's start?

**The inbox and cancel**

9. Why does a Host learn whether the run waits for input from two events, through its recorder?
10. Why does `send` return `false` after the run has ended, where it could return an error?
11. What will call `CancelHandle::child()`?
12. Why does every run get a record, including one that ends before its first step?
