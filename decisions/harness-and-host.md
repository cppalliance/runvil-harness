# The Harness and the Host API: design decisions

This page covers the `runvil` crate, `Host`, `RunSetup`, one Harness for each run, the effect loop, inference brokers, the timer, what a Host hands a Rust Plugin, the resources a Host offers the Plugins of the C contract, the recorder and run events, the run inbox from the Host's side, and stop and cancel. A Host is an application that runs prompts, and it depends on one crate, `runvil`. At startup it builds one `Host` struct: its installed Packs and a prompt finder. For each run it fills a `RunSetup` with a recorder, an inference broker, a timer, a file system, and the run's settings, and creates one `Harness`. `Harness::run` is one future. It steps the Engine, performs each effect, writes every event, effect, and answer to the recorder, and returns a `RunReport`. A `RunControl` sends messages to the run, stops the work in flight, or cancels the run.

## 1. The three layers and the `runvil` crate

### The Engine emits effects, the Harness performs them, and the Host decides policy

The root `AGENTS.md` defines the terms. The Engine, the `runvil-*` crates, parses a prompt, steps a run, and emits an effect each time the run needs a model reply, a Pack's render of the tools a section offers it, a Pack tool's result, a timer, or a file operation. The Harness, the `runvil` crate and the `harness-*` crates, steps the Engine, performs every effect, and records every run. A Host is an application such as Workshop, and it "makes every policy decision".

The Harness owns the effect loop because one ordered record of a run exists only if the Harness sees every effect and every answer. Replay and resume, which are planned work, depend on the same condition.

### A prompt names roles and Pack ids, and the Host binds both

A prompt file names a model by a role, such as `strong`, and a set of tools by a Pack id under `uses:`. The Host supplies the rest: its selected model fills the roles, its `PackRegistry` supplies the Packs, and `RunSetup.vfs` is the file system that the run's tool calls work in.

### A Host depends on one crate, `runvil`

`runvil` re-exports the Harness, the Engine items a Host reads and builds, and the Rust Plugin API. `Harness`, `Host`, `RunSetup`, `RunReport`, and `RunControl` are at the crate root, and the rest of the Harness is in `runvil::harness`, which includes the loader of the C contract, the `HostResources` a Host hands it, the stock prompt finders, and the stock offline broker. `runvil::plugin` has the Rust Plugin API of the crate `runvil-plugin`, such as `PackRegistry`, `Pack`, and `PackFactory`. The types of the C contract are in `runvil-plugin-sys`, outside `runvil`. Each re-export is at one path.

A Host fills its `PackRegistry` in two ways. A Plugin that follows the C contract enters through the loader: `insert_plugin_dir` loads each library Plugin of a directory, and `insert_plugin` loads a Plugin that is compiled into the Host. Each returns a `PluginReport`, which holds the Plugins that loaded and each failure. Each takes the Host's `HostResources` beside the registry, and every tool call of a Plugin it loads is lent them (see Inference, the clock, and what a Host hands its Plugins). The Plugin `plugin-mcp` is a Rust function named `install`, which the Host calls with the registry, the Host's list of MCP servers, and the handle of the Host's tokio runtime.

The owner's goal: "I want the outside world to only see one runvil crate, not runvil-*."

### `runvil` leaves out the Engine's stepping API

`runvil` omits the Engine items that step a run and perform its effects, among them `Run`, `Step`, `RunParams`, `Effect`, `EffectAnswer`, and the `cancel` module. Code that steps a run itself imports them from `runvil-engine`, as the Harness does, and a Host cancels through `RunControl`. `runvil` re-exports every other Engine name, including names that the existing Hosts leave unused.

### `harness-gateway-client` is the Harness family's second public crate

`harness-gateway-client` contains what a Host needs to use the Gateway: `GatewayBroker`, which implements `InferenceBroker`; `GatewayChat`, the HTTP client it uses; `fetch_model_catalog`; and `provide_gateway_resources`, which offers the Gateway to the Plugins of the C contract (see Inference, the clock, and what a Host hands its Plugins). It also has `TokioTimer`, the stock timer of a Host that runs on tokio (see the same section). It is the one Harness crate besides `runvil` that an outside crate may depend on.

The client is a crate of its own for two reasons. The Host supplies inference and `runvil` excludes tokio, so `runvil` leaves HTTP to the Host. Every Host still needs a standard Gateway client.

### The Harness's async code uses `futures-util`, and any executor can poll a run

`harness-runner`, the crate with the effect loop, uses `futures-util` for its async work, and a structural check fails when `runvil` or an internal Harness crate lists `tokio` under `[dependencies]`. A Host therefore awaits `Harness::run` on whichever executor it has.

A Host's own parts may still need a runtime: the docs of `GatewayBroker` and of `TokioTimer` state that their futures need tokio.

## 2. `Host`, `RunSetup`, and one Harness per run

### A Host builds one `Host` at startup and one `RunSetup` for each run

`Host` is a struct with the public fields `packs` and `prompts`. A Host builds one and shares it behind an `Arc`. `RunSetup` is a struct of public fields that the Host fills for each run.

The two structs sort every input by its use: `Host` contains what a parse reads and what every run shares, and `RunSetup` contains what one run uses, which includes the services a run waits on: the recorder, the broker, and the timer.

### `Harness::new` takes the `Host`, a parsed `Prompt`, and the `RunSetup`, and `run` consumes the Harness

`Harness::new(host, prompt, setup)` creates the Harness for one run. `Harness::control()` returns a `RunControl`. `Harness::run()` takes the Harness by value and returns a `Send` future, so the Host takes the control first. A chat conversation is Workshop's own code around one run.

### `Host::parse` parses the prompt before a run exists, and a parse failure returns to the caller unrecorded

`Host::parse(name)` calls `Prompt::parse` with the Host's `PromptFinder`, which parses the named prompt and every prompt that the `prompts:` lists reach from it ([The Engine](engine.md)). It then checks the `uses:` list of each of those prompts against the installed Packs. It returns the parse error unchanged, or a `RequirementsUnmet` error that names each declared Pack that the Host lacks or that a Plugin inserted as unavailable. The line of a Pack that only a linked prompt declares starts with that prompt's name, as in `- review.md: web is unavailable: runvil/web is not installed`. A parse failure is unrecorded: the recorder's first call for a run is `begin_run`, inside `Harness::run`.

Only a Rust `install` inserts a Pack as unavailable. The loader drops every Pack of a contract Plugin whose load fails, so `Host::parse` reports each of them with the text `is not installed`, and the reason is in the `PluginReport` that `insert_plugin_dir` returned.

### `Host::parse` returns the prompt behind an `Arc`, and every run of the prompt shares it

`Host::parse` returns an `Arc<Prompt>`, and `Harness::new` takes one. A Host that runs a prompt more than once parses it once and passes a clone of that `Arc` to each `Harness::new`. Runs that are in progress at the same time then hold the same parsed prompt, and a later run reuses that parse.

A parsed prompt is shared because a run only reads it. One parse compiles every Lua block and reads every prompt that the `prompts:` lists reach, so a parse or a copy for each run would repeat that work and hold the same prompts in memory again. A Host that wants a changed prompt file to take effect calls `Host::parse` again, and the runs that hold the earlier parse are unaffected.

### A prompt that lists other prompts needs a finder that serves each listed name

The Harness calls the Host's finder only inside `Host::parse`. One parse finds the named prompt and then each prompt that the `prompts:` lists reach, so the finder has to serve every one of those names. A listed name that the finder lacks fails the parse, and the failure is placed on the `prompts:` line of the prompt that lists the name.

`runvil::harness` has stock finders. `FindTextPrompt` serves one prompt from text, and `FindFilePrompt` serves one from a file. Each fails a find for any other name with the code `not_found` and the message `no prompt named {name}`, so the two fit a prompt that lists at most itself. `PromptSet` serves the prompts that a Host inserts as text, as files, and as directories, each file of a directory under its file name, such as `greet.md`. A Host whose prompts execute each other therefore gives its `Host` a `PromptSet`, or a finder of its own that serves each name.

A run that has started holds its parsed prompt and every prompt that one links, and an `execute` runs one of those. `PromptSet::rescan` lists the set's directories again, which affects what later parses find and leaves every run in progress as it is.

Every find happens in `Host::parse` because a stock finder reads a file with a blocking call, and a parse runs before the run's future exists. The rule holds for a prompt that executes others because the parse has read them all.

### `RunSetup::new` takes the recorder, the broker, and the timer, and every other field has a default

`RunSetup::new(recorder, broker, timer)` sets the required fields. Every other field starts at a default: an empty in-memory file system in `vfs`, no selection in `selected_model`, the empty object `{}` in `args`, the default `RunLimits` in `limits`, 10 in `max_tool_calls_in_flight`, and `false` in `has_user`. The default limits include 24 loop rounds and 8 concurrent tasks.

The default of `args` is `{}`, which is what a caller sends when it leaves every argument out. A Host that leaves the field alone therefore starts a valid run of every prompt whose arguments are all optional.

The broker is a required argument because the Harness asks the broker of every run for its models before it prepares the run. A Host that leaves it out fails to compile. A Host that runs prompts without a model passes `OfflineBroker` (see Inference, the clock, and what a Host hands its Plugins), so the code that builds the setup says so. A broker that a Host could leave out would be missed only when a prompt first asks for a model. The timer is a required argument for a reason of the same kind (see Inference, the clock, and what a Host hands its Plugins).

### Each optional field of `RunSetup` has a method that sets it and returns the setup

Each field that `RunSetup::new` starts at a default has a method of the same name, which takes the setup, sets the field, and returns the setup. A Host writes a setup as one expression, as in `RunSetup::new(recorder, broker, timer).args(text).has_user(true)`. The method `selected_model` takes the model id itself. The fields are public too, so a Host that holds a setup assigns a field directly.

The methods let a Host name only the fields it changes. The recorder, the broker, and the timer are set by `RunSetup::new` alone, because they are its required arguments.

### The Harness reads each `RunSetup` field once, and the run's tool calls work in the one file system that the setup holds

The Harness reads each field once per run. This matters most for `vfs`, the file system that the run's tool calls work in: the Harness performs every file request of the run's tool calls on that one file system. The run's store is outside it. The Engine makes each run an empty store of its own, which the run's own Lua fills and which ends with the run. The `Policy` of that file system, such as a `ModePolicy` in its Ask, Plan, or Agent mode, sees the file operations of tool calls. So does the operation sink that `VfsRefBuilder::on_op` installs. A store operation passes both by.

The Host chooses where the files of a run's tool calls are stored, because the Engine only executes the prompt, and each file request of a tool call works on the file system that the Engine's caller supplies. A Host's policy ends at that file system, and a store is outside it: a policy decides what a run's tools may do to the Host's files, and the store is the run's own memory. [Files and the VFS](files-and-the-vfs.md) covers the file system.

### The Harness passes `args` to the Engine as the Host set it, and the run's result is the report's `result`

`RunSetup.args` is the text of one JSON object, and the Harness passes it to the Engine unread. The Engine parses and checks the text at the run's first step ([The prompt language](prompt-language.md)), so text that fails the check ends the run with the code `args_invalid`, and the Host reads that error in `RunReport.result`. When the run ends with its final text, `RunReport.result` is that text: the value that the prompt returned, or the text `done` when the prompt returned nil or its walk ran past the last section. A prompt that built a file returns the file's text, and the Harness leaves the run's files unread after the run.

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

The prompt's Lua asks with a `store` call, and the operation works on a store of the run. A run has the store that it starts with, and one more for each `execute` ([Files and the VFS](files-and-the-vfs.md)). The Harness reaches a run's store only to perform the one operation of a `Vfs` effect. It leaves the store alone before the run and after it.

A tool call asks through the files of its context ([Tools and Packs](tools-and-packs.md)). The loop reads the requests of the calls in flight beside their answers and reports each one to the run with `Run::request_file`. The run issues the request as a `Vfs` effect that names the call, and the loop performs it on the file system of `RunSetup.vfs`, under the call's own identity and the Host's policy. The loop then resumes the run with a receipt of the outcome and hands the whole outcome to the call, which stays in flight throughout. The file operations of a call end when the call lands or is dropped: the loop drops a request that the call still makes, and answers `Dropped`, unperformed, a `Vfs` effect that the run issued for it. A panic of the Host's policy or storage inside an operation reaches the tool as a failed operation, and the run goes on.

With a recorder that answers at once, the loop goes from one file request to the next in one stretch, inside a single poll by the executor that polls the run. So after a bounded number of file requests it returns to that executor once, before it reports the next request. Which task the executor polls next is the executor's choice, so the return is one more chance for the Host's other tasks, one that stops or cancels the run among them, and the executor may still poll the run again first. An executor that takes its tasks in turn runs them then, so a tool call that keeps asking for files shares the thread with them. A file operation is one blocking call, so the loop returns only between operations, however long a backend takes.

The loop performs the operation itself because file operations are synchronous by design, so the outcome is ready when the call returns. The owner's reason: "all file I/O synchronizes the, the harness, and that's in line with our design. Our whole design of Runvil is a single-threaded. It's cooperative asynchrony." A store is memory. The owner's rule: "Every prompt starts with a, a memory store that's completely empty. And that's the only type of store there is. And it doesn't map to any file system path in the virtual file system." With the store closed to the outside, a run's result depends only on its prompt, its arguments, and its answers.

### The loop answers `Dropped` for a performer that panics

The loop catches a panic from a broker, a Pack, or a timer, logs it with the effect id, and answers the effect `Dropped`. The Lua call that waited raises the canceled error, which a `pcall` can catch, and an uncaught one ends the run as canceled.

### The Harness sends each tool call to the Pack named by the id's first segment, and at most 10 run at once

The run's Pack instances perform every `ToolCall` and `Render` effect. The Harness finds the Pack from the first segment of the tool id and calls `Pack::call`. The lookup skips the run's tool catalog, because a model can call a model-only tool, which a Pack's `render` adds outside the catalog.

At most `RunSetup.max_tool_calls_in_flight` tool calls run at once, 10 by default, and the loop starts waiting calls lowest effect id first. The count is one for the whole run, so the tool calls of a prompt that an `execute` runs are in it with the calls of the run's own prompt. Chat, render, and timer effects start at once and are outside the count. [Tools and Packs](tools-and-packs.md) covers the cap's reasons and `Pack::call`.

### A tool call of a contract Plugin that waits is parked, and its Plugin wakes it

A tool call of a contract Plugin runs as a series of steps, and the Harness sends a step when the call's future is polled. A step that waits on something of the Plugin's own, such as a reply from the network or a timer, parks the call. The loop polls the call again when the Plugin wakes it, from whichever thread the Plugin wakes on, or when the answer to the call's file request is ready ([Tools and Packs](tools-and-packs.md)). A parked call costs the run no polls. It holds one of the run's tool call slots while it waits, and a stop or a cancel drops it at any time, as it drops any call in flight. A call that its Plugin never wakes therefore holds its slot until a stop or a cancel.

A parked call waits the way every other effect of the run waits, on a wake, so a run whose tools wait on the network or on a timer leaves the Host's threads idle.

### A pending run whose effects are all answered ends with an `Internal` error, unless its main chain waits for a message

A `Step::Pending` with an empty effect list, while every earlier effect is answered, shows that the loop lost an effect. The loop ends the run with an `Internal` error: "the effect loop has nothing to await for a pending run". The Engine ends its own stalls with the code `stalled` or `internal`; see [The Engine](engine.md).

When the main chain, the run's root task, waits on its inbox of sent messages, `Step::Pending` sets `waiting_for_input`, and the loop waits for a `RunControl::send` or a cancel. The same applies to a run whose `has_user` is `false`, since a Host may still call `RunControl::send` from code.

## 4. Inference, the clock, and what a Host hands its Plugins

### The Host supplies model inference as an `InferenceBroker`

`InferenceBroker` is a trait with the methods `models()` and `chat(binding, messages, tools, options, round)`. `models()` lists the models the Host offers. `chat` runs one model round and returns the finished reply, and the loop calls it for each `Chat` effect. The broker decides where it sends each round, so a Gateway change applies to the next round.

The Host supplies inference so that the Host's broker owns the Gateway's URL, its key, and its model catalog, and `RunSetup` needs only the broker. [Model calls and conversations](models-and-conversations.md) covers the request a round sends.

### The Harness picks the run's model once, and that model fills every role

Before it prepares the run, the Harness awaits `models()` until the broker answers or the Host cancels the run, so a broker may wait until it has a model. The Harness picks the model that `RunSetup.selected_model` names, or the first listed model when the field is `None`. A selection absent from the list ends the run with `RequirementsUnmet` and the reason `model_absent`. The Harness passes the Engine a role map with one entry, `default`, so the picked model fills every role. Workshop's broker uses the wait: it answers once its catalog has a chat-capable model.

The map has one entry because `RunSetup` has one `selected_model`. A model for each role is planned work.

### `OfflineBroker` is the stock broker of a Host that runs prompts without a model

`runvil::harness` has a stock broker, `OfflineBroker`. Its `models()` lists an empty catalog, and its `chat` fails every round with the code `Unavailable`. A run over it that selects no model picks none, and the Harness passes the Engine an empty role map. A prompt that asks no model runs to its end. A `models.infer` or a `models.loop` raises an error of kind `model_required` before a round reaches the broker ([Model calls and conversations](models-and-conversations.md)). A run that selects a valid model id ends with `RequirementsUnmet`, because the list lacks the selection.

The Harness exports the broker because Hosts run prompts without a model: a tool that runs a prompt whose Lua does all the work, and a Host whose Gateway is not configured yet. Each would otherwise write the same broker and import the chat vocabulary of a round to do it. The Host still passes the broker to `RunSetup::new`, so a run without a model is a choice that the Host's code states.

### The broker returns each reply whole, and a Host streams inside its own broker

`InferenceBroker::chat` returns one finished `Completion`. A Host that shows a reply as it forms streams the pieces from inside its broker. `GatewayBroker::chat_streaming` runs the same round and passes each piece to a callback. The record contains the finished reply, as an `assistant_reply` event, and omits the pieces.

The owner's rule: "the engine, the harness would not know anything about streaming. only the host, through the inference broker. and for headless hosts, they dont need to stream at all".

### The Host's `Timer` performs each sleep and stamps each run's start

`Timer` is a trait with the methods `sleep(duration)` and `now()`. `sleep` returns the future that the loop polls for a `Timer` effect. `now` returns the time that the Harness records as the run's start and passes to the Engine, and its default reads the system clock.

The timer is a required argument of `RunSetup::new`, so a Host that leaves it out fails to compile. A timer that a Host could leave out would be missed only at a timed wait inside a run. It is part of the setup because a run is what uses it: the Harness reads the run's start from it and performs the run's sleeps through it, and a parse never waits. A Host that only parses prompts therefore supplies no timer, and two runs of one Host may each have a clock of their own.

### `TokioTimer` is the stock timer of a Host that runs on tokio

`harness-gateway-client` has a stock timer, `TokioTimer`. Its `sleep` returns one `tokio::time::sleep`, so the future needs a tokio runtime with its timer, and its `now` is the trait's default. A Host on tokio passes `Arc::new(TokioTimer)` to `RunSetup::new`, so the code that builds the setup still names the timer.

The timer is in `harness-gateway-client` because `runvil` excludes tokio, and the Gateway client is the public Harness crate that depends on tokio. Each Host on tokio would otherwise write that timer itself. A Host on another runtime implements `Timer` over that runtime's sleep.

### The Host hands a Rust Plugin what its Packs need as the arguments of `install`

A Rust Plugin is a function named `install` that inserts its Packs into the Host's `PackRegistry`, and the Host calls it at startup. The function's signature is its own: it takes the registry and whatever else its Packs need from the Host, as plain arguments, and a Pack factory stores what its instances need. `plugin_mcp::install` takes the Host's list of MCP servers and the handle of the Host's tokio runtime. A contract Plugin is handed JSON values, the Host's resources (see the next entry), because a Rust object stays on the Host's side of the C contract. The bashkit Plugin and the web Plugin therefore each own a tokio runtime.

A plain argument is enough because a Rust Plugin is compiled into the Host and shares the Host's Rust types, and the compiler checks that the Host passes each argument. [Tools and Packs](tools-and-packs.md) covers a Pack's side.

### The Host fills one `HostResources`, and each tool call of a contract Plugin is lent them as text

`HostResources` holds what a Host offers the Plugins it loads through the C contract: JSON values under names of two segments, a vendor and a name, such as `runvil/web-search`. A Host makes one with `HostResources::new`, offers each resource with `provide(name, read)`, and passes the whole to `insert_plugin_dir` and `insert_plugin`. `read` is a function that returns the resource's value, or nothing when the Host has none to offer right now. `provide` refuses a name that is not a vendor and a name, and a name that is offered already, with an `InvalidValue` error that names it. A Host that loads nothing through the contract, or offers nothing, passes an empty one.

The Harness calls each `read` when a tool call of a contract Plugin starts, on the task that polls the run, so `read` returns at once. The call is lent the JSON text of one object that holds each value under its name, and every step of the call is lent that same text. A `read` that returns nothing leaves its resource out of that call, and a `read` that panics ends that one call as any panic in a call does. A clone of a `HostResources` shares the functions, and its `Debug` lists the names and leaves the values out, because a value can hold a key.

A resource is read when a call starts so that a Host whose value changes has nothing more to do: the next call reads the new value, and no task of the Host hands changes on. `provide` takes a function alone, because a value that stays the same is a function that returns it. A resource is a JSON value because that is what crosses the contract: a Plugin in any language reads the text, where a Rust object would stay on the Host's side. [Tools and Packs](tools-and-packs.md) covers a Plugin's side.

### A Host offers the Gateway to contract Plugins with `provide_gateway_resources`

A Host that uses the Gateway offers it to the Plugins it loads through the C contract with one call, `provide_gateway_resources(resources, current)` of `harness-gateway-client`. `current` is a function that returns the Gateway to use right now, as the API root and the key that `GatewayBroker::new` takes, or nothing when the Host has no Gateway right now. The call offers each resource that the Gateway serves to Plugins. The Gateway serves them its web search, under the name `runvil/web-search`, and the value is a JSON object with the fields `version`, which is 1; `url`, the address of the Gateway's web search route under that API root; and `api_key`, the Gateway's key. Version 1 means that a Plugin sends the Gateway's web search request to `url` as the JSON body of a POST, with `api_key` as its bearer key, and reads the Gateway's web search reply. An entry that leaves `api_key` out means a request with no `Authorization` header, and the function always writes the key. A `current` that returns nothing leaves the entry out of that call. A second call on the same `HostResources` is refused, because the name is offered already. The search tool of the `web` Pack reads this entry ([Tools and Packs](tools-and-packs.md)).

`current` is a function because a Host's Gateway can move or stop. The Harness asks it as each tool call starts, so a call gets the Gateway of that moment. One call offers every resource so that a Gateway that serves Plugins a second thing changes this function and leaves each Host as it is. The entry holds an address and a key because a library Plugin links an HTTP client of its own and sends the request itself, where a search object of the Host would stay on the Host's side of the contract. The version is a field of the entry and is absent from the request body, because the Gateway refuses a body with a field it does not know. The name and the shape are written twice, in this function and in the Plugin that reads the entry, because a Plugin builds independently of the Harness. The function is in `harness-gateway-client` because that crate alone reads the key out of the client's secret value. The entry holds the key as plain text, so every Plugin that is lent those resources can read it.

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

`CancelHandle::child()` returns a handle that its parent also cancels. A caller of the Engine can give each of several runs a child of one parent, so that one cancel of the parent ends them all. The Harness creates one handle for each run. Every task of a run shares the run's handle, so the handle cancels the run as a whole, and the owner of a single task stops it with `tasks.cancel`.

## 7. Planned work

Each of these items is still to be built.

- **Replay and resume.** A driver that builds a run from a recorded seed and start time, passes the recorded answers to `Run::resume`, the messages to `Run::deliver`, and the file requests of tool calls to `Run::request_file`, and reports where the second run diverges. Today the recorder receives the seed, the start time, every answer, every message, and each file request of a tool call as the record of its `Vfs` effect.
- **A model for each role.** Today `RunSetup` has one `selected_model`, which fills every role.
- **Small records for large answers.** Today an answer record stores the whole tool output, or the whole file text for a `store` call, so the record contains a copy of everything a script reads. For a file operation of a tool call the answer record holds a receipt, such as the number of bytes of a read.
- **Request limits for a Host.** Today `GatewayBroker` always gives a chat round a receive timeout of 120 seconds and a reply cap of 16 MiB. Its model list request has the same cap and waits as long as the Gateway takes to answer.
- **A step hook.** Pack code that rewrites an effect or its answer between `Run::step` and the performers. Today the loop starts each effect as `Run::step` returned it and resumes the run with each answer as its performer returned it.
- **More for a Host to hand its Packs.** Secrets, a schedule, a model client for Packs, and the parent environment of a sub-run. Today a library Plugin is lent the Host's resources, and the one resource that a shipped Pack reads is the Gateway's web search.
- **One Plugin API.** `mcp` as a Plugin that follows the C contract, which a Host inserts with `insert_plugin`, and with it contract functions for configuration, an unavailable Pack with its reason, and the origin of a call, and something in place of the runtime handle that the Host passes to `install`. Today the Plugin is a Rust function named `install`, compiled into the Host, which takes its list of servers and the Host's runtime handle as arguments ([Tools and Packs](tools-and-packs.md)).
- **Approvals.** An approval as an effect of its own, with the dialog in the Host. Today the effect kinds are `Chat`, `Render`, `ToolCall`, `Vfs`, and `Timer`. When the `Policy` of `RunSetup.vfs` returns `Ask` for a file operation of a tool call, the file system refuses it with `VfsError::ApprovalRequired`, and a store operation passes the `Policy` by.
- **Requirement gaps before a run.** A call that returns every gap as data before a run exists. Today `Host::parse` reports missing Packs, and every other gap ends a run with a `RequirementsUnmet` error.
