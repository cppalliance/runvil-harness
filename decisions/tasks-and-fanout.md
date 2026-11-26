# Tasks and fanout: design decisions

This page covers concurrent work inside a run: tasks, fanout, the scheduler's order, limits, determinism, and a task's inbox. A chain is one walk through a prompt's sections. A run has a chain for the main walk, one for each `call`, one for each `execute`, and one for each task, and a task is a chain that runs beside the chain that spawned it. The Engine's scheduler runs the chains in turns on one thread. `tasks.spawn` starts a task, `tasks.join_any` waits for one, and `tasks.fanout` is a Lua function that spawns one task for each member of a table.

## 1. Chains and the scheduler

### Chains take turns on one thread, first in, first out

The scheduler resumes one chain's Lua coroutine at a time. The chain runs until its Lua yields a request, such as a model call, a store call, or a wait. The scheduler then takes the next chain from its queue of ready chains, first in, first out. A request for outside work becomes an effect that `Run::step` returns ([The Engine](engine.md)), and the scheduler itself answers a request about another chain. A long Lua loop between two requests therefore blocks every other chain.

One thread is enough to dispatch every chain, because a fanout spends most of its time waiting for inference.

### The scheduler itself makes each write that spans its task records, its queues, and its slots

The scheduler keeps one record for each task, and through these records it checks ownership and enforces the task lifecycle. The records are permanent, so a late `tasks.status` still reports how a task ended. A separate part of the scheduler owns the queues of waiting chains and the set of chains that have slots (see Admission and limits). An operation that writes to several of these parts belongs to the scheduler as a whole, and it reports a subtree's events before its owner's terminal event.

These operations belong to the scheduler as a whole because its parts depend on each other through the order of these writes. The queue of ready chains belongs to the scheduler directly, because first in, first out is its only rule. [The Engine](engine.md) describes the scheduler's other parts.

## 2. Tasks

### `tasks.spawn` starts a chain over a section and returns a handle at once

`tasks.spawn(target, opts)` queues a chain that starts at the target section, and returns before that chain runs. `opts.args` gives the chain its own arguments, a table of named fields or a string, by the rule of the second argument of `call` ([The prompt language](prompt-language.md)), and a chain spawned with that key left out reads its spawner's `args`. `opts` takes the keys `args`, `item`, and `index`, and any other key raises an error that names it, so a misspelled option fails at the call. The handle is a plain table, `{ task = id }`, and every `tasks` function that names a task takes the handle or the id string. A handle, a set of tasks, or an `opts` table that is a weak table raises an error, and so does one whose `__index` leads to a weak table for the field that the Engine reads ([The prompt language](prompt-language.md)). The functions of the `tasks` table are `spawn`, `join_any`, `join`, `ready`, `status`, `pending`, `note`, `cancel`, `concurrency`, `receive`, `send`, `reply`, and `fanout`.

### The chain that spawns a task owns it

A task belongs to the chain that spawned it. A `jump` or a fall-through to the next section leaves the task running. The preamble is the Lua before a prompt's first heading ([The prompt language](prompt-language.md)). It and the sections after it are entries of the main walk, so a task that the preamble spawns belongs to the main walk, and a later section can wait on it. Only the owner may wait on, send to, or cancel the task, and any other chain receives a `task_not_owned` error. A chain may also call `tasks.status` and `tasks.note` for the task it runs inside, whose id is `sys.taskid`.

A task is bound to the lifetime of `var`, the table that the sections of one chain share. Authors know that rule from `var`, and it matches the scheduler's record of chains exactly.

### `tasks.join_any` is the one wait in the Engine, and `tasks.join` is Lua over it

`tasks.join_any(set)` suspends the caller until one task in the set ends or replies. `tasks.join(set)` is a Lua loop over `join_any` that returns one entry for each member. A wait takes each result once, and a second wait for the same result raises `task_consumed`. `opts.timeout`, in seconds, starts a timer that the wait cancels itself.

The names state the contract: after a join, the task's writes are visible to the caller. Each wait returns a member's failure as data. A `tasks.join` that raised on a member's failure would have to cancel the other members or leak them, and each choice is wrong for half the callers.

### A chain that ends with a running task fails with `tasks_live`

When a chain ends normally while a task it spawned is still running, the chain's result becomes a `tasks_live` error that names the task. The Engine ends the task with the state `abandoned`. A task that its owner stops with `tasks.cancel` has the state `canceled`. When the owner ends, the Engine also abandons an idle task, one that waits after `tasks.reply`, and leaves the owner's result unchanged.

The owner's rule: "subagents spawned by Lua become a hard error on chain termination". A task left running at its owner's end is a bug in the prompt. An idle task is exempt from the error, and the guide states the reason: "It already handed its owner a reply, so an owner may end while it has idle tasks". The end states `abandoned` and `canceled` are separate because a task that lost its owner and a task that someone stopped are different facts for the log and the UI.

### Every started task reports one start event and one terminal event

The scheduler emits `TaskStarted` at admission, the moment it starts the task's queued chain, under the section that spawned the task. The event's `args` field has the chain's own arguments as JSON, a string as an object with the one field `text`, and it is null for a chain that reads its spawner's `args`. One terminal event follows: `TaskSucceeded`, `TaskFailed`, `TaskCanceled`, or `TaskAbandoned` with an `AbandonReason`. The scheduler skips the start event and the terminal event for a task that ends while still queued.

When a run ends as a whole, by a cancel or a fatal answer, the scheduler first abandons every live task. The reason is `run_terminated`, or `owner_aborted` for a task nested under another. If the scheduler skipped this step, `TaskStarted` would be the last event of such a task, and its record would read `running` forever.

## 3. Fanout

### `tasks.fanout` is a Lua function over `spawn`, `join_any`, and `cancel`

`tasks.fanout(worker, collection)` runs the worker section once for each member of the collection, and each run is an arm. The function is Lua, and it yields the same requests that `tasks.spawn`, `tasks.join_any`, and `tasks.cancel` yield, so every concurrency rule in the Engine is a rule about tasks.

### The collection is a Lua table, and `tasks.fanout` lists its members in a fixed order

`tasks.fanout` lists the items of the list in index order, from index 1 up to the first index that holds nothing, and then every other entry sorted by key: whole numbers from 1 up, then booleans, then the other numbers, then strings.
An index past such a gap is a keyed member, and it comes before a boolean key.
So the arms of a collection come in the order in which `pairs` visits its keys ([The prompt language](prompt-language.md)).
A `__pairs` metamethod of the collection changes what `pairs` visits and has no part in the fanout, which reads the table itself.
It reads a view through to its data, so a list from `args` or from `var` is a collection like any table. A keyed member becomes a pair table that the arm reads as `item.key` and `item.value`. Any other value in place of the table raises an error that points to `prompt.section(path, 'list').items`.
A weak table, one whose metatable has a `__mode` with `k` or `v` in it, raises an error too.

A heading in place of the table is one of the refused values. `prompt.section(path, 'list').items` returns the items of a list in a section, so a heading as the argument would be a second way to do one thing, and a bare string is ambiguous against a section name. The sort of the keyed part makes every process spawn the arms in the same order.
It is the order of `pairs`, so a block that walks a table and fans out over it meets the entries in one order.
The list ends at the first gap because the length that Lua gives a table with a gap differs between two runs ([The prompt language](prompt-language.md)).
Lua's collector can take a member out of a weak table while `tasks.fanout` reads it, and which members it takes follows the order of Lua's own walk, so two runs with the same inputs would spawn different arms.

### Each arm is a task that runs the worker section with `item` and `sys.index`

An arm's chain starts at the worker section in a fresh Lua VM. The member is the global `item` and the value of `{{ item }}` in prose, its 1-based position is `sys.index`, and `var` is a copy of the caller's. The worker section can do what any section does, including `call`, `jump`, and a nested `tasks.fanout`.

An arm is normal flow: its chain follows the rules of every other chain.

### `tasks.fanout` spawns every arm before it waits for any

`tasks.fanout` makes a `spawn` request for every member before its first `join_any`.

Under the rule that every delivery is a join (see Determinism), an arm spawned after a join would be ordered after the arm that the join returned and could read that arm's store files, so its result would depend on timing. With every arm spawned before the first join, the arms are unordered among themselves.

### Results return in collection order, each with `text`, `ok`, `item`, and `exhausted`

`tasks.fanout` returns an array with one result at each member's position. `text` is the arm's final text, `ok` says whether the arm completed, `item` is the member, and `exhausted` says whether the arm ran out of model rounds. Each result is a read-only view: `pairs` walks its fields as it walks a plain table, and an assignment to a result raises an error that reads "tasks.fanout results are read-only".

The caller combines the results itself. `item` lets the caller match each result with its member when the member is a rich value, such as a record.

### The first failing arm cancels the others, and an exhausted arm becomes a stub

When an arm fails, `tasks.fanout` cancels the arms still live and raises that arm's own error at the `tasks.fanout` call. An arm that fails with `tool_loop_exhausted` is the exception: its position gets a stub text, `ok` false, and `exhausted` true, and the other arms continue, so that one stuck arm leaves the evidence of its siblings intact. An empty collection raises an error before any spawn, and the error's text states the reason: "tasks.fanout over an empty collection: no work is likely a bug".

### The model-first form runs every arm on one model

`tasks.fanout(model, worker, collection)` takes a model handle first. The function sets the caller's current model to that handle, spawns the arms, and restores the caller's model before the first wait. Each arm copies the model at its spawn ([Model calls and conversations](models-and-conversations.md)).

The form exists only in the Lua of `tasks.fanout`: every form of the call yields the same requests to the scheduler. It spares authors a manual save and restore of the model.

## 4. Admission and limits

### A default limit of 8 bounds the tasks that run at once in the whole run

`RunLimits::max_concurrency` is the most tasks the scheduler admits at once. The default is 8, and the Host sets it. A running task counts against the limit of its owner and of every ancestor chain, and each such count is a slot. A fanout inside an arm therefore runs within that arm's share.

One limit covers the whole run. A limit inside each fanout would multiply when fanouts nest, since 8 arms that each run 8 arms are 64 live tasks, and it would leave tasks from `tasks.spawn` unbounded. The Harness has a separate limit on Pack tool calls in flight, 10 by default ([Tools and Packs](tools-and-packs.md)), because one shared limit would couple the task limit and the call limit.

The value 8 is arbitrary, here and for `max_idle_tasks` ([The Engine](engine.md)). The value is unmeasured, and the owner did not pick it. The owner's rule is that each resource has a limit, and the value is secondary. The owner's view of the value: "8 sounds good". A value may change freely, because, in the owner's words, "this thing's not in production yet".

### A spawned task starts at admission, in spawn order, after the chains that resume

A spawn forks the file identity, assigns the task id, and queues the chain. Admission is the moment the scheduler starts a queued chain. The chain gets its Lua VM then, and until then `tasks.status` reports `blocked` as `queued`. The scheduler runs an admission pass each time its queue of ready chains is empty. The pass admits chains that resume from a wait first, then spawned chains in spawn order.

A queued task costs very little, so the arms of a fanout over 500 members have at most as many Lua VMs at one time as the limit allows. The pass admits resumed chains first, which protects a resume from starving behind a long queue of spawns. Admission order affects timing only: the ids and each store's decisions on conflicts (see Determinism) are the same under every admission order. A collection may be as large as the author likes, because a large collection only takes longer, and whether that is worth it is the author's call.

### A task that waits on tasks or on its inbox releases its slots

A task that waits in `tasks.join_any` or `tasks.join` releases its slot at its owner and at every ancestor, and takes the slots again before it resumes. A task that waits on its inbox does the same. A task that waits on a model call or a tool call still counts against the limit.

The release prevents a deadlock: a full set of outer arms that wait on their own inner arms would otherwise fill every slot.

### `tasks.concurrency` sets a chain's limit and clamps it to the parent's

`tasks.concurrency(n)` sets the limit for the tasks the calling chain spawns from then on and returns the limit in effect. The scheduler clamps `n` to the limit of the enclosing chain, or to `max_concurrency` on the main walk.

The scheduler clamps a value above the limit, where it could raise an error, so that a prompt is portable wherever it runs, whatever the limit. The function is named after tasks because tasks are what it limits.

### `call`, `execute`, `tasks.spawn`, and fanout arms share one depth cap of 8

A `call` child, an `execute` child, and a spawned task each run one level deeper than the chain that started them, and the depth cap is 8. The start that would run at level 9 raises an error with the code `call_depth_exceeded` and the text "recursion exceeded cap of 8".

A fanout arm counts as a level, so the depth of a recursion accumulates across a fanout, where it would otherwise start again in each arm. An `execute` counts as a level too, so the cap also ends a recursion between prompts that execute each other ([The prompt language](prompt-language.md)).

The cap is 8 because that is more depth than the owner expects a prompt to need ([The Engine](engine.md)). The owner compares it with Cursor, which the owner recalls as allowing about five levels, an unchecked figure, and says of nesting there: "I've only ever gone to three." The cap is a constant, where the other limits are fields of `RunLimits`, because the one value has been enough for every Host so far. The owner's reason: "I just don't see a need for it yet."

## 5. Determinism

### The same seed, start time, and answers produce the same run

For each run the Harness generates a random seed, reads the clock, and passes them to the Engine in `RunParams`. With the same seed and start time, and the same answers, messages, and file requests delivered in the same order at the same steps, and with the cancel flag clear, the Engine emits the same effects, events, and ids. The order of answers is an input. So is the order of the file requests that the Harness reports for open tool calls, each of which the Engine issues as an effect at its next step ([The Engine](engine.md)). So is each message that a Host sends the run, with its place among the answers and the steps ([The Harness and the Host API](harness-and-host.md)). Among waiting tool calls, the Harness starts the one with the lowest effect id ([Tools and Packs](tools-and-packs.md)), and `Run` hears of a call again only at its file requests and at its answer. That choice therefore affects the Engine only through the order of answers and of file requests, so a replay (see Planned work) that repeats the answers, the messages, and the file requests in their order and at their steps is independent of the Harness's start order. The results that depend on the order of answers are which task `tasks.join_any` returns, whether a timed wait runs out, how far a canceled task got, and, as the guide adds, where a `tasks.send` message applies in a task's work.

Because of this rule, replay is a job for the Harness: the Engine is deterministic, so the Harness can match a recorded run exactly. The rule covers Lua's table walks too. The Engine's `pairs` and `next` walk a table in sorted order, and refuse a table with a key that only its memory address could place, where the order of stock Lua's walk depends on a hash seed and on such addresses.
The length of a table with a gap in its list is outside the rule: `#t` of such a table can differ between two such runs, and so can what `table.insert`, `table.remove`, `table.concat`, `table.unpack`, and `table.sort` do with it ([The prompt language](prompt-language.md)).
The Engine seeds `math.random` from the run's seed, its `table.sort` orders by the list and the comparison alone, and its `tostring` and `string.format` print a table, function, thread, or userdata by its name, where stock Lua's read the clock or print a memory address ([The prompt language](prompt-language.md)).

### Chain ids, task ids, and `sys.id` are dot-separated paths

A chain's id is its parent's id followed by a number from the parent's own child counter. The main walk is `0`, its first child is `0.0`, and a task's id is the id of its chain. `sys.id` is the chain's id followed by a number from the chain's entry counter, which also starts at 0, so the first section that the main walk enters reads `0.0`. Each counter belongs to one chain, so the ids are the same in every run, whatever order the chains finish in.

An id is a path of numbers, and paths can name any number of arms. The number of a fanout's arms is unbounded, so a packing of the numbers into one fixed-width integer would have an overflow rule to document and test.

### A spawn is a fork, and every delivery is a join

Each task has a file identity, an `ExecId`, under which a store records the task's reads and writes. A spawn orders everything the parent did before it ahead of the child's first step. A delivery orders everything the task did ahead of the owner's next step: `join_any` joins the task it returns, `join` joins each member, and a chain's end joins every task that the chain owns. A store uses this order to decide whether two accesses conflict ([Files and the VFS](files-and-the-vfs.md)).

The result is one rule: after a join, a chain can read what the task wrote to the store. A wait that returned a task unjoined would be a second concept, and a trap: a caller could take a task's result and leave the task unjoined. Determinism here means that a store's decisions on conflicts and the contents of its files depend only on program order and on explicit points of synchronization, and are independent of scheduling. The claims check covers a store alone: two tasks may write one of the Host's files through their tool calls, and the later write is the content.

## 6. Inboxes

### Every task has an inbox, and `tasks.receive()` reads it

An inbox is a queue of text messages, oldest first. The Engine has one for the main chain, which is the main walk, and one for each task. `tasks.receive()` returns the oldest message, or suspends the chain until a message is queued. A `call` child and an `execute` child read the inbox of the task they run inside. A chain inside `models.loop` reads its messages before its next model request ([Model calls and conversations](models-and-conversations.md)).

The inbox functions are members of `tasks` because `tasks.send` is there, and because `tasks.note` is a member that also acts on the caller's own task.

### The Host queues messages for the main chain, and a task's owner queues messages for the task

`Run::deliver` queues a message for the main chain, and the Harness calls it for each message the Host sends ([The Harness and the Host API](harness-and-host.md)). `tasks.send(task, text)` queues a message for a task that the caller spawned. It returns true, or false when the task has ended, and it raises `task_not_owned` for any other task.

The Host's `RunControl` sends only to the main chain: a sub-agent is a task in the same run, and only a run has a `RunControl`. When every chain that remains waits on a task or on a task's inbox, the run ends with the error `stalled` ([The Engine](engine.md)).

### `tasks.reply` publishes a result, and the task then waits for its owner's next message

`tasks.reply(value)` publishes `value` as the task's latest result and then waits on the task's inbox. The task is then idle: its Lua state and its conversation are intact, `tasks.status` reports `blocked` as `inbox`, and the owner's next `tasks.send` resumes it.

A task can therefore return a result and remain open for follow-ups with its conversation intact, up to the cap on idle tasks in the next entry. `tasks.fanout` cancels each arm after it takes the arm's result, so an arm that replied ends as `canceled`. It cancels the arm whether the arm is live or has ended, since a cancel of an ended task is free and silent.

### A run has at most 8 idle tasks by default, and one more reply closes the one idle longest

`RunLimits::max_idle_tasks` is 8 by default. A reply that would put the run past that cap closes another idle task: the one idle longest among those whose own tasks have all ended, or the one idle longest of all. The closed task ends as `done` with its latest reply as its result.

The cap bounds memory: an idle task still has its Lua VM, which the Engine is unable to rebuild from the task's conversation. The closed task ends as `done` because its owner is still alive and can still take that reply, while `abandoned` means that the owner ended. The value 8 is arbitrary, as it is for `max_concurrency`.

## 7. Planned work

- **Replay and resume.** A Harness driver that builds `RunParams` from a recorded run's seed and start time and repeats the recorded answers, messages, and file requests, and a way to spawn a canceled task again under its original id. Today the Harness builds `RunParams` for every run from a random seed and the clock, and a spawn assigns the next child id.
- **A read of task history.** A Lua call that returns the events that a task has reported. The design waits for a concrete use, and today a prompt reads a task's state at the time of the call with `tasks.status`.
- **Task tools for models.** Tools with which a model starts tasks itself, as the tools of the Pack `runvil/subagents` ([Tools and Packs](tools-and-packs.md)). Today the Pack reserves the name, and only Lua starts a task, with `tasks.spawn` or `tasks.fanout`.
- **Inbox messages other than text.** Images, files, audio, and video as inbox messages. Today an inbox message is a text string.
