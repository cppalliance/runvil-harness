# The Engine: design decisions

This page covers the Engine as a library: parsing a prompt, the run state machine, effects and their answers, run limits, the one error type, the public surface, and the internal crates. The Engine is a Rust library that parses a prompt file and runs it as a state machine. `Prompt::parse` compiles each Lua block and returns a `Prompt`, which also holds every prompt that it lists for `execute`, and `Run::new` builds a `Run` from it. Each `Run::step` runs the prompt's Lua until the run waits for answers and returns the effects the run needs: model rounds, Pack renders, Pack tool calls, file operations, and timers. The caller performs each effect and passes its answer to `Run::resume`. A run ends with its final text or with one `Error`. Private crates contain the code, and `runvil-engine` re-exports the names a caller uses.

## 1. The Engine and its caller

### The Engine returns the work it needs as an effect, and its caller performs it

An effect is a Rust value that describes one piece of outside work, such as a model round. `Run::step` returns the effects a run needs, and the run waits until the caller passes each answer to `Run::resume`. In production the caller is the Harness ([The Harness and the Host API](harness-and-host.md)). `Run::step` and `Run::resume` are synchronous functions, and a structural check fails when an Engine crate depends on an async runtime or an HTTP client.

The Engine returns this work as values so that it is independent of any runtime and is a deterministic function of its inputs.

### The Engine returns model rounds, Pack work, file operations, and timers as effects, and performs the rest itself

The variants of `Effect` are `Chat` for one model round, `Render` for a Pack's rendering of offered tools, `ToolCall` for one call of a Pack's tool, `Vfs` for one file operation, and `Timer` for one sleep. A file operation is a `store` call of the prompt's Lua, or a file request of an open tool call. [Tools and Packs](tools-and-packs.md) covers `Render` and `ToolCall`.

The Engine performs these kinds of work itself and reports each as an event: an adhoc tool call, a `GetDynamicTools` call, and a refused discovery call.

### The caller supplies the start time and a 256-bit seed

`RunParams::new` takes a `RunSeed` and a `Timestamp`, and `with_` setters add a run's other inputs. Lua reads the timestamp as `sys.when`, every timeout is a `Timer` effect, and clippy rejects a clock read in an Engine crate. The same prompt, arguments, params, and answers produce the same effects, events, and result ([Tasks and fanout](tasks-and-fanout.md)).

The seed is 32 bytes, and the Engine derives from it, with SHA-256, the nonce of the envelope that it wraps around untrusted text ([Tools and Packs](tools-and-packs.md)) and the seed of the `math.random` generator of each section VM ([The prompt language](prompt-language.md)). A 64-bit seed would cap the nonce's unpredictability at 64 bits.

### Plugin code and wire code are outside the Engine

The Engine receives each tool as a `ToolInfo`, each Pack prelude as Lua source, and each model reply as a `Completion`. `runvil-plugin`, the crate with the `Pack` trait, depends on Engine crates, and the Engine crates are independent of it. The Engine sees only a Pack's descriptors, preludes, and ids. A Pack whose tool calls the Harness performs is a catalog Pack in Engine code. The name leaves the Harness out, because Engine crates name the Harness only to assign it a responsibility.

The Harness's gateway client parses SSE streams and OpenAI bodies, because the Engine is neutral about the transport.

## 2. Parsing a prompt

### `Prompt::parse` takes a `PromptFinder` and a name, and the name identifies the prompt

`PromptFinder` is a trait whose method, `find(name)`, returns a prompt's source text. `Prompt::parse(finder, name)` calls it and parses the text, and a closure of the form `Fn(&str) -> Result<String, Error>` is a finder. `Prompt::file` and every failure name the prompt by the caller's name for it, such as `research.md`.

Prompts refer to each other by these names. A prompt's `prompts:` list names the prompts that its Lua may run with `execute`, and the parse keeps each prompt it has reached under its name, so the name is also the key that tells the parse it has a prompt already.

### `Prompt::parse` also parses every prompt that the `prompts:` lists reach

A prompt lists the prompts that its Lua may run with `execute` under `prompts:` in its front matter ([The prompt language](prompt-language.md)). `Prompt::parse` finds each listed prompt through the same finder, parses it as it parses the named prompt, and reads that prompt's own list in turn, until it has every prompt the lists reach, each once. An entry that names a prompt the parse has already reached, the named prompt included, refers to the prompt already parsed, so prompts may list each other and a prompt may list itself. The `Prompt` that the parse returns holds the others in one flat table by name, and `Prompt::linked` returns them in the order of their names. Each prompt in that table has an empty table of its own.

A listed prompt that the finder lacks, or one that fails to parse, fails the whole parse. A find failure whose location is unset is placed on the `prompts:` line of the prompt that lists the name, and a parse failure is placed in the prompt that failed. An empty entry and a repeated entry fail with the code `frontmatter_invalid`.

The parse reads every prompt so that every find is done before a run starts. A run holds all its prompts from its first step, so a prompt file that changes later leaves each started run as it was, and a missing prompt is an error when the prompt loads, before any section runs. The owner's rule: "I want to parse each of those named prompts so that we don't have to find it later". The table is flat because of the owner's design: "when we parse a prompt, we have a table. We have the initial prompt, and then we have a map for the other prompts to their parsed sections".

### `Prompt::parse` compiles every Lua block

The parser compiles each `lua` fence to Lua 5.5 bytecode and keeps the source text beside it. A syntax error in any block fails the parse with the code `lua_compile`, because a `Prompt` that parses must be executable. The bytecode exists in memory, and its docs say it "must not be persisted". `Prompt::frontmatter` returns everything the front matter declares, and the run's first step checks those declarations against the caller's params.

### One lookup finds every heading in a visible set, and a hidden heading looks the same as a missing one

A running section may name its siblings and its direct children by heading, and that list is its visible set ([The prompt language](prompt-language.md)). One lookup searches the set for `jump`, `call`, a task spawn, and a `prompt.section` read. Depending on the call, a miss is `nil` or an error. A `prompt` read also takes a path from the top, which names any section, and `require` takes only a path from the top.

A heading outside the set and a heading absent from the file get the same answer, because telling them apart would need the whole document, and a miss would then reveal what the visible-set rule hides.

## 3. The run state machine

### A caller steps a run with `Run::new`, `Run::step`, `Run::resume`, `Run::deliver`, and `Run::request_file`

`Run::new(prompt, args, params)` builds a run, and `args` is the text of the run's arguments, one JSON object. The run holds `prompt` and every prompt it links, so a caller passes the prompt that `Prompt::parse` returned. `Run::step` advances it until it needs an answer and returns a `Step`. `Run::resume(id, answer)` delivers the answer to one effect. `Run::deliver(text)` queues a message for the main chain's inbox ([The Harness and the Host API](harness-and-host.md)). `Run::request_file(call, op)` reports one file request of an open tool call, and the next `Run::step` returns the operation as a `Vfs` effect. A chain is the Engine's record of one sequence of sections: the main walk, a `call` child, an `execute` child, or a task ([Tasks and fanout](tasks-and-fanout.md)). `Run` is `Send` and owns its prompt through an `Arc`.

The caller answers effects one at a time, in any order. Parallel work in a fanout depends on that: the caller resumes one arm while the effects of the other arms are in flight.

### A tool call stays open while its tool asks for files

The `ToolCall` effect stays open until the caller answers it with the tool's result or the run withdraws it. The Engine keeps the file access of the call, so a request is a tool's one way to a file. While the call is open, the tool asks its caller for each file operation it needs, and the caller reports each request with `Run::request_file(call, op)`, where `call` is the id of the `ToolCall` effect. The Engine issues the operation as a `Vfs` effect whose asker is that call, under the task that made the call and with the call's own file access. The Engine ignores a request that names an effect the run has withdrawn, and whose answer the caller still owes, whatever kind that effect is: the Engine forgets the kind of an effect that it withdraws. The Engine ends the run with an `internal` error, as it does for a bad answer, when a request names an id unknown to the run or the id of an effect already answered. It does the same when a request names an effect of another kind than a tool call that the run still waits on. A run whose result is decided keeps that result, and a run that has returned `Step::Done` ignores every request.

The request goes through the Engine so that the run's record holds every file operation of a tool, in order with the rest of the run. The owner's rule: "every VFS operation that's performed by a tool has to turn into an effect in the engine." The call stays open because the tool's result is still to come: the file operation is a step of the call, and the call's one answer is that result.

### `Run::new` always succeeds, and the first `step` checks that the run can start

The first `Run::step` makes the start checks, in this order. The prompt and every prompt it links declare `runvil: 0`. The arguments are one JSON object that matches the prompt's `args:` declaration ([The prompt language](prompt-language.md)): `Run::new` reads the text and keeps a mismatch for this check, which returns it with the code `args_invalid` and its location unset. The params meet what each of those prompts requires: each Pack under a `uses:` is `Usable` in the roster, when the caller passed one to `RunParams::with_packs`, and the model in each role that a prompt declares meets that prompt's requirements for the role. The step skips the model checks for an empty role map, and it counts as gaps a role map that has entries and lacks `default`, and a Pack id that the Engine reserves, such as `adhoc`, in the caller's tool catalog or roster. A requirements failure has the code `requirements_unmet`, and its message names every gap.

`Run::new` always returns a `Run`, so every failure of a run, a failed start check included, reaches the caller in one place: the `result` of `Step::Done`. The caller handles a failed run there already, and a failure out of `Run::new` would need a second path that records and reports the same thing. A failure inside `Run::step` or `Run::resume` reaches the caller there too. The owner's reason: "having one place for the error is better than two".

The checks cover the linked prompts because a run may execute any of them. The owner's reason: "we want to know up front if the prompt can run or not". Within each kind of gap, the message lists the gaps of the run's own prompt first and then those of each linked prompt in the order of their names. A line about a linked prompt's gap starts with that prompt's name, as in `- review.md: missing required Pack: web`, and a Pack that more than one prompt declares has one line. The failure's location is in the first prompt that caused a gap, on the `uses:` key for a Pack and on the `models:` key for a model.

### A step returns its effects and its events as values

The fields of `Step::Pending` are `effects`, `events`, `withdrawn`, and `waiting_for_input`. Each element of `effects` is an `IssuedEffect` with an `id`, a `provenance`, and the `effect`. `events` lists what the run reported since the previous step. An `EffectId` may differ between runs. The `provenance`, a task and a position within it, is the same in every run with the same inputs and answers.

Because a step returns effects and events as values, the Engine is pure, the Harness receives one ordered stream, and a test asserts on a vector. The caller receives events one step at a time, and a step runs at the speed of its Lua.

### `Step`, `Effect`, `EffectAnswer`, `VfsAsker`, `VfsOp`, and `VfsOutcome` are closed enums

These enums omit the `#[non_exhaustive]` attribute. A `match` that names every variant of one of them is complete as it stands, and a variant that the Engine adds fails the build at each such `match`.

The Harness matches `Step`, `Effect`, and `EffectAnswer` values. With `#[non_exhaustive]` it would need wildcard arms, and it would still compile after the Engine gains a variant.

`VfsAsker`, `VfsOp`, and `VfsOutcome` are closed for the same reason. The asker of a `Vfs` effect settles the answer that the effect takes, so a caller has a rule for each asker. Each place with a rule for every operation or every outcome, such as the code that performs a file operation, records it, or makes its receipt, names every variant, so a variant that the Engine adds fails the build there. With `#[non_exhaustive]`, each such place would need a wildcard arm, and the record of a run would leave out an operation or an outcome that is new to the record code. The owner's words on the attribute: "I would expect that if they are extended, then all call sites should break, no?" The owner's rule: "also VfsAsker, and VfsOp I think should fail to build if a new thing is added and call sites are not updated." And on `VfsOutcome`: "Yeah close VfsOutcome too."

`VfsReceipt` and `VfsError` keep the attribute. Only the crate that defines `VfsReceipt` matches on it, and the attribute asks a wildcard arm of other crates alone. The owner wants an error type that can grow and leave every build intact.

### Every effect gets exactly one answer, and `Dropped` is an answer

`EffectAnswer` has one variant for each effect kind, a second one for the `Vfs` kind, and `Dropped`. A `Vfs` effect takes `Vfs` when the prompt's Lua asked for it and `VfsReceipt` when a tool call did. A caller that abandons an effect answers it `Dropped`, and the chain that waited resumes with the canceled error. The chain that made a tool call waits on the call itself, so `Dropped` for one of the tool's file requests leaves that chain waiting, and the tool call still needs its own answer. `Run::step` returns `Step::Done` once the caller has answered every issued effect, so a file operation still in progress finishes inside the run's file scope.

These caller mistakes end the run with an `internal` error: an answer of the wrong kind, a `Vfs` answer of the other form, an answer for an unknown id, and a second answer for one effect. The Engine discards an answer for an effect whose chain stopped waiting.

### A step lists the effects it withdraws

`withdrawn` lists the ids of effects whose answers the run will discard: an effect of a canceled or abandoned task, a `models.loop` round that a delivered message replaces, and every outstanding effect in the step that decides the run's result. The caller may skip or abort each one, and it still owes each the answer `Dropped`.

The step names the withdrawn effects in a list, because a shared cancel flag inside each effect would stop effects from being plain recorded data and would hide the withdrawal from logs and tests. A flag would also leave a running call running. The list covers every kind of effect, so the caller can abort a model round of a canceled task and save the cost of a reply that the run would discard.

### The caller cancels a run through the `CancelHandle` in its params

A caller passes a `CancelHandle` to `RunParams::with_cancel` and sets the flag, from any thread, to cancel the run. The Engine polls the flag before each step of a chain, and a hook in every Lua VM polls it too. The next `Run::step` ends every chain and lists every outstanding effect in `withdrawn`, and the run ends with the code `canceled` once the caller has answered them.

The polls follow one rule: the Engine honors a cancel while a block runs and while a chain waits on a request.

### A run ends with its final text or with one `Error`

The fields of `Step::Done` are `result`, `events`, and `unread`. `result` is a `Result<String, Error>`: `Ok` is the run's final text, a canceled run ends with the error whose code is `canceled`, and any other error is a failure. `unread` lists the messages still in the main chain's inbox.

A prompt declines its task by returning its reason from Lua, so the run completes and `result` is `Ok` with that text. A prompt that fails a check of its own, such as an `assert` on the range of an argument, raises a Lua error, and the run ends with the code `lua_runtime`, whichever section raised it ([The prompt language](prompt-language.md)). The code `requirements_unmet` belongs to the start checks alone. The code `args_invalid` belongs to the start check of the run's arguments and to an `execute` whose arguments the executed prompt's declaration refuses.

A prompt has the contract of a tool: it takes one JSON object and returns text. A tool reports a failure inside its result, as text with an error flag. The text is the only place where a tool can say that it declined, so a decline is ordinary result text. The run's `Ok` and `Err` are that flag, and the Engine sets `Err` only for a fault that it can identify itself, such as an unmet requirement. MCP states the rule for a tool in its [CallToolResult](https://modelcontextprotocol.io/specification/2026-07-28/schema#calltoolresult) schema: "Any errors that originate from the tool SHOULD be reported inside the result object, with `isError` set to true, not as an MCP protocol-level error response." The owner's reason: "I don't want to innovate here." The owner's goal for a prompt's input and output contracts: "as standard as possible, so they can fit into the existing LLM ecosystem".

### A run where every chain waits ends as `stalled` or as `internal`

A run stalls when every chain that remains waits, all its awaited effects have their answers, and its main chain waits on something other than its inbox. The Engine then ends it. When tasks wait on their inboxes, the error has the code `stalled` and names them. Otherwise the error is `internal`, because only an Engine fault produces that state.

A main chain that waits on its inbox is a different case: `Run::step` returns `Step::Pending` with `waiting_for_input` set, and the caller continues with `Run::deliver` or a cancel. The case differs because the main chain's inbox is the only one that code outside the Engine can fill.

## 4. Effects and their records

### `Effect::Chat` contains the whole request

The fields of a `Chat` effect are the model `binding`, the `messages` in wire order, the `tools` schemas, the `round`, and `after`. The messages are the full request, so the caller can perform the round from the effect. `after` names the earlier round whose request this one extends, with the number of leading messages the two share, and the effect's record omits those messages.

The request settings are in the binding only: a second copy in the effect could differ from the binding, and the effect's record would then describe a request that differs from the one sent.

### The Engine numbers the model rounds of a run from 0

One counter numbers every model round of a run in issue order, and loop rounds and nested `models.infer` rounds share it. Each `Effect::Chat` has a `Round` with that `RoundId` and an origin, `Chat` or `Infer`. The Engine writes the same id in the round's thinking, reply, and tool-call events, so a caller that streams partial output matches it to the finished events.

A `Round` holds only the id and the origin, because a caller needs only a round's id and whether the round is user-facing.

### `Effect::Vfs` is one file operation, and it says who asked

The Engine issues a `Vfs` effect for each `store.*` call, and for each file request of an open tool call that the caller reports with `Run::request_file`. Its fields are `asker`, `access`, and `op`, a `VfsOp` with the operation's arguments. `asker` is a `VfsAsker`: `Lua` for a `store` call, with the id of the store that the call works on, and `Tool` for a tool's request, with the effect id of the tool call. The asker settles the files. A `store` call works on its chain's store, and `access` is the calling chain's view of that store. A tool's request works on the file system the caller gave the run, and `access` is the call's own file access. A caller performs the operation with `Access::perform`, or with `perform_vfs_op(&access, &op)`, a synchronous function that calls it ([Files and the VFS](files-and-the-vfs.md)).

A caller answers a `store` call with `EffectAnswer::Vfs`, the whole `VfsOutcome`, and the `store` call resumes with it. It answers a tool's request with `EffectAnswer::VfsReceipt`, once it has handed the whole outcome to the tool. A `VfsReceipt` is a stat, a bool, a count of entries or paths, a count of bytes, or done, and `VfsReceipt::of` makes one from an outcome. The Engine gets the receipt alone, so the bytes of a read stay with the caller and the tool. The chain goes on waiting through the answer to a tool's request, and a failure in it goes to the tool and leaves the run alone, because the chain waits on the tool call and the tool decides what a failed operation means. The view of a tool's file operation is its effect record and its answer record alone. A `store` call keeps its events.

A run has more than one store when a prompt executes another, so the effect of a `store` call names its store. The id is a `ChainId`: `0`, the id of the root chain, for the store that a run starts with, and the id of an executed prompt's chain, such as `0.0`, for a store that an `execute` made. The view alone would be enough to perform the operation, and the id is there for the record.

Lua says `store`, and the names below the Lua layer say `Vfs`, because every one of these operations is a VFS operation: one made through a view of a store, or one made through the access of a tool call.

### `Effect::record` and `EffectAnswer::record` return JSON for the run log

`Effect::record` returns the effect as a `serde_json::Value`, minus its live handles. A `Timer` record stores seconds, though the effect has a `Duration`. A `Vfs` record omits the access and keeps who asked beside the operation: the id of the store for a `store` call, as in `{"Vfs":{"store":"0","op":{...}}}`, and the effect id of the tool call for a tool's request, as in `{"Vfs":{"call":7,"op":{...}}}`. A write or an append records its bytes as text when they are UTF-8 and as hex otherwise.

`EffectAnswer::record` returns the answer's outcome, and a failure is an object with a `kind`, the tag of its `ErrorCode` or `VfsError`, and a `message`. A `Vfs` answer records the whole outcome, because a replay resumes Lua with it. A `VfsReceipt` answer records the receipt: a size in place of content, the number of bytes of a read or the number of entries of a listing, and the stat of a stat. The tool's own result is in the record as the answer of its `ToolCall`. A receipt describes content by its size alone.

## 5. Inside a step: chains, section VMs, and requests

### Each section entry gets a fresh Lua VM, and one code path builds every entry

The main walk is the run's one root chain, and it starts at the first top-level section, which is the preamble when the prompt has one ([The prompt language](prompt-language.md)). An `execute` child starts the same way in the prompt it runs. Each time a chain enters a section, the Engine builds a fresh sandboxed Lua VM, which runs every block of the section. The next section's VM starts with the Engine globals and the chain's `var`.

The rule is one VM for each section entry, whichever way the chain entered the section: by falling through from the section before, by `jump`, or by `call`. One VM shared across sections would create hidden globals that depend on the path the run took, uncloneable state, and inconsistent section inputs.

One function sets up every entry, the preamble's included. The owner's reason: "What's most important is the code in the engine, that it's uniform." A second function for the start of a prompt would repeat most of the first. The owner's rule: "I don't want to have two different functions that do similar things for the whole section, and then only encode the differences for the H1."

### Section VM setup runs in one fixed order, with the limits first

The Engine creates the VM and applies the run's memory and log limits. It then installs the rest in this order:

1. `args`, `sys`, and `var`.
2. The Engine globals, such as `log` and `json`.
3. `jump` and the `prompt` reads.
4. The calls that wait by yielding, `models.loop`, `call`, `execute`, and the `store` functions among them.
5. The prelude of each Pack that the section's prompt declares.
6. `require`.

The `store` functions wait by yielding, so they are installed with the other calls that yield, before the preludes. A prelude runs on the main Lua state, outside any coroutine, where a yield fails. A prelude that calls the store or a tool while it installs therefore fails to load, and the run ends with the code `lua_runtime` before it issues any effect. A function that the prelude defines may call the store or a tool when a block calls that function.

The `store` functions are in place before the preludes on the owner's instruction: "refuse store calls during a prelude's install". The call fails the way a tool call there fails, on the same code path, and the run's record holds every store operation of the run. The limits apply before any install, so each prelude runs under the run's own limits.

### A Lua call that waits yields a request from a coroutine

The Engine starts every Lua block as a coroutine. A call that waits, such as `models.infer`, is a short Lua function that yields a table. The Engine parses the table into a typed request, and answers it at once, or issues an effect and records what the chain waits on. Later it resumes the coroutine with an `(ok, result)` pair. A yield from author code fails the block with the message "scripts may not yield directly". The functions that yield are written in Lua, because a yield across the C boundary fails.

Each block is its own coroutine because the loop over a section's blocks is Rust. The scheduler starts a block, and when the block's coroutine ends, it decides what follows: the section's next block, the end of the chain, or a jump. A coroutine ends when its function returns, so a loop in Rust needs one coroutine for each block. A loop in Lua could run a whole section in one coroutine, and `require` runs a library's blocks that way, because it starts inside a running block, where a call from a Rust function to Lua that yields fails. The section loop stays in Rust. The owner's rule: "key rules we keep in Rust". The owner's view of the Lua loop: "I don't see the point of putting it in Lua". One coroutine for each block was not a choice between the two forms. The owner's words: "It just kind of happened that way."

One thread runs a whole run. A run needs only one, because every Lua block executes fast.

### The Engine uses mlua's synchronous API on Lua 5.5

The Engine runs Lua 5.5 through the `mlua` crate and calls its synchronous API. The `async` feature of mlua is off, permanently and unconditionally.

Runvil's needs are modest, and for them a small request protocol that the project owns beats a general-purpose mechanism that the project would depend on and that is outside its control. The owner's rule: "I never want async mlua."

### The requests a block can make are one closed set, and every answer variant is a success

Every call that suspends a block is one variant of a single request enum, with typed fields that the parse checks. A leaf request, such as a `models.infer` or a `store` call, becomes an effect. A structural request, such as a `call`, an `execute`, or a task spawn, changes which chains the Engine runs. The enum is closed because it is the audit surface: one file lists everything a script can cause.

The Engine resumes a block with a success or a failure. Every answer variant is a success, and a failure stays outside the variants, because the Lua side reads the variant of an answer only when the answer is a success.

### The rules of `models.loop` run in Rust, and a Lua trampoline performs each action

`models.loop` is a short Lua function over a trampoline. The trampoline calls a Rust step function, performs the one action the step returns, and calls the step again with the outcome. The actions are: yield a request, call a tool handler, call the compactor, raise an error, and return. The Rust side is a state machine.

The trampoline is Lua because a Rust function can yield only with mlua's `async` feature, and its call to a Lua function that yields fails. In the wider division, Lua in the prompt decides policy, such as compaction, and Rust runs the mechanics ([Model calls and conversations](models-and-conversations.md)).

The loop's rules are in Rust because the loop works on the message list, and the list is a Rust object. A model's reply reaches the step function as a value that is opaque to the trampoline, which passes it along, and the step appends the records itself, so the Engine can keep data in the list that is hidden from Lua. The owner's reason: "we need to be able to manipulate the messages object. The messages object is in Rust". The owner expects to need this for requests to OpenAI endpoints and to Anthropic endpoints, which is planned work, and says of what the Engine then does with the list: "Lua can't see it." The Rust state machine also meets the owner's request that "the rules are unit-testable without a VM". The other calls with rules of their own, `tasks.fanout`, the `tasks` waits, and `require`, are Lua, and they work on values that Lua holds: task handles, results, and section code.

### A chain reads everything about its prompt from the prompt it runs

The Engine reads a chain's prompt from the chain. A section lookup, `sys.section_count`, the round cap of `models.loop`, and the file name in an error each come from the prompt that the chain runs. A `call` chain and a task keep the prompt and the store of the chain that started them.

Each prompt has a tool set and a model set of its own because both depend on its front matter. The tool set marks the Packs under that prompt's `uses:` as the declared ones, and the model set has the thinking switch that the prompt's `models:` gives each role. Each is built over the run's one catalog, roster, and role map.

### `execute` starts a `call` chain in another of the run's prompts

An `execute` has a name and optional arguments. The Engine makes its checks in this order: the depth cap, the name against the `prompts:` list of the prompt that the caller runs, and the arguments against the `args:` declaration of the named prompt. A failed check raises its error at the `execute`, before any chain starts. The chain that starts has the named prompt, the checked arguments, and a store of its own, and its parent is the caller. It starts at the first top-level section of the named prompt, with an empty `var`, on the `default` model of that prompt's model set, under the caller's file access and the caller's concurrency limit.

The chain is a `call` chain in all but the prompt it runs, so the Engine has one code path for both. The caller is parked as on a `call`, and the chain's end answers it with the chain's final text, or with nil when the chain ends with a nil result. The finish, the abort of a subtree, a cancel, the depth count, and the admission of tasks work as they do for a `call`, and the events of an `execute` are those of the sections that its chain runs. The owner's reason: "it's not a separate run. The run is somewhat integrated into the whole."

A run that a caller builds over a prompt from another prompt's `Prompt::linked` holds that one prompt. An `execute` of a second prompt fails there with the code `internal`, because a run is built over the prompt that a parse returned.

## 6. Run limits

### `RunLimits` is a struct with public fields

A caller changes a limit by assigning a field of `RunLimits::default()` and passing the struct to `RunParams::with_limits`. The fields and their defaults:

- `max_tool_iterations`: 24 model rounds in each `models.loop` call.
- `tool_iterations_ceiling`: 1000, the cap on the field above and on a prompt's own `max_tool_iterations`.
- `max_concurrency`: 8 tasks admitted at once.
- `max_idle_tasks`: 8 tasks idle after a reply.
- `lua_memory_bytes`: 64 MiB for each Lua VM.
- `lua_log_events`: 1024 `log` calls for each Lua VM.

The fields are public because `RunLimits` is a plain `Copy` value whose only invariant is in its `NonZero` types. The call depth cap is a constant of the Engine with the value 8, and a deeper `call`, `execute`, or task fails with the code `call_depth_exceeded`. Every prompt of a run counts against the run's one set of limits: a task that an executed prompt spawns takes a slot under `max_concurrency` and counts toward `max_idle_tasks`, as a task of the run's own prompt does.

The cap is 8 because that is more depth than the owner expects a prompt to need. The owner compares it with Cursor, which the owner recalls as allowing about five levels, an unchecked figure, and says of nesting there: "I've only ever gone to three." The cap is a constant, where the other limits are fields of `RunLimits`, because the one value has been enough for every Host so far. The owner's reason: "I just don't see a need for it yet." A field for it is a small change, in the owner's words: "it's trivial to make it configurable".

The default values of the limits are arbitrary, and so is the interval of the cancel hook, 10,000 instructions. The numbers are unmeasured, and the owner did not pick them. The owner's rule is that each resource has a limit, and the value is secondary. The owner's view of the values: "8 sounds good", "64 meg sounds about right", and "10,000 seems alright". A value may change freely, because, in the owner's words, "this thing's not in production yet".

### A Lua block may run any number of instructions, and a hook polls the cancel flag

Each Lua VM has a hook that fires every 10,000 instructions and reads the run's cancel flag, and a set flag aborts the block. An endless loop in a prompt therefore runs until the caller cancels the run. The owner's reason: "If the user codes an infinite loop that's their problem".

The Engine's quotas are on `log`: a count of events and a count of bytes. A call past either one raises an error with the code `lua_quota`.

## 7. The one error type

### Every Engine and Harness failure is one `Error` record with an `ErrorCode`

`Error` is a struct in `runvil-types` with the public fields `code`, `message`, `prose`, `at`, and `facts`. `ErrorCode` is an enum of codes, each with a number and a tag such as `lua_runtime` (201). The hundreds digit names the family: 1xx parse, 2xx Lua, 3xx tasks, 4xx model, 5xx tools, 6xx store, 7xx run, and 8xx caller. The enum's docs add: "A number is never reused." The root `AGENTS.md` makes `Error` the one error type of the Engine and Harness crates, with `VfsError` as the main exception.

The crates share one `Error` because a separate error type for each failure makes the code harder to read and to change, and every one that a facade exports is another public item. One record makes the code easier to read and to change, and it keeps the public surface small. The owner's reason: "There was a proliferation of error types and unifying them into one made the code much easier to understand and easier to work on". On the surface: "one is better than 52 in this case". The owner limits the rule to this API: "for unified public harness and engine API, it's better to just have one error", and of the rule elsewhere, "that might not be generally true".

Today `call_depth_exceeded` is 204, in the Lua family. The grouping and the numbers may change until Runvil has users. The owner's reason: "there's no users that are comparing error codes yet, so it's easy to change". A code that moves to another family takes a new number, so a move keeps the rule that each number names one code for good.

`Error`, `ErrorCode`, and `ErrorFact` omit the `#[non_exhaustive]` attribute, so a code that the Engine adds breaks the build of a Host whose `match` on `ErrorCode` names every variant. The closed state of these types is unplanned. The owner's rule marks every facade type `#[non_exhaustive]` except the enums that it keeps closed (see the entry on closed enums), and the error types lack the attribute. The owner's words: "it just happened", and "we should probably mark the type as non-exhaustive". Adding the attribute is planned work.

### An error's `message` includes each cause once, and its `prose` is the text a model reads

`message` is the full diagnostic for logs and people, and `Display` prints it. `Error::with_cause` appends the text of each cause and skips text the message contains. The owner's rule: "Keep the full self-contained message in Display".

`prose` is a second text for a model: what failed and what to do next. Each `ErrorCode` has a default prose. The field exists because a model may read any string that the system produces.

### One location shape describes a place in a prompt or in Rust source

`at` is an optional `SourceLoc`: a file, a start line and column, and an end line and column. The file is the prompt's name, or for an `internal` failure the Rust source file that built the error, and such a failure names the line that found the fault. The field's name, `file`, is generic, so one shape describes both kinds of place.

`Error::located_message` returns the message followed by the location, as `(at file:line)` for most codes.

### A Lua failure the author caused has the code `lua_runtime`, and a broken Engine rule has `internal`

A failure has the code `lua_runtime` when the prompt author caused it and can fix it in Lua, such as an empty `messages` list. It has the code `internal` when an Engine rule broke, such as a poisoned lock.

The code names the party at fault, and a failure with the wrong one of these codes would blame the wrong party.

## 8. The public surface and the internal crates

### `runvil-engine` contains re-exports, and each item has one path

The crate consists of `pub use` lines, at the root and in `pub mod` blocks, and private crates define every item. The code calls such a crate a facade. Each line re-exports one item by name from the crate that defines it. A structural check rejects a glob, a list, and an item definition, and `cargo xtask api` rejects a re-exported module. The `runvil` crate, which a Host depends on, is a facade too ([The Harness and the Host API](harness-and-host.md)).

A caller finds every item in this one crate. The owner's reason: "I dont want types scattered around different crates. I have discovered it becomes hard to browse the cargo doc that way." Each line names a single item because a re-exported module or crate would expose everything that the module later gains, and a second path to an item would make the API ambiguous.

### The facade re-exports an item that non-test code outside the Engine uses

The facade re-exports an item when production code of a crate outside the Engine uses it. A use in a test, a bench, or a doc example is outside the rule, and the author rewrites the test. Every change to the surface appears as a difference in the facade's checked-in API listing. So the Engine builds every `ToolSchema`: a public constructor would widen the facade only for tests.

The goal is a minimal surface in which every item is necessary. The owner's rule: "I want ZERO public API surface growth just for tests."

### A signature on the surface names only facade items, the standard library, and the serde crates

`cargo xtask api` reads rustdoc's JSON for the facade and fails when a signature, field, or impl on the surface names anything else. The serde crates are `serde`, `serde_core`, and `serde_json`.

The limit applies to the surface only: the internal crates may use many external crates, and the surface is restricted to basic ones.

### An operation reserved for Engine crates is a free function in a module outside the facade

Some operations on a public type are reserved for Engine crates, such as building a `ToolSchema` or reading a prompt's sections. Each is a free function in a module of the crate that defines the type, and the facade omits that module.

The operations are free functions because a free function in a module outside the facade is invisible on the type's docs page. A hidden trait implemented on a public type can render there as an impl that links into an internal crate.

### The Engine is a set of private crates, each with one job

Code outside the Engine's private crates imports them through `runvil-engine` and `runvil-plugin`. The crates are:

- `runvil-types` defines ids, events, metrics, `Error`, the cancel flag, and the untrusted envelope.
- `runvil-vfs` is the virtual file system.
- `runvil-model-client` defines messages, tool schemas, completions, and model bindings.
- `runvil-markdown` reads Markdown text into blocks, writes blocks back as text, and defines the grammars that name a section and a block.
- `runvil-lua` runs the Lua sandbox and defines the Engine globals and the request protocol.
- `runvil-prompt` parses a prompt and compiles its Lua, and uses `runvil-lua` for the compiled form of a block.
- `runvil-runtime` contains `Run`, the scheduler, and the effects.

The owner's goal: "I want as many internal crates as possible and I want maximum internal structure." A crate boundary is what stops `runvil-lua`, for example, from using the scheduler's internals.

## 9. Planned work

- **An `execute` that returns at once.** A Lua `execute_async` call that starts another prompt as a task and returns its handle. Today `execute` runs a listed prompt as a blocking child chain, and its caller waits for the result.
- **Replay.** Harness code that runs a recorded run a second time and compares the records of the two runs. Today the Engine attaches a `Provenance` to each effect and event, and `Effect::record` and `EffectAnswer::record` return the JSON that a run log stores.
- **The round id on `ModelTurnFailed`.** A round id in the `ModelTurnFailed` event, so that a Host can drop exactly the failed round's streamed pieces. Today the Engine writes the round id in a round's thinking, reply, and tool-call events.
- **Bounds on JSON conversion.** A size budget at every site that converts a Lua table. Today `json.encode` and `var` writes have a size budget, other sites in the Lua crate convert a table of any size, and one encode runs as a single Rust callback between cancel polls.
- **`#[non_exhaustive]` on the error types.** The attribute on `ErrorCode`, so that a new code leaves a Host's build intact. `Error` and `ErrorFact` may get it too. Today these types are closed, and a `match` in `runvil-lua` names every code and is complete as it stands, so it would need a catch-all arm or a move into `runvil-types`.
