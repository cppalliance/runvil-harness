# Model calls and conversations: design decisions

This page covers how a prompt calls a model and what a conversation is: the `models` API, model roles, the model loop, replies, limits on a round, messages, compaction, and the client that sends a model request. A prompt names a model role, and the Host decides which model fills each role. Lua starts every model request: `models.infer` sends one prompt, and `models.loop` runs rounds of model replies and tool calls over `messages`, the one conversation of a section entry. A compactor, a Lua function, shortens the conversation when it exceeds the model's context window. The Engine asks for each round as an effect, and the Host's broker sends it. The standard broker uses the Gateway client.

## 1. Model roles and the models that fill them

### Every prompt has a fixed set of model roles, and the Host maps each role to a model

The roles are `default`, `strong`, `small`, `fast`, and `creative`, the variants of `ModelRole`. A prompt names a role, and the Host decides which model fills it, so one prompt file runs on any Host's models.

The names are fixed because a label that a prompt declared for itself would be an empty formality: the Host would bind every such label to the same model.

### The `models:` key is optional and states requirements

Under `models:` in the front matter, a prompt maps a role name to optional requirements: `min_context`, a token count, and `thinking`, true or false. A prompt that omits the key runs on the Host's `default` model. The parser refuses a role outside the fixed set, any other field, a repeated role, and `min_context: 0`.

The key is optional so that a typical prompt leaves it out, and a prompt with a requirement can still state it. `thinking` is a requirement on a role's model. The owner's reason: thinking "is a property of a model not a model role".

### The Engine's caller passes one map from role to model, and `default` fills each role that the map omits

The caller passes a map from `ModelRole` to `ModelInfo` to `RunParams::with_models`. Before any Lua runs, the Engine builds a `ModelBinding`, one model with its request settings, for each role, and uses the `default` entry for a role that the map omits. An empty map tells the Engine that the Host lacks a model. Every role is then empty, and `models.infer` and `models.loop` raise an error of kind `model_required`.

### The run's first step checks each declared requirement against the model that fills the role

A role's `min_context` fails on a model with a smaller context window. `thinking: true` fails on a model whose mode is `Never`, and `thinking: false` fails on one whose mode is `Always`. A `Switchable` model passes both, and each round on the role sends the switch as the request field `chat_template_kwargs.enable_thinking`. A failed check, or a map that has entries and lacks `default`, ends the run at its first step with the code `requirements_unmet`.

A `Switchable` model passes `thinking: false` because it can honor the requirement: the role's binding asks for thinking off.

### The Harness passes one model for a run, as the `default` entry

The Harness asks the Host's broker for its model list ([The Harness and the Host API](harness-and-host.md)) and takes the model that `RunSetup.selected_model` names, or the first one listed. The Harness passes that model as the `default` entry, so one model fills every role. A model id has a namespace and a name, as in `gateway/fast`, and a request names the model by the name part.

The Harness passes only `default` because `RunSetup` selects a model for the whole run, not for each role. A model for each role is planned work.

## 2. Choosing a model in Lua

### A chain owns its current model, and `models.use` switches it for the rest of the chain

A chain is the scheduler's record of one sequence of section entries ([The Engine](engine.md)): the main walk through the sections, a `call` child, or a task. A round that names a handle runs on that handle, and every other round runs on the chain's current model. `models.use('strong')` switches the chain for the current section and for every section it enters afterward, and `models.current()` returns a handle for the current model.

The chain owns the selection so that one model choice applies across sections and in fanout arms. The lasting switch can raise a run's cost by accident, so an author who wants one expensive step saves `models.current()` first and passes that handle back to `models.use`.

### A `call` child, a task, and a fanout arm start on a copy of the parent's current model, and an executed prompt starts on `default`

A `call` child, a `tasks.spawn` task, and each fanout arm copy the parent chain's current model when the Engine dispatches the call or the spawn. A later `models.use` in the child changes only the child. A prompt that an `execute` runs starts on the `default` role of its own model set, whatever model its caller is on, and a `models.use` in it leaves the caller's model as it is ([The prompt language](prompt-language.md)). The main walk starts on the `default` role. The preamble, the Lua before a prompt's first heading, is the first section of that walk ([The prompt language](prompt-language.md)), so a `models.use` there is still in effect in the sections after it.

The Engine copies at dispatch because a spawned task can wait in a queue before it starts, and a copy taken at the task's start would include a later switch in the parent. [Tasks and fanout](tasks-and-fanout.md) describes the model-first form of `tasks.fanout`.

### `models.use` takes a role name or a handle, and sampling options

`models.use('strong', { temperature = 0, max_tokens = 4096 })` selects the role and sets the options on the selection. `temperature` is a number from 0.0 to 2.0, `max_tokens` is a positive integer, and any other key raises an error. A handle saved from `models.current()` restores an earlier selection with its options.

The options are arguments of `models.use` because a prompt needs to set `max_tokens` section by section. The value is also arguably outside the prompt's contract, which the front matter states. A second call with a role name replaces the options, because a merge across calls would make a section's settings depend on its call history. The client sends a temperature as declared, and a provider's refusal of the value fails that round.

### A model handle is a value with fields only, and a `models` function takes it as its first argument

`models.use`, `models.get`, and `models.current` return a handle, a userdata over a frozen `ModelBinding`. Lua reads `name`, `label`, `model_id`, `description`, `context`, `provider`, `thinking`, `temperature`, and `max_tokens`. `provider` is the provider id that the Host's catalog names, or nil. A prompt writes a call on a handle as `models.infer(model, prompt)` or `models.loop(model, compactor?)`.

A handle has only its fields, so the `models` functions are the one way to run a model. The owner's reason: "Simpler implementation. Simpler mental model for authors. This is best for AI because only one way to do things."

### `models.get` returns a role's handle and leaves the current model as it is

`models.get('fast')` returns the handle of the `fast` role. A role name is a string that Lua passes as an argument. A name outside the roles raises an error that lists them.

A prompt names a model by its role, and the Host chooses the model that fills each role ([The Harness and the Host API](harness-and-host.md)).

## 3. `models.infer` and `models.loop`

### A model request starts from a Lua call

The Engine sends a request to a model when Lua calls `models.infer` or `models.loop`. A prose block sets the `prose` global, and Lua decides what to do with the text, as in `models.infer(prose)`.

Every request is an explicit call because a loop that the Engine started implicitly would hide model execution.

### `models.infer` sends one prompt as one round, on a fresh conversation with an empty tool list

`models.infer(prompt)` runs on the chain's current model, and `models.infer(model, prompt)` runs on a handle. The Engine issues one `Effect::Chat` with a single user message and returns the reply text. The call leaves `messages` untouched, so it works inside a tool handler and inside a compactor. A reply with tool calls fails with the code `MalformedResponse`, which a Host may retry.

### `models.loop` runs model rounds over the section's `messages` and returns nil

`models.loop(compactor?)` sends the section's `messages` to the chain's current model. When the model asks for tools, the loop runs the calls ([Tools and Packs](tools-and-packs.md)), appends an assistant record and the tool results, and sends the next round. When the model replies with text, the loop appends one assistant record and returns nil. The caller reads the reply as `messages[#messages].content`.

The call leaves the list implicit because a section has exactly one, so a list argument would have one legal value. It returns nil because `messages[#messages].content` is the final reply.

### Each round of a loop is a coroutine yield, so loops in fanout arms take turns

The loop yields each model request and each tool request from the block's coroutine. The scheduler runs other chains while a round waits, so several fanout arms can each run a loop and advance round by round. The rules of the loop are a Rust state machine that a Lua function steps ([The Engine](engine.md)).

The yields are in Lua because a loop written as async Rust on the scheduler's thread would make every other arm wait while one arm's loop waited on a model. The rules are in Rust so that typed tests can check them.

### A section runs one loop or one compactor at a time

A `models.loop` call that starts while a loop or a compactor runs in the same section raises an error of kind `lua`, whose text includes "ask a side question with models.infer(prompt)". Such a call can start from the handler of an adhoc tool ([Tools and Packs](tools-and-packs.md)) or from a compactor, and `models.infer` and `tools.call` work in both places.

The Engine refuses the nested loop because its records would precede the records of the outer loop's batch of tool calls.

### A prompt writes its own chat loop from `tasks.receive` and `models.loop`

A conversation with a person is a Lua loop in one section: `messages:user(tasks.receive())`, then `models.loop()`. The built-in chat prompt is that loop. A task that waits for follow-up messages loops over `models.loop()` and `tasks.reply` ([Tasks and fanout](tasks-and-fanout.md)).

## 4. Rounds, replies, and limits

### The loop judges each answer in a fixed order

The loop reads a round's answer in this order. For an overflow, the loop calls the compactor. For tool calls, it starts a batch of calls. For a text reply, it appends an assistant record and returns. For an empty reply that finished with `stop`, after this call answered a tool call, it returns as well. For any other answer, it raises an error of kind `empty_model_reply`.

A reply with tool calls starts a tool round whatever its finish reason, because llama.cpp and vLLM routinely finish a tool-call round with `stop`.

### An empty reply raises `empty_model_reply`, except after an answered tool call

A reply is empty when its text is blank and its list of tool calls is empty, even when it has reasoning text. The Gateway client fails such a round with the code `EmptyReply` and adds the provider's finish reason. The loop accepts the round as its normal end when the finish reason is `stop` and this loop call has answered a tool call. It then appends an assistant record with empty content.

A reply with only reasoning text is empty because the client refuses to promote reasoning into the answer. The exception exists because a prompt may ask only for tool calls: the model then does all its work through the calls and legally exits the loop with an empty reply.

### One `models.loop` call runs at most 24 rounds by default

The cap counts the rounds that one call starts, the final text round included. After the tool results of the last allowed round, the loop raises an error of kind `tool_loop_exhausted`. A `models.infer` round, the retry after a compaction, and a request sent again for a message are outside the count. A prompt sets its own cap with `max_tool_iterations`, from 1 to 1000, under the Host's ceiling ([Tools and Packs](tools-and-packs.md)).

### The loop appends queued messages before each request and reads the inbox once more after its final reply

A chain's inbox can receive a message while its loop runs ([Tasks and fanout](tasks-and-fanout.md)). Before each round, the Engine answers the loop with the queued messages, and the loop appends each as a user record under a line such as `This message arrived while you were working:`. After its final reply the loop reads the inbox once more, and a message there starts another round. A loop that has used its last round returns, and the message waits for the chain's next read.

The line marks each message as new input, so that the model reads it as separate from the task it started with. A loop that has used its last round skips the final read, because the read would take the message and the next round would raise `tool_loop_exhausted`. The message would then be out of the inbox and still unanswered.

### A message withdraws a model request that is still unanswered, and the loop sends it again

When the Engine delivers a message to a chain whose loop waits on a model request, it withdraws that request, even while the Host streams the reply. The loop appends the message and issues the request again. The withdrawn request leaves `messages` as it was and is outside the round cap. Tool calls in progress finish first.

The cap skips the withdrawn request because the request ended before it produced a round. The Engine delivers a message the same way every time: it always steers the loop that is running. A stop is a separate signal ([The Harness and the Host API](harness-and-host.md)).

## 5. Messages and the section's conversation

### The Engine creates one message list with each section VM and names it `messages`

`messages` is a Rust list that Lua reads as a userdata. A section VM has exactly one, and every `models.loop` call in the section runs over it. The `_G` guard, the Engine's metatable on the Lua globals table ([The prompt language](prompt-language.md)), returns that list on each read of `messages` and raises an error on an assignment. The list also stores the section's tool list from its first loop round ([Tools and Packs](tools-and-packs.md)).

Rust owns the list so that the Engine sees every append and every compaction. A section has one list because several lists would make it unclear which list owns the tool list, and whether a tool offer between two loops is a change.

### Every section entry starts with an empty conversation

Each entry into a section builds a fresh VM, so `messages` starts empty in a section that the walk enters, a `jump` target, a `call` child, a task, and a fanout arm. The preamble is a section entry too, so the section after it starts with an empty list. A library loaded with `require` uses the list of the section that loaded it. A prompt passes data between sections through `var` and the store ([The prompt language](prompt-language.md)).

The owner's rule: "the whole point of Runvil is that sections clear the history! if someone wants history preserved, they stay in the section".

### A section appends records, and a compactor rewrites them with `replace`

The methods `system`, `user`, `assistant`, `tool`, and `append` add one record at the end of the list and return the list. `messages:replace(first, last, records...)` swaps a range of records for other records. Outside a compactor it raises an error whose text includes "replace works only inside a compactor", and an assignment such as `messages[2] = x` raises an error too.

`replace` works only inside a compactor so that the request's prefix changes only at an explicit compaction. A request that repeats the first messages of the previous request can use the provider's prompt cache.

### Lua reads a record as a read-only view

`#messages` is the record count. `messages[i]`, `pairs`, and `ipairs` return each record as a view, a userdata with the fields `role`, `content`, `tool_calls`, `tool_call_id`, and `meta`. A write to a view raises an error. A view converts to the record's JSON form when Lua appends it or passes it to a tool.

The view is a userdata because a proxy table in its place would convert to JSON as an empty table.

### System records form the leading block of the list

The list refuses an edit that would put a system record after a record with another role. A list may start with several system records, and the projection, the step that converts the records into the request's messages, composes them into one system message for the request.

The owner's reason: "Because of prefix cache invalidation, changes to the prefix of the messages should be visible and explicit". A list that accepted a system record at any position, and merged the system records at the front for each request, would rewrite the prefix implicitly and discard the provider's prompt cache.

### The list checks a record when Lua adds it, and the projection checks the whole list before each request

At the builder call, the list checks one record's role, content, and tool fields, and drops any field outside a record's own fields. Before each request, the projection checks the rules that span records: every tool call id is unique, and one result for each call follows an assistant record's tool calls directly. It then builds the request's messages. It joins consecutive user records into one message, and it joins consecutive assistant records that hold text alone into one message. A text-only assistant record also joins the assistant record with tool calls that follows it.

The first check runs at the builder call so that a malformed record fails there, before `models.loop` runs. The projection enforces the pairing because OpenAI-shaped models reject unanswered tool calls.

### A record can have `meta`, which Lua reads and the projection omits

`meta` is any JSON value of at most 8192 bytes as compact JSON. Lua sets it as the last argument of a builder or as a field of an appended record, and reads it from a view. The projection sends `role`, `content`, `tool_call_id`, and `tool_calls` to the model. The records that the loop appends have plain text content and omit `meta`.

The field exists because a compactor needs hints that survive in the list, and each read of a record returns a new view.

### A Chat record logs the messages that are new since the list's previous send

When the Engine issues a round, it compares the request with the list's previous one. The effect's record names the earlier round as `after`, counts the equal leading messages as `keep`, and logs the messages that follow them ([The Engine](engine.md)). The Engine does this after the check against the context window passes, so the log contains only rounds that passed that check.

A record of the whole list for every round would make the run log grow with the square of the number of turns. The form with `after` and `keep` is less complex than exact deltas, and its price is a little extra log at each compaction.

## 6. The context window and compaction

### The Engine checks before each round of `models.loop` that the conversation fits the model's context window, and a `models.infer` request goes out unchecked

This check is the precheck. It estimates four characters per token plus four tokens for each message, adds a reserve for the reply, and compares the sum with the model's context window. The reserve is the binding's `max_tokens`, or one eighth of the window up to 8192 tokens, and at most half the window. When the chain's previous round reported token usage and the request extends that round, the count starts from the provider's numbers. A `models.infer` request skips the precheck and goes to the provider as written, and when the provider refuses it as too large, the call raises an error of kind `model_failure`.

The estimate omits tool schemas and image parts. A count for both is planned work.

### A compactor is a Lua function that edits `messages` in place under a snapshot

A request overflows when the precheck refuses it or when the provider rejects it as too large. The loop then takes a snapshot of the records and calls `compactor(reason)`, where the reason is `precheck` or `provider`. The compactor edits the list with `replace`. When it raises an error or leaves a list that breaks a rule, the Engine restores the snapshot and raises.

### `compactors.fail` is the default compactor, and `compactors.clear` empties the list

`models.loop()` uses `compactors.fail`, which raises an error of kind `context_exhausted` with the reason. An uncaught one ends the run with the code `ContextExhausted`. `compactors.clear` removes every record. A prompt runs it as a compactor, as in `models.compact(compactors.clear, reason)`, and a call from outside a compactor raises an error.

The Engine refuses `compactors.clear` outside a compactor because the snapshot that restores `messages` exists only while a compactor runs. A direct `compactors.fail(reason)` call in a block raises `context_exhausted` as well, and the run ends the same way.

### A compacted list must pass the structural rules, and after an overflow it must shrink

When a compactor returns, the Engine runs the projection's checks on the list: tool calls paired with results, unique call ids, and system records first. After an overflow, the list's token estimate must also be below the snapshot's. A failed check restores the snapshot. A structural failure raises an error of kind `lua`, and a list that failed to shrink raises `context_exhausted`.

An empty list passes. Stricter rules, such as unchanged system records or more records than the system block, would each refuse the result of `compactors.clear`.

### An overflowed round compacts once, and its retry is outside the round cap

After a compaction the loop sends the round again and leaves the round count as it was. When the retry overflows as well, the Engine restores the snapshot and raises `context_exhausted`. A retry that fails another way, such as a provider error or a stop, leaves the compacted records in the list.

The loop calls the compactor once for a round because a compactor can loop inside itself.

### `models.compact` runs a compactor on demand

`models.compact(compactor, reason)` calls `compactor(reason)` once over `messages`, outside a loop, with the same snapshot and restore. The reason is a string that the caller chooses, and only the structural rules apply to the result. The call raises an error while a loop or a compactor runs in the section.

The call and `compactors.clear` let a section start over or rewrite its history and then continue in the same section.

### A compactor may call `models.infer` and `tools.call`

Inside a compactor, Lua may ask a model for a summary with `models.infer` and may call a tool with `tools.call`. The built-in chat prompt's compactor summarizes older turns with `models.infer`.

A failed compaction restores the records and leaves a tool's side effects in place. The Engine allows the tool call because a refusal would only block a call that works.

## 7. The broker and the Gateway client

### The Gateway client always asks for a stream, and a Host that sets `RunSetup::streaming` receives the pieces as they form

The Gateway client sends every request with `stream` set to true and calls a callback with each piece of text or reasoning. The standard broker uses the Gateway client, and its `chat` hands each piece to the callback the Harness gives the round. A Host that shows a reply as it forms sets `RunSetup::streaming`, and the Harness hands that callback each piece of a section's own round ([The Harness and the Host API](harness-and-host.md)). The Harness asks the Host's `InferenceBroker` for the finished reply either way, and the record holds it.

The client has one transport path, the stream, because two paths would diverge.

### A Host swaps models during a run by sending a round to another model

The broker sets another model name on a round's options with `CompletionOptions::with_model`, and Workshop's broker reads the model dropdown's pick for each round. The Engine's binding is the one from the run's start, so the precheck uses the first model's window. Reply events and the answer record name the model that the broker sent the round to, and the effect record names the role.

The swap happens inside the run because a separate run for each pick would cost the conversation.

### The Engine defines the model vocabulary, and `harness-gateway-client` sends the HTTP requests

`runvil-model-client` defines `Message`, `ToolSchema`, `Completion`, `ModelBinding`, and the model error phrases. `harness-gateway-client` contains the HTTP and the wire parsing: the chat client, the standard broker, the fetch of the model list, and public wire functions such as the one that builds the request body. The Engine's projection also writes wire format: it writes each tool call of an assistant record in the OpenAI function-call shape. The wire functions are public so that another broker can reuse them.

The client is a crate of its own because the Host supplies inference and the Harness excludes tokio, so the Harness leaves HTTP to the Host, and a Host that uses the Gateway still needs a standard Gateway client.

### The client sends tools in the OpenAI function-calling format

The client writes each tool as an object of type `function` with a name, a description, and JSON Schema parameters, and sets `tool_choice` to `auto`. A reply is text or a list of tool calls: `CompletionResult::Text` or `CompletionResult::ToolCalls`. One function builds the request body, and the client's `AGENTS.md` makes that function "the only place the chat-completions body is shaped". A model with another tool format is the Gateway's concern.

### One function normalizes every reply, and reasoning text is separate from the answer

One function in the client reads the assembled reply and returns text or tool calls, the finish reason, and reasoning text. It reads reasoning from the first non-empty field among `reasoning_content`, `reasoning`, and `thinking`.

The function's module concentrates the wire quirks so that the rest of Runvil works the same across models. A new special case belongs in this module. Its growth is acceptable, and scattered special cases are the risk.

### The Engine's constructors check every reply, whichever broker built it

`ToolCall::from_parts` and `Completion::from_result` refuse a blank call id, a blank name, arguments of any JSON type other than an object, and two calls with one id. Checks of the wire format are in the client: the `[DONE]` sentinel, the byte cap, and a batch of tool calls cut off by a `length` or `content_filter` finish.

The checks that are independent of the wire format are in the Engine so that they run for every broker. The client fails a cut-off batch so that only calls with complete JSON arguments run.

### The receive timeout restarts with each chunk, and the client owns the request limits

The client waits at most 120 seconds for the response headers and then for each next chunk, and it accepts at most 16 MiB for one reply. A stream that continues to send data completes, and a silent stream fails with the code `Timeout`. The client sets the limits together, and the standard broker uses the defaults.

The timeout restarts with each chunk because a cap of 120 seconds on the whole reply would end a long thinking turn.

### A model failure's prose is a fixed phrase, and provider text is only in the message

A failed round is an `Error` with a model code such as `Overloaded`, `Timeout`, or `ContextOverflow` ([The Engine](engine.md)). Its `prose` field, the text that a model reads, is the code's fixed phrase, with ` (status N)` appended for an HTTP status. The client appends the provider's escaped text to the error's message, and Lua receives the prose. In Lua a caught failure has the kind `model_failure`, and a round on a chain that lacks a model has `model_required`.

Lua has these kinds because a single kind, `internal`, would cover unrelated conditions, and a model that reads a caught 503 would see `internal`.

### The client redacts its key and builds a keyless client for a loopback Gateway

The client stores the Gateway's bearer key as a secret value and prints `<redacted>` in its debug output. It cuts a backend's error body and escapes the control characters, and the code's docs say why: "The escaping stops a backend body from forging log lines". For a chat request it reads the body up to the reply cap, 16 MiB by default, and keeps the first 2000 characters. For the model list it stops reading at 2000 bytes. The client's constructor from the environment reads `RUNVIL_GATEWAY_URL` and `RUNVIL_GATEWAY_API_KEY`, and builds a keyless client when the URL names a loopback address and the key is absent.

The client has the Gateway's key, and the Gateway has the model provider's key. A Gateway can admit a loopback caller that omits the key, and the keyless client is for that case. The keyless form exists for the chat client alone. The standard broker, the fetch of the model list, and `provide_gateway_resources` each require a key, so a Host that uses them needs the Gateway's key for a run, on a loopback Gateway too.

Beside its own requests, the client hands the key out only through the entry that `provide_gateway_resources` offers the Plugins of the C contract. That entry holds it as plain text, because such a Plugin presents the key to the Gateway itself ([The Harness and the Host API](harness-and-host.md)).

## 8. Planned work

- **A model for each role.** A model that the Host selects for each role. `RunSetup` has a `selected_model`, and the Harness passes that model as `default`, so it fills every role.
- **A model swap that the Engine knows about.** A swap that also changes the Engine's binding for the role. After a swap by the broker, the Engine's precheck, `sys.model`, and the request's thinking switch and token cap describe the first model.
- **A scoped model switch.** A Lua form that switches the model for one block of work and restores it afterward. `models.use` switches the model for the rest of the chain, and a prompt restores the earlier model with a handle saved from `models.current()`.
- **A role for compaction.** A role beside the fixed set that names the model for compaction. A compactor calls `models.infer` on the chain's current model or on a handle, and the item waits for evidence of what compaction needs.
- **Tool schemas and images in the precheck.** A token count for tool schemas and image parts. The estimate omits both, and the item waits until retries overflow at the provider or image summaries fail the shrink rule.
- **Tool ids in records.** A record whose tool call names the tool by its id, such as `web/fetch`, as Lua does. The record has the wire name, such as `web_fetch`.
- **A log entry for a compaction.** An entry in the run log for each compaction. The log omits compactions, and the item waits for a debugger.
