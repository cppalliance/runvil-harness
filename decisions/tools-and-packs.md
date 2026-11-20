# Tools and Packs: design decisions

This page covers tools, Packs, Plugins, the Plugin C contract, and the Packs that ship with Runvil. A Plugin installs Packs, a Pack installs tools, and a section's Lua offers tools to its model. The Engine asks for each tool call as an effect, and the Harness runs it.

## 1. Plugins, Packs, and names

### "Pack" names the set of tools, and "Plugin" names the entry point that installs it

A Pack installs tools, and a Rust Pack can also set a Lua prelude. A Plugin is an entry point that installs Packs. A Plugin that follows the C contract reports a name and a version, which the Host lists. A Rust `install` function inserts its Packs and reports only success or an error.

"Plugin" is the usual word for what a loadable crate or library provides, and the names of the Plugin crates, such as `plugin-std` and `plugin-web`, fit it.

### A Pack id is the second segment of the Pack's name

A Pack's fully qualified name has two segments, a vendor and a name, as in `runvil/web`. The name segment is the Pack id, and every tool id in the Pack starts with it, as in `web/fetch`.

### A registry holds one Pack for each Pack id

`PackRegistry` refuses a Pack whose id it already holds, whatever the vendor of either Pack. The first Pack that a Plugin inserts keeps the id, and the second insert fails with the reason `pack_id_taken` and a message that names the holder, as in ``Pack `acme/web` cannot take the Pack id web, which `runvil/web` holds``. A Pack that a Plugin inserted as unavailable holds its id too. The owner's rule: "runvil/web and acme/web cannot both be in the host's PackRegistry".

Every Host has the built-in Packs `subagents` and `user`, which the Harness makes itself, so a registry starts empty and holds only what Plugins inserted. The registry still keeps the id of each built-in: it refuses a Pack that takes one of them with the same reason, `pack_id_taken`, and the message names the built-in, as in ``Pack `acme/user` cannot take the Pack id user, which the built-in Pack `runvil/user` holds``. The Harness makes the built-ins because no Plugin delivers them and no Host can leave them out.

The rule follows from the tool id. A tool id starts with the Pack id and leaves the vendor out, and the Harness finds the Pack for a tool call from that first segment. With one Pack for each id, `web/fetch` names one tool on a Host, and a prompt reaches every Pack of a run by its bare Pack id.

### Lua uses a tool's id, and the model uses its wire name

Lua code uses the id, such as `web/search`. The model sees the wire name, which by default is the id with `_` in place of each `/` and `.`. A Pack may set a different wire name, and the model sees `std/read` as `Read`.

The tool objects that Lua reads omit wire names, because the Engine may rename them, and prompt prose that names them would then be stale.

### A bare name under `uses:` means the `runvil` vendor

`uses: [web]` means `runvil/web`. A prompt writes another vendor's Pack with both segments, as `acme/web`. The bare name is the one thing that sets the `runvil` vendor apart: a Pack of any vendor is in every run of its Host, and Lua reaches each one by its Pack id.

Under this rule each name has one meaning. If a bare name could mean a third-party Pack, a `runvil` Pack of the same name that ships later would silently take its place in an existing prompt.

An entry must name the vendor that the Host installed the Pack from. On a Host that has `acme/poop`, the entry `poop` is refused with `poop means runvil/poop, which is not installed; write acme/poop to use that Pack`, and on a Host that has `runvil/web`, the entry `acme/web` is refused with `acme/web is not installed`. The Harness leaves the installed Pack of that id out of such a run, because the run's roster lists the id as unavailable, and the run's first step refuses the run.

The Engine reserves the Pack ids `adhoc` and `dynamic` for its own tools. A tool that a prompt defines in Lua gets an id under `adhoc`, and the two discovery tools get ids under `dynamic`.

### A Pack is for a feature that works on something outside the run

The Engine provides `store`, `var`, `log`, and `models` in every section. A Pack provides a feature that works on something outside the run, such as the web or an MCP server.

The test for a feature is whether it works on something outside the run. The store is part of the language because it is how a prompt moves data between sections and handles fanout. A Pack for budget, compaction, request shaping, or model routing would duplicate work that the Engine, the prompt's Lua, the Gateway, or the Harness owns, in that order.

## 2. Requiring Packs with `uses:`

### A `uses:` entry requires a Pack and runs its prelude

A prompt names Packs under `uses:` in its front matter. For each entry, `Host::parse` checks that the Host has the Pack and refuses the prompt when the Pack is missing. The Engine then runs the Pack's Lua prelude in every section of the prompt. A Rust Pack sets its prelude in its `PackInfo`. Contract 0.6 gives a Pack a name and a description, so a prelude for a library Pack is still to be built, and each shipped Pack leaves its prelude unset.

A prelude runs only for a declared Pack because it defines Lua globals, and so decides what a prompt's Lua can call. The Lua globals of a prompt therefore depend on its own `uses:` list and are the same on every Host.

Every entry is required. A prompt that treats a Pack as optional tests `tools.packs.get('name')`, which returns the Pack object when the run has the Pack.

### A run includes every Pack the Host installed, and a section opts in to the undeclared ones

A run includes every Pack in the Host's registry and the built-in Packs, whatever the Pack's vendor and whatever the prompt declares. A `uses:` entry marks a Pack as required, so that the run is refused when the Pack is missing or unavailable, and it runs the Pack's prelude. It leaves the run's Packs as they are, except that an entry that names the wrong vendor keeps the installed Pack of that id out of its run, which the first step refuses (see the entry on a bare name). The owner's rule: "Packs are determined by the host, not the prompts. The frontmatter just indicates which packs are *required*."

The guide calls the undeclared Packs extras. A section offers them with `tools.offer(tools.packs.extras())`, as the built-in chat prompt does. The owner's rule: "extras should be a per-section setting. Disabled by default, and opt-in".

The Host decides because it is the party that installs Plugins. With this rule the Packs and tools that a prompt can reach on one Host are the same for every prompt, and a prompt's run holds the same Packs whatever the front matter of another prompt says.

### A linked prompt's `uses:` requires its Packs for the run and runs their preludes in its own sections

A prompt may list other prompts under `prompts:`, and a run of it holds each of them for `execute` ([The prompt language](prompt-language.md)). `Host::parse` and the run's first step read the `uses:` of every one of those prompts. A Pack that a linked prompt declares is required for the run, so the parse refuses the prompt, or the first step refuses the run, when the Host lacks the Pack or the Pack is unavailable (see the entry on the three stages of a refusal). Each refusal names the linked prompt. The parse writes each one in the form `- review.md: web is unavailable: runvil/web is not installed`, and the first step writes the same form with its own reason, or `- review.md: missing required Pack: web` for a bare entry whose Pack id is absent from the registry. A Pack that two prompts declare counts once.

A linked prompt's `uses:` leaves the rest of the run as it is. The run still has every Pack in the Host's registry, one instance of each, and a tool of any Pack answers a call from any of the run's prompts. In each section, `tools.packs` marks as required the Packs under the `uses:` of that section's own prompt, and `tools.packs.extras()` lists every other Pack of the run. A caller's Packs are therefore the same before and after it executes a prompt.

A Pack's prelude runs in the sections of each prompt that declares the Pack, and in those sections alone. The owner's rule: "A prompt's sections should only have preludes injected for the packs it names. It should not inject preludes for packs named by other prompts in prompts: frontmatter". The Lua globals of a prompt therefore depend on its own `uses:` list, whichever prompt executes it, and two Packs whose preludes define one global collide only in a prompt that declares both.

A prompt's preludes run in the order of its `uses:`. A prelude that fails to load fails the section entry that installs it. For an executed prompt, that entry is its first section, so the failure is the error that the `execute` raises.

### A declared Pack can be refused at install, at parse, and at the run's first step

1. At install. A Rust Plugin whose setup fails inserts its Pack as unavailable, with a reason. A Rust Plugin whose function returns an error keeps each Pack that it inserted before the error, and the error is the one failure of that Plugin's report, which the `Host` keeps. The loader drops every Pack of a library Plugin whose load fails, and `Host::parse` then reports each one with the text `is not installed`.
2. At parse. `Host::parse` refuses a prompt when it, or a prompt it links, declares a Pack that the Host lacks or one that a Plugin inserted as unavailable.
3. At the run's first step. A factory that fails or panics, or a `ready` that returns an error, makes the Pack unavailable to that one run. The Harness also marks `runvil/user` unavailable in a run that lacks a user.

The third case exists because a factory's result is unknown at parse time. The next run calls the factory again.

### The Harness reads each Pack's tools once, when the run starts

The Harness makes the run's instance of each installed Pack, waits for each `ready`, and reads each tool list. The run uses that catalog until it ends. A run waits for every Pack, declared or undeclared, because the tools of an undeclared Pack are in its catalog too. `Pack::ready` resolves within a bound that the Pack owns, so a Pack that cannot start fails its `ready` within that bound, and the Pack is unavailable to that run. An error from `ready` is temporary. A Pack that recovers is available to the runs that start afterward.

### Lua reads one catalog for the whole run, and the Engine builds the model's tool list once for each section entry

The run's catalog lists every Pack tool's `ToolInfo` (its id, wire name, description, parameter schema, and flags) by tool id. `tools.get`, `tools.call`, and `tools.offer` look ids up in it. The Harness builds it before any Lua runs, so each lookup returns the same answer for the whole run. For each section entry, which is one execution of a section, the Engine builds what a model sees: the wire names, the Static Set, and the Dynamic Set.

The Harness fixes the catalog in advance so that the Engine is deterministic and leaves all I/O to its caller. One catalog covers the whole run because the catalog is the same for every model.

## 3. Offering tools to a model

### A section's Lua offers tools to its model

`tools.offer("web")` offers both tools of the `web` Pack. `tools.offer("web/fetch")` offers one tool. The model's tool list for the section contains the offered tools.

Each entry into a section starts a fresh Lua VM with an empty offer list. A prompt that wants the same offers in several sections puts them in one section and loads that section with `require`.

Offers are per section for isolation: a section's model sees only the tools that the section offered. The rule also stops a prompt with 20 tools from sending 20 schemas to the model of every section.

### `tools.offer` takes tools and Packs, as ids or as objects

Its single argument is a tool, a Pack, or an array of both, and each item is an id string or an object. A string that contains a slash is a tool id, and any other string is a Pack id. The owner's rule: "I think there should only be one api to offer tools".

### The first `models.loop` round builds the tool list, and later rounds reuse it

Each `tools.offer` call checks its argument and records the offer. The section's first `models.loop` round builds the tool list, and every later round of that section sends the same bytes. A call to `tools.offer` after that round raises an error that says "the section's tool list is fixed at its first models.loop round".

The reason is the provider's prompt cache. A tool list rebuilt every round can alter the request prefix in the middle of a conversation, and the provider then discards its cache.

### `tools.offer_dynamic` builds a second set, which the model uses through two discovery tools

The Static Set is the list of tools offered with `tools.offer`, and every round sends it. The Dynamic Set is the list of tools offered with `tools.offer_dynamic`. When the Dynamic Set has members, the Engine appends two tools of its own to the Static Set. `GetDynamicTools` lists and searches the Dynamic Set, and `CallDynamicTool` calls one of its tools.

The names and parameters copy Cursor's pair, and the search pattern is a regex because models that know Cursor send regexes. The Engine answers a mistake in a discovery call with text that tells the model what to do next. The `mcp` Pack has a pair of its own.

### Offer order sets list order, and a shared wire name fails the round

Tools appear in the list in the order of the offers. The tools of one Pack form one block, at the place of the section's first offer from that Pack, and an adhoc tool keeps its own place. The position of each offer line decides, because a hash order would vary from process to process, which would break prompt-cache hits, replay, and tests.

Two tools that share a wire name across the two sets fail the section's model call, with an error that names both. The exception is an adhoc tool: the Engine drops a Pack tool that shares its wire name in the same set. The clash fails the call so that a Pack bug or a name clash shows up as a clear error.

### `Pack::render` lets a Pack shape its tools for each model

At the first loop round, the Harness calls `render` on each Pack the section offered from. The Pack receives the offered tools and the round's model, and returns the tools the model will see. It may rename a tool, rewrite its description and schema, drop it, or add model-only tools. Each shipped Pack uses the default, which returns the tools unchanged. Contract 0.6 lacks a render verb, so the loader gives each library Pack the same default, and a render for a library Pack is still to be built.

One hook covers every Pack, so the Engine treats all tools the same way. The Engine requests the render as an effect: the Harness performs it, so the Engine is still pure, and the run log records it.

### Prompt prose describes tools by what they do

The model sees wire names, and a render can alter them. Prose that tells the model to call `web/search` leads to a failed call when the tool list names the tool `web_search`. The shipped research prompt therefore describes each tool by its job, and its prose is true under any render.

## 4. Tools written in Lua

### `tools.adhoc` defines a tool in Lua, and `tools.offer` offers it

`tools.adhoc(attrs, f)` takes a name, a description, optional parameters, and a function, and returns a tool object. The tool's wire name is the name as written, and its id is `adhoc/<name>` with the name in lower case. The prompt passes that object to `tools.offer`, or calls the tool itself with `tools.call`.
The parameters are a JSON Schema object, or a short form: a table that maps each parameter's name to its type, or to an array of its type and its description.
A name in the short form is a string, and a number in its place raises an error.
The type and the description are strings that the Engine reads from the table itself, so an `__index` metamethod of an array in the short form stays uncalled.
The names, the types, and the descriptions of one short form may add up to as many bytes as the VM's memory limit, which is 64 MiB by default, and a description that several parameters share counts once for each.
One short form may have as many parameters as one eighth of that limit, which is 8,388,608 at the default, and each view that the Engine reads through to reach a parameter's value counts as six parameters.
A short form past either limit raises an error that reads `tools.adhoc: parameters are too large to convert`, whatever else is wrong with it.

Defining a tool is separate from offering it, so every tool has one offer path, and a prompt can define a helper tool that only Lua calls.
A number is refused as a parameter's name because Lua converts it to a string on request, so the keys `1` and `'1'` of one table would name one parameter with two types.
The Engine reads the parameters in the order of Lua's own walk, which differs between two runs with the same inputs, and a metamethod that ran at each read could act on that order.
The schema holds a copy of each description outside the VM's memory, so without the limits a small table could make the Engine hold many times the memory that the run allows.
The conversion is one step that a cancel does not interrupt, so the limit on views also bounds how long that step can take.

### An adhoc handler runs inside the Engine, as section Lua

The handler runs in the coroutine of the Lua block that called it, and the Engine answers the call itself. A `tools.call`, a `store` call, or a `models.infer` inside a handler is an ordinary effect, and the Engine counts it as a script call.

A handler that Rust called outside any coroutine would fail at every `store` call. Inside the block's coroutine, the handler's calls yield the same way as the block's own calls, and a chain needs only one coroutine.

### A failing adhoc handler raises at `models.loop`

When a handler raises an error, the Engine raises the same value at the place the tool was called. For a call the model made, that place is `models.loop`, so the loop ends unless the prompt wraps it in `pcall`. The Engine raises the value unaltered so that a structured error table still has its `kind`. The Engine sends a successful result to the model as written, marked trusted, because the prompt author wrote the handler. The exception is a handler whose own `tools.call` read an untrusted tool listing: the Engine wraps that handler's result and marks it untrusted. Of the shipped tools, `mcp/get_dynamic_tools` is the one flagged `describes_tools`, and the `mcp` Pack marks a script's listing trusted (see Trust of tool output). A handler that returns that listing therefore sends the servers' text to the model unwrapped, and the author wraps the text with `untrusted`.

The Engine raises because a handler is the author's own Lua, so its failure is a bug in the prompt. A prompt that wants the model to read a handler's failure calls `pcall` inside the handler and returns the text.

### An adhoc handler's first return value is the tool's result, and a table fails the call

The Engine uses the handler's first return value as the tool's result: `nil` becomes the empty string, and a string, a number, or a boolean becomes its text. A table fails the call with the error "cannot return a table as a result".

A block's `return` turns its value into text the same way and refuses a table with the same error. The two differ for `nil`: a handler's `nil` is the empty string, and a block's `return` of `nil` ends its chain with a nil result ([The prompt language](prompt-language.md)).

### `jump` raises inside an adhoc handler

A prompt calls `jump` from the block, after the tool call returns. Inside a handler `jump` raises an error.

With a jump inside a handler, `models.loop` would need rules for reporting a batch that the jump abandons partway.

### An adhoc handler makes a model call with `models.infer`, and a `models.loop` there raises

A handler uses `models.infer` for a model call. A `models.loop` that a handler starts while the section's loop runs raises an error.

The records of a loop started there would precede the outer batch's.

## 5. Tool calls

### The Engine emits a `ToolCall` effect, and the Harness calls the Pack

The Engine has each Pack tool's id, schema, and description, and the Pack has the code. When a run needs a Pack tool, `Run::step` returns a `ToolCall` effect and that part of the run waits. The Harness finds the Pack from the first segment of the tool id, calls `Pack::call`, and resumes the run with one answer: the output, the tool's error, or `Dropped`. The effect names the section, the caller, the section entry that made the call, and, for a model's call, the model whose round asked for it.

This split defines the Engine and the Harness. The Engine returns the work it needs, and its caller performs that work. The Engine is therefore independent of any async runtime, and it is a deterministic function of its inputs.

### The model calls offered tools, and Lua calls any Pack tool

The model calls the tools in its section's two sets, by wire name. Lua calls any tool in the run's catalog by id with `tools.call`. A Rust Pack can flag a tool `model_only_call`, and a script's call of such a tool raises an error of kind `model_only_tool`. The flag is part of the catalog entry because Lua must be able to name the tool when it offers it. Contract 0.6 gives a tool a name, a wire name, a description, a parameter schema, and one flag, which says that the tool changes files, so the other `ToolInfo` flags are still to be built for a library Pack.

### A Pack tool's failure returns to the model as text and raises in a script

When the model made the call, the Engine converts the failure into the call's result, wrapped as untrusted text, and the loop continues. When a script made the call, the failure raises at the call site with kind `tool`. The model receives the failure as text so that it can read it and recover, because the failure of a Pack tool's backend is outside the prompt author's control.

Two failures raise at `models.loop` itself. A model call to a name outside both sets fails the whole round with `out_of_scope_tool`. A call that the Harness drops, after a stop or a cancel, raises the canceled error.

### The Engine issues one round's Pack tool calls together, and each call that changes files alone

When one model reply asks for several Pack tools, the Engine issues the calls in one step, so they run at the same time. The loop appends the round to the messages as a whole, in the order the model listed the calls. An adhoc tool or a discovery tool runs by itself, between batches. So does a call to a tool whose `ToolInfo` sets `changes_files`: the Engine issues it once every call listed before it has its answer, and issues the calls listed after it once it has its own.

The calls run at the same time because they are independent of each other, and the model would otherwise wait for them one at a time. A call that changes files is the exception, because a later call of the same reply can depend on the change. The claims check belongs to a store alone ([Files and the VFS](files-and-the-vfs.md)), so on the Host's files this order is what keeps two changes of one reply from interleaving: each changing call finishes before the next one starts. The changes of one reply apply one at a time, in the order the model listed them, and a call listed before a change reads the files as they were. A script's `tools.call` is one call and waits for its answer, so it runs the same with the flag set or clear. The loop appends the whole round because model APIs reject a request unless every requested call has a result.

### Each call works under its own file identity

The Engine forks a file identity for each tool call when it issues the call, and ends it when the Harness answers the call or the Engine aborts it. Every file operation of the call runs under that identity, and the file system refuses one that arrives after the end ([Files and the VFS](files-and-the-vfs.md)). The claims check belongs to a store alone, so on the Host's files the calls that one round runs together, and the calls of two tasks, may read and write one path, and the later write is the content. Of the shipped Packs, `web` and `mcp` leave the files of their calls unused, and the tools of `std` and `bash` ask for file operations through the file functions of the C contract.

Each call has an identity of its own so that the end of one call stops the operations of that call alone. The file changes of a call that the Harness dropped then stop with the call.

### The Harness runs at most 10 calls at once and starts waiting calls oldest first

A run has at most 10 Pack tool calls in progress. The Harness queues the rest and starts the call with the lowest effect id when a slot frees. A Host sets the limit through `RunSetup::max_tool_calls_in_flight`.

The default follows comparable systems, which cluster at 8 to 10: Claude Code 10, Mastra 10, and CrewAI 8. The Harness owns the queue because a stop has to drop waiting calls as well. A queue in the Engine would hide waiting calls from the Harness, and a stop would miss them. Waiting calls start oldest first. The owner's reason: "If we get a batch, then we want to finish that batch."

### A stop drops the calls in flight, and a message waits for them

A stop drops every tool call in progress or waiting and leaves the run's cancel flag clear. Each dropped call raises the canceled error at its call site, so the run continues when the prompt catches that error with `pcall`, as the built-in chat prompt does. A cancel drops every call and ends the run. A message sent while tool calls run waits: the tool calls finish, and the loop appends the message before its next model request. A message sent while a model request is unanswered withdraws that request, and the loop sends it again with the message appended.

A message waits because a tool that is cut off may have altered something, and a stop is the way to cut tools short.

A tool whose `ToolInfo` sets `survives_stop` continues to run through a stop. The shipped Packs leave the flag unset.

### A loop runs up to 24 rounds, and the Host sets a ceiling

`models.loop` runs up to 24 model rounds by default. A prompt declares its own limit with `max_tool_iterations`, because the budget belongs with the prompt that needs it. The Host's ceiling, 1000 by default, caps the declared value.

## 6. Trust of tool output

### The Pack marks each output, and the Engine wraps untrusted text

`Pack::call` returns `ToolOutput::trusted(text)` or `ToolOutput::untrusted(text)`. A library Plugin ends a call with the result kind `RV_RESULT_TRUSTED` or `RV_RESULT_UNTRUSTED`, and the loader builds the same `ToolOutput` from it. A Pack marks as untrusted any output that embeds data an attacker can influence. The Engine wraps untrusted text in an envelope before the model or the script receives it. The exception is a script's call of a tool flagged `describes_tools`: the script receives the untrusted listing as written. The code's docs describe the envelope as "defense in depth, not a security boundary".

### The envelope is a tag named with a per-run nonce

The envelope is an XML-style tag whose name includes a nonce. A sentence at its top tells the model that the enclosed text is data. A run has one nonce, derived from the run's seed with SHA-256.

Each result includes the sentence because a rule stated next to the data aids compliance. The nonce is the same across a run because a fresh nonce for each call would break the byte-identical envelopes that prompt caching needs. The hash is one-way because a reversible derivation would let a reader of one nonce compute the seed.

### The Engine escapes `<`, control markup, and the nonce inside wrapped text

The Engine writes each `<` in wrapped text as `&lt;`, so a fake closing tag inside a page is plain text, and the data block ends only at the real tag. It puts a space after the first character of each chat-template delimiter in a fixed list, so `[INST]` becomes `[ INST]`. It breaks each copy of the run's nonce the same way, because mimicry of the delimiter is the documented counter-attack against nonce envelopes.

The list of delimiters is closed on purpose. The `AGENTS.md` of the `runvil-types` crate states the rule: "The control-markup inventory is closed: add table entries, each with a family rationale, and never generalize the matcher."

### The envelope lowers the risk of injection, and a section's offers decide which tools a misled model can call

A model can still follow an instruction inside the envelope. The envelope only lowers the probability that it does. The checks of `web/fetch` stop a request to an internal address. They leave the query string of a public URL as written, so a model that follows an instruction in a fetched page can put run data into it. The URL check accepts such a request because the destination is public and the payload looks the same as an ordinary query.

The control that works is the prompt author's, applied with `tools.offer`: the author leaves the tools that could exfiltrate text out of each section that reads untrusted text.

### Lua reads metadata and decoded JSON as plain text

Lua reads `tool.description`, `tool.parameters`, and a Pack's `name` and `tools` as the declared text, outside the envelope. The owner's rule: "when Lua asks for tool descriptions and such, it should never arrive in the untrusted envelope". Strings from `json.decode` are plain Lua strings as well, and an author who sends them to a model wraps them with `untrusted(s)`.

### The shipped Packs mark outside text as untrusted

`web/fetch` and `web/search` return untrusted output. `std` marks file content and file names as untrusted. It marks as trusted its fixed stub text, the answers of Write, StrReplace, and Delete, which hold fixed text and counts, and a Grep answer for zero matches that holds only fixed text and counts. `bash/shell` marks the output of each command as untrusted. A discovery listing is untrusted because Packs and their servers write the descriptions. `tasks.receive()` returns the operator's messages as plain text, because a wrapped answer would hide the operator's own words from the script.

### The `mcp` Pack marks its output by caller

The `mcp` Pack reads the caller from its `ToolContext`. For a call the model made, it marks the server's text untrusted. For a script's `tools.call`, it marks the same text trusted, so the script receives the raw string and can decode the listing JSON. The cost is that a script that forwards MCP text into a prompt, or an adhoc handler that returns it to the model, sends it unwrapped, and that is the script author's decision. Only a Rust Pack receives the call's origin: the loader passes a library Pack the tool's name, the arguments, the file functions, the Host's resources, and the call's waker, so the contract needs the origin before `mcp` can move to it, which is planned work.

## 7. Writing a Pack

### A library Plugin is written against `runvil-plugin-sdk`, and a Rust Plugin against `runvil-plugin`

Two crates define what a Pack author writes against, and each shipped Plugin uses one of them. `std`, `bashkit`, and `web` are written against `runvil-plugin-sdk`, the safe layer over the C contract. The author implements the SDK's traits for a Plugin and a Pack, and the SDK's macro exports the entry point. Each of them depends on that crate and on libraries from outside Runvil, so it builds independently of the Harness. `plugin-mcp` is written against `runvil-plugin`, which defines the `Pack` trait, `PackFactory`, `PackInfo`, `PackRegistry`, and `ToolContext`, and re-exports the Engine and file-system types an author needs. The `## Invariants` section of that crate's docs is the rule list for its Pack authors. A Host imports the same items from `runvil::plugin`.

The two crates are two APIs, and the design is one. The Rust API has abilities that the contract lacks, and the move of `plugin-mcp` onto the contract is planned work. The rest of this section describes the Rust API, and the section on the C contract describes the contract.

### A Rust Plugin is a function named `install`, and its arguments are its own

A Rust Plugin is a function that inserts its Packs into the Host's `PackRegistry`. The Host names the Plugin with `HostBuilder::with_rust_plugin`, as a function that is lent the registry, and that function calls `install` with the registry and whatever else that Plugin takes, as plain arguments. `HostBuilder::build` makes the registry and calls each such function in the order written. `plugin_mcp::install` takes the Host's list of MCP servers, which is the `mcpServers` object of an `mcp.json` file, and the handle of the Host's tokio runtime. For each Pack, the function builds a factory from its arguments and calls `PackRegistry::insert` with the Pack's `PackInfo` and the factory. When setup fails, it calls `PackRegistry::insert_unavailable` with a reason.

The arguments are plain because a Rust Plugin is compiled into the Host and shares the Host's Rust types, so the compiler checks that the Host passes each one. The build of the `Host` owns the registry, and Plugins fill it. The owner's rule: "A host can only install plugins". The `install` function is the Rust-only way into the registry. A library Plugin inserts its Packs through the contract's `insert_pack`, and the loader calls `PackRegistry::insert` for it.

### The Harness makes one Pack instance for each run

The Harness calls `PackFactory::new_pack` once for each Pack in a run. The instance drops when the run ends, and its `Drop` does the cleanup. The factory owns the state that runs share, such as MCP connections and HTTP clients.

A Rust Pack is per run because the C contract has the same design for library Plugins, with verbs that create and delete a per-run Pack.

### `new_pack` returns at once, and `install` builds what the runs share

The Harness calls `new_pack` while it prepares the run, so a factory returns quickly: it clones the parts that `install` built and stores them in a fresh instance. `new_pack` runs inside the run's future, so the crate's docs forbid it to block, do I/O, or panic.

### The `Pack` trait lists the tools, performs a call, reports readiness, and renders

`Pack::tools` returns the Pack's tool list, which the Harness reads once per run. `Pack::call` performs one tool call. `Pack::ready` resolves when the tool list is complete. `Pack::render` shapes the offered tools for a model.

### `ToolContext` lends each call the tool id, its files, and the call's origin

`ToolContext::tool` returns the id of the called tool. `ToolContext::files` returns the call's files, a `ToolFiles`: the call asks for one operation of the `VfsOp` list with `request` and awaits its `VfsOutcome`. A request works under the call's own identity, on paths rooted at `/` of the file system that the Host gave the run. Each store is a separate value outside that file system, so a file that `store.write` wrote stays out of a tool's reach ([Files and the VFS](files-and-the-vfs.md)). The Engine keeps the file access of the call, so a request is the call's one way to the run's files. `ToolContext::origin` says which section and which caller made the call. The context lends each of them only until the call ends, so the Rust compiler rejects a Pack that stores the files.

The owner's rule: "The tool can't keep the access. ... it has to give it up, after the call is done". What a Rust Pack needs for longer than one call, its Plugin takes as an argument of `install` and stores in the Pack's factory.

### The rules that every `Pack::call` follows

1. Return to the poll quickly. A call sends blocking or CPU-heavy work to the Host's runtime, because the Harness polls every effect of a run on the same task. A call awaits a file operation that it asks for through its files, and the Harness performs it between two polls of the call.
2. Report failure as an `Error`. The Harness answers a panicking call as `Dropped` and logs the panic.
3. Mark as untrusted any output that embeds data an attacker can influence.
4. Expect to be dropped. The Harness drops the call's future on a stop, on a cancel, and when the calling task ends.
5. Finish independently of the run's other calls. A call that waits for a sibling queued behind the limit of 10 can stall the run.

### A tool's result depends on the call's arguments, the run's files, and what the tool reaches outside the run

A tool's result depends on the call's arguments, the run's files, and what the tool reaches outside the run. Whatever one call would leave for a later call goes into the run's files, and the record holds each such change, because a tool changes a file of the run only by a request through the files of its call ([Files and the VFS](files-and-the-vfs.md)). A connection or a client that a Pack's factory shares between runs leaves each result unchanged, so the rule leaves it alone.

The owner's rule: "Tools are not allowed to have internal states, and if they do, they have to be recorded." The reason is replay, which feeds each recorded tool result back to the Engine in place of a call to the tool. The owner's words: "On a replay, the, the shell tool is not invoked. The shell tool never executes." A run that went on live after a replay would call a tool that is missing the state its earlier calls left. The shell of the `bash` Pack follows the rule with a shell for each call: what a script leaves in its shell ends with the call, and the next call finds the run's files alone (see The Packs that ship with Runvil).

### A tool asks for a file operation, and the Harness performs and records it

A tool asks for a file operation and waits for the outcome. A Rust Pack's tool asks through the files of its `ToolContext` and awaits the outcome. A library Pack's tool asks through a file function of the C contract, and the loader makes the same request for it through the files of the call (see The C contract). The Harness reports the request to the Engine, which issues it as a `Vfs` effect that names the call. The Harness performs the operation, records it and its answer, and wakes the call with the outcome ([The Harness and the Host API](harness-and-host.md)). The call stays in progress the whole time, and its one answer is still the tool's result. A request is made when the call first polls it, and it goes out when the Harness takes it, between two polls of the call, so a request that the call drops before then stays unsent. The Harness performs the requests of one call in the order the call made them, and it drops what a call that has ended still asks for. The owner's rule: "every VFS operation that's performed by a tool has to turn into an effect in the engine."

A tool that changes files sets `changes_files` in its `ToolInfo`, and the Engine then runs each of its calls alone, in the order the model listed them (see Tool calls).

The operation itself is synchronous, because the storage underneath is synchronous ([Files and the VFS](files-and-the-vfs.md)): the Harness performs it with one blocking call. The Harness is also independent of any particular async runtime, so each part of the context has to be independent of tokio. A request is a future that the Harness itself wakes, so a tool awaits it on any runtime.

The shipped Rust Pack, `mcp`, leaves the files of its calls unused. The shipped tools that touch files are in the library Plugins `std` and `bashkit`, and they ask through the file functions of the contract, so the record holds each of their file operations.

A long file job yields so that the Harness can cancel it. Grep, in the `std` library, ends its step after every 32 files or directories, so a stop or a cancel drops a long search between two steps.

### The run's mode policy decides each file change that a tool makes

The Harness performs every file operation of a tool on the file access of its call, and the file system applies the run's mode policy to each one. In Ask mode it refuses each change, and in Plan mode it accepts changes to markdown paths. A `store` call is outside the policy, because the run's store is a separate value, outside the file system that tools work in. Workshop's runs use the file system's default policy, `AllowAll` ([Files and the VFS](files-and-the-vfs.md)).

The check is in the file system because every operation passes through the access, so the check covers every tool, including each tool written later.

### A Rust Pack runs on the Host's async runtime, and a library Plugin owns a runtime

A Rust Pack that needs an async runtime uses the Host's. Its Plugin takes the handle of that runtime as an argument of `install`, and the Pack's factory keeps the handle. The `mcp` Pack spawns the connection to each server, and each call of a server's tool, on it. A library Plugin links a copy of tokio of its own, so the Host's handle is of no use to it, and a Rust object stays on the Host's side of the contract. A library Plugin that needs a runtime therefore owns one, as the bashkit Plugin and the web Plugin do. What the Host hands a library Plugin is its resources, which are JSON values that each of the Plugin's tool calls is lent (see The C contract).

### A Pack's Lua prelude runs once in each section VM

For a prompt that declares the Pack, the Engine runs the Pack's prelude in each section VM, before `require` and before the author's code. The prelude receives the Pack id through `...` and defines globals, which the Engine then seals. A prelude defines each global under a name that is a UTF-8 string and leaves its environment a table that is not weak, and the install fails for a prelude that breaks either rule. Each Rust Pack that ships leaves its prelude unset. A Pack from the C contract has a name and a description, and a prelude for it is still to be built.

## 8. The C contract

### A Plugin exports one function, and each call to it names a verb

A dynamic library exports one function, `runvil_plugin`, and each call passes an `RvCall` struct that names a verb. In contract 0.6, verbs 1 and 2 load and unload the Plugin, and a load names the Plugin and inserts its Packs. Verbs 3 and 4 create and delete one run's instance of a Pack, and the instance lists its tools when it is created. Verb 5 runs one step of a tool call, and verb 6 frees the state of a call that the Host abandoned. Numbers 7 and 8 are reserved for later versions.

A single function means one exported symbol, and a later version of the contract adds verbs.

### The contract has its own repository, and each 0.x version is a separate contract

`runvil-plugin-api` contains the C header, the Rust mirror `runvil-plugin-sys`, and a safe SDK, `runvil-plugin-sdk`. While the major version is 0, a Plugin built for 0.6 loads only in a Host that reports 0.6. The Plugin reads the Host's version first in `RV_PLUGIN_LOAD` and answers any other version with `RV_ERR_UNSUPPORTED`.

The contract has a repository of its own so that a Plugin depends on that repository only.

### Each callback belongs to one verb and takes a `ctx`

The functions a Plugin may call back are fields of a verb's arguments struct. For a load, `RvLoadArgs` has `ctx`, `refuse`, `describe_plugin`, and `insert_pack`. `RvPackNewArgs` has `refuse` and `add_tool`, and `RvToolCallArgs` has `finish`, the file functions, `answer`, the answer to the file request of the call's last step, `resources`, the Host's resources as text, and `waker`, the call's waker. Each function takes the `ctx` first and answers only while its call runs, on that call's thread. A callback belongs to one verb because each verb has its own lifetime and thread rules. The one exception is the `wake` of a call's waker, which takes the waker's own `ctx` and answers on any thread until the Plugin is unloaded (see the entry on a parked call).

A tool call is a series of steps. The Host sends `RV_TOOL_CALL` once for each step, a step that returns `RV_PENDING` leaves the call open, and a step ends the call by calling `finish` once with its result and returning `RV_OK`. The call has steps because a step that blocked would stall its run, and the Host can abandon a call only between two steps.

### A step that returns `RV_PENDING` parks its call, and the Plugin wakes it

Every step of a call is handed the call's waker, the same one at each step. A step that returns `RV_PENDING` parks the call. The Host sends the call's next step after the Plugin has woken the call through that waker, or, when the step made a file request, when the answer is ready. A wake during a step counts, so a step that wants the next one at once wakes its call and then returns `RV_PENDING`. Wakes do not add up: a wake asks for the next step that begins after it, however many wakes come before that step. The Host may also send a step that no wake asked for, but none while a file request waits for its answer. A call that nothing wakes stays parked until the Host abandons it, and a stop or a cancel abandons it at any time.

A Plugin may wake a call on any thread, from the call's first step until the Plugin is unloaded: during a step, between two steps, while the Host abandons the call, and after the call has ended. The Harness keeps what a waker points at until the Plugin is unloaded, so a late wake reaches live memory. A late wake does nothing for the call that ended, and it can cost a later call of the same Plugin one step that the call did not need.

A call that waits on something outside its run, such as a reply from the network or a timer, is parked for the whole wait, and a thread of the Plugin wakes it when the wait is over. The run spends nothing on a parked call. A Host that sent a pending call its next step as soon as it could would keep a thread busy for as long as the call waited. The waker means what a waker means for a Rust future, so the SDK hands it to a tool's future as an ordinary `Waker`, and a tool written as `async` code parks with no code of its own. A waker stays valid until the unload because a late wake is ordinary: a timer thread fires after its call ended, and a destructor wakes as the call is abandoned. A Plugin cannot crash its Host with one.

### A file function takes a request, and the call's next step receives the answer

A step reaches the run's files through the file functions of its arguments: `file_stat`, `file_list`, and `file_read` read, and `file_write`, `file_append`, `file_mkdir`, `file_remove`, `file_rename`, and `file_copy` change the files. A file function takes one request and returns only whether the Host took it. The step returns `RV_PENDING`, the Harness performs the operation, and the call's next step receives the answer: what the request asked for, a status alone for a change, or the error of an operation that failed. The Host sends that step when the answer is ready, and no step before then, even when the Plugin wakes the call. A call has one request outstanding, so a second request in one step is a misuse, and so is a request in a step that calls `finish`. An operation that fails leaves the call running, and the tool decides what its result says. A write or an append takes at most 16 MiB, and a tool writes a longer file as one write and then one append for each further part. The contract leaves out a function that asks whether a path exists, because a stat answers that.

The loader makes each request through the files of the call, as a Rust Pack's tool does (see Writing a Pack). Each file operation of a library Pack's tool, a read as well as a change, is therefore a `Vfs` effect that the Harness performs under the run's policy and records ([Files and the VFS](files-and-the-vfs.md)). The owner's rule: "every VFS operation that's performed by a tool has to turn into an effect in the engine." The answer waits for the next step because the Harness performs each operation as an effect of the run, between two steps of the call. A function that returned its result inside the step would perform the operation there, outside the run's record.

The contract has an error kind for a conflict between two tool calls, `RV_FILE_ERROR_CONFLICT`, and the Harness reports each failed operation under one of the other kinds, because the claims check belongs to a store alone and a tool reaches the Host's files.

A tool whose calls change files sets the flag `RV_TOOL_CHANGES_FILES` when its Pack instance adds it. The loader then sets `changes_files` in the tool's `ToolInfo`, and the Engine runs each call of the tool alone and in order (see Tool calls).

### Every tool call is lent the Host's resources as JSON text

A Host offers the Plugins of the contract its resources: JSON values under names of two segments, a vendor and a name, such as `runvil/web-search`. Every step of a tool call is handed them as `resources`, the JSON text of one object with a key for each resource, and `{}` when the Host offers none. The text holds the resources as they were when the call started, and every step of one call gets the same text. What a resource holds is between the Host and the Plugins that read it. A Plugin parses the text with a JSON library of its own, and a Plugin with no use for the resources ignores them.

A Host fills a `HostResources` and passes it to the builder of its `Host` with `with_resources`. `HostResources::provide` takes a name and a function that returns the resource's value, or nothing when the Host has none to offer right now. It refuses a name that is not a vendor and a name, and a name that is offered already. The Harness calls each function as a tool call starts, so the function returns at once, and a resource whose function returns nothing is absent from that call's text. A Host whose value stays the same passes a function that returns it.

The resources are text because a Rust object stays on the Host's side of the contract, and every Plugin can read JSON, whatever language it is written in. The Harness reads them as each call starts, so a Host whose value changes, such as the address of its Gateway, starts no task to hand the change on: the next call reads the new value. One call has one text from its first step to its last, so a tool finds the same resources at each of its steps.

### A library Plugin and a C entry point compiled into the Host receive the same calls

A Host names each Plugin of the contract on the builder of its `Host`. `with_plugin_dir` names every dynamic library in a directory, `with_plugin_file` names the library in one file, and `with_plugin` names a C entry point that is compiled into the Host. `HostBuilder::build` loads them, and each is offered the resources that the Host gave the builder. The loader names the two kinds `PluginSource::Library` and `PluginSource::Linked`. Each entry point gets the same calls, so one path loads both kinds. The owner's rule: "those have to be treated exactly the same. In other words ... the entry point has the same shape". For each Pack that a Plugin inserted, the loader puts a factory in the registry. The factory sends `RV_PACK_NEW` for each run, the instance sends one `RV_TOOL_CALL` for each step of a call, and its drop sends `RV_PACK_DELETE`.

`std`, `bashkit`, and `web` are library Plugins. The shipped Plugin that is compiled into the Host, `plugin-mcp`, is a third kind. It is a Rust `install` function written against `runvil-plugin`, which the Host calls itself, so the contract's calls go to the library Plugins alone. The docs of `runvil-plugin` call that third kind a linked Plugin as well. The move of `plugin-mcp` onto the contract is planned work.

A Pack from the contract has tools only, and a tool has a name, a wire name, a description, parameters, and the flag that says it changes files. The other `ToolInfo` flags, the prelude, `ready`, `render`, the call's origin, a configuration at `install`, and a Pack inserted as unavailable exist in the Rust API alone. The contract is still to gain the ones that a shipped Pack uses.

Each of those steps makes one `PluginReport`, which holds each Plugin that loaded and each failure. A Plugin that fails to load is a failure of the report, and a step that names a directory continues with the next library. A Pack that the registry refuses is a failure too, and its Plugin stays in the report, because the Plugin did load and its other Packs went in. The two kinds are reported alike for the reason they are loaded alike: a Host reads one report whichever kind it loaded. A failure of a library names the library's path, and a failure of a C entry point has no path to name.

A loaded Plugin stays loaded while a `PluginReport`, one of its Packs, or a run's instance of one of its Packs holds it. A `Host` keeps the report of each of its steps and the Packs, so a Plugin unloads after the `Host` is dropped and its last Pack instance is deleted.

Loading happens in the Harness, and the Engine receives data: the tool catalog, the roster of the run's Packs, and the prelude text. The reason is that the Engine is pure: tool calls are effects that the Harness performs, and a replay can feed recorded results back to the Engine in place of calls to the Plugins.

## 9. The Packs that ship with Runvil

### `std` copies Cursor's file tools

`std` is a library Plugin. Its code is in the `runvil-plugin-std` repository, written against `runvil-plugin-sdk`, and Workshop loads its library from the Plugin directory. `std/read`, `std/dir`, `std/grep`, `std/write`, `std/replace`, and `std/rm` work, and `std/glob` answers a fixed text that says the tool is missing from this build. The model sees Cursor's names, such as `Read`, `Grep`, and `StrReplace`. The tool definitions are Cursor's with a recorded list of edits, except ListDir's, which is the crate's own.

Write, StrReplace, and Delete change the run's files, and each sets `changes_files`, so the Engine runs each of their calls alone, in the order the model listed them (see Tool calls). Write makes a file, or replaces the whole content of one, with one write, and the Host makes each absent directory on the way to the file. It refuses contents over 16 MiB, the most that one write of the contract takes. StrReplace reads a file and writes it back with one text in place of another. It refuses a directory, a special file such as a pipe or a device, and a file over 16 MiB, as Read does, the file must hold UTF-8 text, and the text to replace must occur exactly once unless the call sets `replace_all`. When the text occurs zero times, or more than once in a call that needs exactly one occurrence, the failure states how many times it occurs. The count takes the occurrences one after another from the start of the file, each starting at or after the end of the one before it, and a call with `replace_all` replaces those. A text occurs exactly once when it starts at exactly one place, and two places that overlap are two places, as `ana` starts at two places in `banana`. When a text starts at more than one place and its count is one, a call that needs exactly one occurrence is refused with a sentence that says so, in place of the count. StrReplace refuses an edit that would leave the file over 16 MiB, the most that one write takes, and it refuses before it builds the edited text, because a call with `replace_all` could otherwise ask for far more memory than the file and its arguments hold. The edit is a stat, a read, and then a write, because the contract lacks a function that replaces text in one step, so it can lose a change that another task makes to the file between the read and the write ([Files and the VFS](files-and-the-vfs.md), [runvil-plugin-std issue 4](https://github.com/cppalliance/runvil-plugin-std/issues/4)). Delete removes a file or a link, leaves what a link leads to in place, and refuses a directory. It asks what the path names before it asks for the removal, because the removal of the contract also takes an empty directory. The two are requests of their own, so an empty directory that another task puts at the path between them is removed. A directory with entries always stays. A removal in the contract that takes a file only would close that gap ([runvil-plugin-std issue 3](https://github.com/cppalliance/runvil-plugin-std/issues/3)).

The Pack copies Cursor's text because the model was trained against that exact text, and a stored copy can be diffed against Cursor's. A stub answers as a success so that calls to the Pack exercise both success and failure from end to end. Walks skip `.git` and list the other dotfiles, because a model often needs to edit `.gitignore` and similar files. A walk leaves out a name that contains a control character and counts it, because a line feed in a name would forge a line of the answer. The shell is in a Pack of its own, `bash`, so a run that wants a shell declares it.

### `bash` runs a shell inside the run, and the shell reads and changes the run's files

`bash` is the Pack of a library Plugin. The bashkit Plugin has a repository of its own, `runvil-plugin-bashkit`, and is written against `runvil-plugin-sdk`. It reports the name `bashkit` and inserts one Pack, `runvil/bash`. The Pack has one tool, `bash/shell`, and the model sees it as `Shell`, which is Cursor's name for its shell tool. The definition is Cursor's, with a recorded list of edits. Workshop loads the Plugin's library from the Plugin directory, beside the library of `std`. The built-in chat prompt requires `bash` under `uses:` and offers it.

The shell is a bash interpreter from the `bashkit` crate, and it runs inside the run. Every command is built into bashkit and runs in the Host's process, and the bashkit features that reach the network are off. Each call builds a shell of its own, which starts at the root of the run's files and goes when the call ends. The working directory, variables, and functions of one call end with it, so the run's files, as the earlier calls left them, are all that a call finds of an earlier call. A tool's result depends on the call's arguments, the run's files, and what the tool reaches outside the run (see Writing a Pack), and a shell for each call is how the shell follows that rule. The owner's reason: "Its the easiest to implement and doesn't expand the API surface." A command ends after 30 seconds, or after the `block_until_ms` of its call when that is less. The answer is the exit code and the output, marked untrusted.

The shell's files are the run's files. The call that runs a script performs each file operation of the shell, a read or a change, through the contract's file functions, so every operation goes through the Host: the run's policy applies to it, and the run's record holds it. Redirects, `tee`, `sed -i`, `cp -r`, `mv`, `rm -r`, `mkdir -p`, `touch`, and `mktemp` work. The tool sets `changes_files`, so the Engine runs the Shell calls of one reply one after another, in the order the model listed them (see Tool calls). Each operation that goes to the Host ends a step of the call, so the Host can abandon a script between two operations, and a change that reached the Host before then has happened.

The contract reports what a path names and how long a file is, and it lacks a function for a mode, a time, a link, or a named pipe. The shell answers those itself, each with a fixed answer: a directory shows mode 755 and every other path 644, `chmod` to another mode fails, a time that a command sets is discarded, and `ln` and `mkfifo` fail. Every file's execute bit is clear, so a script runs as `bash script.sh`. The shell reads at most 16 MiB of one file and writes at most 16 MiB at a time, and a longer write or append fails and leaves the run's files as they were. On every platform it refuses a write or an append to a path under `/dev/`, because bashkit on Windows sends a redirect to `/dev/stdout` or `/dev/stderr` to its file system ([bashkit issue 2933](https://github.com/everruns/bashkit/issues/2933)), where the write would make a file of that name among the run's files. A redirect to `/dev/null` discards its output all the same, because bashkit answers that one itself. A copy, a move, a new directory, and a removal under `/dev/` go to the Host as under any other path. `mktemp` makes its file under `/tmp` among the run's files, and makes that directory when it is absent.

### `web` searches through the Gateway and fetches pages itself

`web` is the Pack of a library Plugin. The web Plugin has a repository of its own, `runvil-plugin-web`, and is written against `runvil-plugin-sdk`. It inserts one Pack, `runvil/web`, with the tools `web/search` and `web/fetch`. The Plugin owns a tokio runtime, because a library links its own copy of tokio, so the Host's runtime is out of its reach. Each search and each fetch runs on that runtime, and its call is parked until the request ends (see The C contract).

`web/search` sends each search to the Gateway, which has the search vendor's key. The tool reads the `runvil/web-search` entry of the Host's resources that its call is lent: the address of the Gateway's web search route and the Gateway's key. A Host that uses the Gateway offers the entry with `provide_gateway_resources` ([The Harness and the Host API](harness-and-host.md)). When the entry is absent, because the Host offers none or has no Gateway right now, the search fails with a text that says so, and `web/fetch` works all the same. The Pack has both tools in every run, because the resources are read as each call starts, so only a call finds out whether a search can run.

A fetch reports a failure of the target, such as a 404 or a timeout, as text the model can act on. The text states the status and the URL, and the Pack drops the error page's body, because the body is untrusted HTML and unhelpful for recovery.

### `web/fetch` connects to public addresses, and checks each address when it connects

The model supplies the URL, so `web/fetch` checks it twice. Before a request, it refuses a URL that uses plain `http`, contains a user name or password, names a port other than 80 or 443, or names an IP address in place of a domain name. When the client connects, a resolver of its own drops each DNS answer in a blocked range, such as a private or loopback address. Both checks run again on each redirect, up to 5 redirects. The client omits cookies and credentials.

The second check runs at connect time because a check that reads only the URL string is defeated by a name that resolves to an internal address and by DNS rebinding.

### A fetch returns a page's text under a three-line header

`web/fetch` takes a `url` and two optional arguments: `raw` renders the whole page, and `max_chars` lowers the limit of 40,000 characters. The tool extracts the main article of an HTML page as markdown, returns JSON, XML, and plain text as written, and refuses every other type and a response that omits `Content-Type`. It reads up to 8 MiB of a body, counted after decompression, so a gzip bomb hits the same cap.

The result starts with the lines `url:`, `truncated:`, and `extraction:`. The first is the final URL after redirects, so a citation names the address that returned the bytes, which can differ from the one the model supplied.

### `mcp` is one Pack with two tools for remote servers

The Plugin's `install` takes the whole `mcpServers` object as its list of servers, beside the handle of the Host's tokio runtime, and the Pack connects to each server in the background. It offers `mcp/get_dynamic_tools`, which lists the servers and their tools, and `mcp/call_dynamic_tool`, which calls one. The listing shows each server as `ready`, `loading`, or `error`. The owner's rule: "there should just be 2 static tools that the mcp pack offers".

The client speaks Streamable HTTP through the `rmcp` SDK and returns results as text. An entry for a local server, one started with a `command`, shows as `error`. Local servers are still to be built, because each one needs a launcher program (`npx`, `uvx`), a job object or process group so that the server dies with Workshop, and handling for Windows `.cmd` shims.

### A server entry has the format of Cursor's `mcp.json`

A remote entry has a `url` and optional `headers`. The Pack expands `${env:NAME}` and `${userHome}` in an entry's strings, refuses an entry that names an unset variable, and leaves every other `${...}` form as written, as Cursor does. It ignores unknown fields because the format is defined outside this repository.

The Pack refuses the legacy `sse` type and `headers` that set `accept`, `Mcp-Session-Id`, or `Last-Event-Id`. The `rmcp` SDK sets those three headers, and the Pack refuses them at install, since `rmcp` would refuse such an entry only on the first request. A refused entry shows as `error` in the listing, with the reason, and the other servers still connect.

### The `mcp` Pack names the keys of `env` and `headers` and omits their values

A value in an entry's `env` or `headers` is often a secret. Each refusal reason, log line, `Debug` output, and tool error of the Pack may name a key and omits the value. The Pack leaves the values out because a reason appears in the notice of a refusal, and a secret inside it would appear there too.

The Pack uses `rmcp`'s own HTTP client, which refuses redirects, so the client sends the configured headers to the configured server only. One case is still open: `rmcp`'s own logs can name a URL that contains an expanded variable.

### `user` marks a prompt that needs a person

`runvil/user` is a Pack of every Host, and the Harness makes it itself. A prompt that needs a person declares `uses: [user]` and reads the person's messages with `tasks.receive()`. When `RunSetup::has_user` is false, the Harness marks the Pack unavailable, and the run's first step refuses a prompt that declares it. The Pack's tool list is empty. A model asks the person a question in its reply: the reply ends `models.loop`, and the prompt then returns to `tasks.receive()`.

### `subagents` reserves a name

`runvil/subagents` is a Pack of every Host, and the Harness makes it itself. It reserves its name for subagent tools that the Engine will provide. It is a Pack so that a prompt can name it under `uses:`. `tasks` is a language feature, because an option to leave the `tasks` table out of a run would be useless.

## 10. Planned work

Each of these items is still to be built.

- **One Plugin API.** `plugin-mcp` written against the C contract and named with `with_plugin`, so that a Pack compiled into the Host goes through the same contract as a Pack from a library and the `install` function, which is what the Plugin is, goes away. The contract first needs what the Pack uses and contract 0.6 lacks: a configuration for its list of servers, the call's origin, the `describes_tools` flag, the Host's runtime handle, a Pack inserted as unavailable with a reason, and error causes. The Plugin is compiled into the Host, and its `install` takes the list of servers and the Host's runtime handle as arguments.
- **Glob in `std`.** A `std/glob` that finds files by a name pattern and returns their paths. The tool answers a fixed text that says it is missing from this build.
- **Approval by a person.** The step that asks the operator to approve a file change. When the run's policy answers `Ask`, the file system refuses the operation with `VfsError::ApprovalRequired` and a reason, which a library Pack's tool receives as a denied file error with the same reason, and under Workshop the policy allows every operation, because Workshop's runs use `AllowAll`.
- **Ignore files.** Applying `.gitignore`, so that Grep skips a folder such as `target/` and spends its budget of 100,000 files elsewhere. Walks skip `.git` and names that contain a control character.
- **Shell state that lasts across calls.** A shell that saves its state to a file of the run and loads it at its next call. Each `bash/shell` call builds a new shell, so the working directory, variables, and functions of one call end with it.
- **Subagent tools.** The tools of `runvil/subagents`, and the wrapping of text that one model writes for another. The Pack only reserves its name.
- **MCP in the Dynamic Set.** The `mcp` Pack rendering its servers' tools into the Dynamic Set, which retires its own two tools, along with local servers and OAuth. The Pack offers its own two tools, for remote servers.
- **Filters and labels.** An offer narrowed by a hint such as read-only, and labels on `ToolInfo`, modeled on MCP's, for a future Ask mode. An offer names each tool or Pack, and the `ToolInfo` flags are `structured_output`, `survives_stop`, `describes_tools`, `model_only_call`, and `changes_files`.
- **Partial rounds.** Saving the finished results when a stop cuts a round short. The loop discards the whole round.
- **Fetch settings.** A way for a Host to set the fetch limits. The fetch policy has a setter for each limit, and the contract hands a Plugin no configuration, so each Host runs the defaults.
