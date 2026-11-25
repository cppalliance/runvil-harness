---
name: Deterministic Lua, the follow-up
overview: "A table that the Engine converts to data has no gap in its list, is not weak, nests at most 127 deep, and holds no more text than the VM's memory limit, with every string key a name; `tostring`, `string.format`, `table.concat`, and `table.sort` are one frame with no position to the author's code, as the stock functions are; an argument error names a function that the Engine wrote in place of a stock one as the author's call is written; `tasks.fanout` lists the keyed members of a collection in the order of `pairs`; every Engine function that reads the entries of a table refuses a weak one; a failure text loses the position of an Engine chunk; and the whole of `var` nests at most 127 deep, however many writes made it. A read of one field that the Engine makes of the author's table refuses a weak table that an `__index` leads to, a record holds no more text than one conversion spends, and the Engine tells its own `var` and a library that still loads by identity, which the review of the pull request asked for. Closes issues #63, #66, and #69, which the plan `2026-11-19-1-deterministic-lua.plan.md` left open."
todos:
  - id: plan-copy
    content: "runvil-harness: this plan goes to `plan/2026-11-23-1-deterministic-lua-follow-up.plan.md` in the first commit of the change, before any other change"
    status: completed
  - id: replacement-frame
    content: "runvil-lua: `tostring`, `string.format`, `table.concat`, and `table.sort` call a metamethod or a comparison function from their own body, with no protected call around it and no debug info, and take the list that the stock function takes"
    status: completed
  - id: failure-label
    content: "runvil-lua: the rule on an address reads what follows the `runtime error: ` that `mlua` wrote, and takes the same words anywhere else as text"
    status: completed
  - id: weak-readers
    content: "runvil-lua: `json.encode`, `tools.offer`, `tools.adhoc`, `models.use`, and the conversion of a table to data refuse a weak table"
    status: completed
  - id: weak-metatable
    content: "runvil-lua: every read through a view refuses a weak table and a table under a weak metatable before it reads an entry or asks the metatable for a field"
    status: completed
  - id: mode-budget
    content: "runvil-lua: the bytes of a `__mode` that the test of a weak table reads count against the budget of the conversion, and against the VM's memory limit for one call of `pairs`, `next`, `tasks.fanout`, or `tools.offer`"
    status: completed
  - id: fanout-order
    content: "runvil-lua: `tasks.fanout` lists the keyed members of a collection in the order `pairs` visits their keys"
    status: completed
  - id: offer
    content: "runvil-lua: `tools.offer` measures an array with no `__len` by the entries the table itself holds from 1 to the first it lacks, and refuses the key outside the array that comes first"
    status: completed
  - id: adhoc-keys
    content: "runvil-lua: of several unknown keys of the attributes of `tools.adhoc`, the refusal names the one that comes first in the shared order of keys"
    status: completed
  - id: data
    content: "runvil-lua: `data_of` in the new `data.rs` makes every conversion of a Lua value to data, refuses a list with a gap, a weak table, and a value nested more than 127 deep, and names the fault of the entry whose key comes first"
    status: completed
  - id: record-limits
    content: "runvil-lua: the parse of a record refuses one whose JSON form nests more than 127 deep or holds more values than one conversion may spend"
    status: completed
  - id: data-text
    content: "runvil-lua: the conversion of a value to data counts the bytes of every string, keys included, and a path of `var` quotes at most 64 bytes of a key"
    status: completed
  - id: quoting
    content: "runvil-lua: a path and a refusal of `tools.adhoc` quote at most 64 bytes of a key, a name, or a type"
    status: completed
  - id: raised-fields
    content: "runvil-lua: the read of a raised error table asks for the fields of its code by name, so it passes over an entry that is no field, where it failed at the first one Lua's own walk met, and copies no other entry"
    status: completed
  - id: plain-json
    content: "runvil-lua: `plain_json.rs` reads a Lua value and JSON text as data with every key a name, where `serde_json` reads the key `$serde_json::private::RawValue` as JSON text in a build with its `raw_value` feature on"
    status: completed
  - id: engine-position
    content: "runvil-lua: a failure text loses the position of a chunk of the Engine, or `?:?:`, in front of it, where the text leaves the crate and where the Engine's `pcall` hands it to the author's code"
    status: completed
  - id: concat-abort
    content: "runvil-lua: `table.concat` starts again only on the stock function's own refusal, and raises any other failure as it is"
    status: completed
  - id: var-depth
    content: "runvil-lua: a write to `var` counts the levels of the table it writes to, so the whole of `var` nests at most 127 deep, a guarded table keeps one step of its path, and a write builds a path only to refuse"
    status: completed
  - id: var-key
    content: "runvil-lua: a write to `var` refuses a string key that is not UTF-8"
    status: completed
  - id: string-metatable
    content: "runvil-lua: `install` takes the metatable of strings from the VM once, keeps it with the names of the events that the captures read, and hands it to the chunk of the replacements, so no call reads it through a push that has no protected call around it"
    status: completed
  - id: chunk-names
    content: "runvil-lua: the chunk of the `__concat` of an error value has the name of an Engine chunk and is built when the VM is made, and a test holds each of the ten chunk names of the Engine to a name whose positions the position rule reads and to 59 bytes"
    status: completed
  - id: weak-tasks
    content: "runvil-lua: the `tasks` functions refuse a weak set of tasks, a weak Task handle, and weak options before each read of the table, and call no global that a block can replace"
    status: completed
  - id: weak-field-reads
    content: "runvil-lua: a read of one field of the author's table, by a `tasks` function, `tools.offer`, the lazy `prose`, and `models.loop`, refuses a weak table that the `__index` of the table leads to, as far as Lua follows the read, and one call counts the tables that its reads follow against the budget of the call"
    status: completed
  - id: record-bytes
    content: "runvil-lua: the parse of a record refuses one whose JSON form holds more bytes of text than one conversion may spend"
    status: completed
  - id: identity
    content: "runvil-lua: the read that tells whether a block reassigned `var` compares the global by identity, and `require` marks a load under way with a function, so neither runs an `__eq`"
    status: completed
  - id: engine-reads
    content: "runvil-lua: the read that tells whether a block reassigned `var` is a raw read of the globals, and the chunk of `require` takes no function of a library, since a Pack prelude can set a field of each library table that it shares with the VM"
    status: completed
  - id: pcall-arguments
    content: "runvil-lua: the Engine's `pcall` and `xpcall` refuse their own arguments in stock Lua's words, on the line of the call"
    status: completed
  - id: argument-names
    content: "runvil-lua: an argument error of `tostring`, `string.format`, `table.concat`, `table.sort`, `math.randomseed`, `pcall`, `xpcall`, `setmetatable`, and `getmetatable` names the function as the author's call is written, by one capture that reads the call from Lua's stack"
    status: completed
  - id: guard-debug-info
    content: "runvil-lua: the chunk of the guard of the globals has no debug info, so a failure that Lua raises in it names no variable of the chunk"
    status: completed
  - id: capture-calls
    content: "runvil-lua: a capture of the replacements reads a metamethod by a name made once, so it makes no protected call and no Lua string for a table, and a metamethod that calls the replacement again fails with Lua's own `C stack overflow`"
    status: completed
  - id: meta-names
    content: "runvil-lua: a read through views asks a metatable for `__mode`, `__metatable`, and `__index` by a name that its walk, or the installed function it serves, makes once, so the test of a weak table adds no protected call to the read of a table"
    status: completed
  - id: caller-names
    content: "runvil-lua: an argument error names a replacement that another replacement runs as a metamethod by the library, as under the C function, and one that the guard of the globals runs for a handler that is a table by the event"
    status: completed
  - id: failure-text
    content: "runvil-lua: a failure whose message or traceback holds bytes that are not UTF-8 fails the block with its own text, and the quote of a long key or name ends within three bytes of its 64th, with the length of the author's bytes"
    status: completed
  - id: build-under-limit
    content: "runvil-lua: a new VM grows the stack that `mlua` keeps the Lua values of Rust in before the run's memory limit is set, and `install` makes the one function that names the type of a refused argument, so the build of a VM that does not fit its limit fails with `not enough memory` while what it holds fits the 68 slots of that stack, as 36 tables inside `args` and `var` with no Pack prelude do, where the 34 slots of `22a0992f` take 10"
    status: completed
  - id: pages
    content: "decisions: `prompt-language.md`, `models-and-conversations.md`, `tasks-and-fanout.md`, `tools-and-packs.md`, and `engine.md` state the new rules and their reasons"
    status: completed
  - id: guide
    content: "runvil-docs: chapters 3, 4, 5, 6, 7, 8, 10, 11, 12, 14, 15, 16, and 17 of the language part state the same rules"
    status: completed
  - id: verify
    content: "runvil-harness: run the checks in AGENTS.md on the final tree, all but `cargo dylint`, which the machine of the run does not have"
    status: completed
isProject: false
---

# A table as data, the replacements as one frame, and one order of keys in a section's Lua

Basis: `runvil-harness` `22a0992f`, which is `master` one commit after the thirteen commits of the first plan, `22693b33` to `9ba47f6e`; that commit changes the lock file alone.
The first plan is `plan/2026-11-19-1-deterministic-lua.plan.md`, and this plan states only what changes after those commits.
`bdcadfbb` below is the commit that [pull request #56](https://github.com/cppalliance/runvil-harness/pull/56) put those thirteen commits on; `master` was 46 commits past it when they landed.
A part of the change was built on the head of that pull request, and was moved to `22a0992f` with no conflict and no line of it altered.
Every number below was measured on one of those two trees, on macOS on Apple silicon, in a debug build where no build is named.
The Lua in the build is 5.5.0, which the lock file pins through `lua-src` 550.1.1.
The commits of the change were made on `5d9c7bf8`, which is `master` eleven commits after `22a0992f`, and [pull request #78](https://github.com/cppalliance/runvil-harness/pull/78) had them at `4fdbe940`.
None of the eleven changes a file of `runvil-lua`, and the two decision pages that they and this change both edit merged with no conflict.
The commits now go on `92abc305`, which is `master` twelve commits after `5d9c7bf8`.
None of the twelve changes a file of `runvil-lua`, and no line of a commit of the code was altered in the move.
Three decision pages that the twelve and this change both edit conflicted, in the commit of the pages, and the words of this change on those pages that give a count of things in the code were written again without one, by a rule that the twelve bring.
The review of that pull request found three faults in `4fdbe940`, and the three commits that end them were made on `92abc305`.
The commits now go on `cc55a351`, which is `master` five commits after `92abc305`.
Two of the five change `runvil-lua`, and three commits of the code were altered in the move for them: the levels of a write to `var`, a weak table in every reader, and the unknown keys of `tools.adhoc`.
`c235fcd6` holds `var` to 127 levels: it gives each table of `var` its level, and a write counts the levels of the converted value a second time and adds them to that level.
The commit on the levels of a write takes that second count out, since the conversion counts from the level of the table written to, and the two tests of `c235fcd6` pass on it.
`cc55a351` reads the attributes and the parameters of `tools.adhoc` through a view, such as a table read from `var` or `args`.
The commit on a weak table in every reader makes that read with the refusal of a weak table on its way, and the commit on the unknown keys walks the table that the view stands for.

## The problem

Three issues stayed open when the commits of the first plan went to `master`:

- **A table with a gap converts to two values.** `mlua`'s conversion of a table to data takes the length of a table as the count of its items.
  `{ [1] = 1, [2] = 2, [3] = 3, [5] = 5 }` became `[1,2,3,null,5]` in 12 of 24 fresh VMs and `[1,2,3]` in the other 12.
  The arguments of `tools.call`, the `item` of a spawn, a field of a message, and a global in prose all go through it.
  [Issue #63](https://github.com/cppalliance/runvil-harness/issues/63) reports it.
- **The author's code under a replacement counts levels that the stock function does not have.** `error(text, level)` puts the position of the function at `level` in front of the text, and a C function has no position.
  So `error('boom', level)` in a metamethod or a comparison function, for the levels 1 to 4, gives under a stock function the line of that code, `boom`, the line of the author's call, and `boom`.
  The replacements of `22a0992f` are Lua functions with debug info, and each calls the author's code through functions of its own.
  There the comparison function of a sort gave `crates/runvil-internal/lua/src/__impl_stdlib.lua:315: boom` at level 2 and `crates/runvil-internal/lua/src/__impl_stdlib.lua:376: boom` at level 3, as [issue #66](https://github.com/cppalliance/runvil-harness/issues/66) reports.
  A `__len` and an `__index` under `table.concat`, and a `__len`, an `__index`, and a `__newindex` under `table.sort`, gave the line of the author's call at level 2, `boom` at level 3, and the line of the call at level 4.
  A `__lt` gave the line of the call at the levels 2 and 3.
  A `__tostring` under `tostring`, under `%s`, and as an element of `table.concat` gave what the stock function gives at all four levels: it ran under a `pcall` of the replacement, which the first of the decisions below takes out.
- **One table walks in two orders.** With the walk by key, `pairs` and `next` visit every whole-number key from 1 up first, and `tasks.fanout` still sorted an index past the end of its list among the other numbers, after the booleans.
  `{ [1] = 'a', [2] = 'b', [3] = 'c', [5] = 'e', [false] = 'f' }` gave `pairs` the keys `1, 2, 3, 5, false`, and the fanout the members `a`, `b`, `c`, the pair of `false`, and the pair of `5`, as [issue #69](https://github.com/cppalliance/runvil-harness/issues/69) reports.

The work on those found more, in the code of `22a0992f` or in a first form of a fix:

- **The conversion names a different fault in each run.** It reads a table that has no list in Lua's own order and stops at the first entry it cannot convert.
  `{ a = function() end, b = coroutine.create(...) }` was refused for the function in 12 of 24 VMs and for the thread in the other 12.
  Its text for one bad key also depends on whether that key came first: a table of six keys gave `expected a string` in 14 VMs and `expected a string key` in 10.
- **The conversion words a key table by what it holds.** `mlua` says `invalid type: map` of a key that is a table without a list, and `invalid type: sequence` of one with a list.
  Two such keys have no order that every run agrees on: a table with 16 keys of each kind gave the `map` text in 32 of 256 runs with the same inputs and the `sequence` text in 224, on `bdcadfbb` too.
- **A deeply nested value ends the process.** `mlua`'s conversion follows a value to any depth.
  A table nested 1000 deep overflowed the stack of a test thread in a debug build, which aborts the process.
  The code of `bdcadfbb` hands the same value to the same conversion and was not run.
- **The walk that names a fault wrote the path of every fault it found.** `FirstRefusal` keeps one refusal of a table and drops the rest, and each refusal was built with its whole path, a copy of the path so far for each step.
  1,000,000 references to one table with a key table, under 125 tables that each hold the next under a key of 64 bytes, took 29.1 s in a release build, and 0.73 s without the 125 tables.
  A review measured 27.4 s for the same value against 0.19 s on its base, in one Rust call that no cancel interrupts, with 2,000,126 values visited of the 8,388,608 that the default budget has.
- **A value that converts copies text without a count.** Only the walk that names a fault counted the bytes of strings, so a table of 64 tables with one key of 1 MiB each became 64 MiB of data outside the VM's memory limit.
- **One key of a table is JSON text.** `serde_json::Value` reads a map whose first key is `$serde_json::private::RawValue` as the JSON text under that key, in a build that has `serde_json`'s `raw_value` feature on.
  A test build of the workspace has it on: `axum`, a dev-dependency of `harness-gateway-client` and of `plugin-mcp`, asks for the feature, and `cargo tree` shows no crate of the workspace that asks for it outside its tests.
  A Host that depends on `axum` with its default features has it on in every build, by `axum`'s manifest and Cargo's union of features; no Host was built to show it.
  A table with that key alone converted to the data of the text.
  Lua's own walk says which key of a table is first: a table with that key and one more converted to an object in 20 of 32 VMs and failed with `invalid length 2, expected fewer elements in the table` in 12.
  The text is parsed with a nesting limit of its own, so a table inside the limit of 127 converted to data 220 deep.
  `json.decode` and the parse of a tool's structured output read the key the same way.
  A text of 836 bytes with the key twice inside itself decoded 378 deep, and one of 1454 bytes with the key four times overflowed the stack of a test thread in a debug build, which aborts the process.
  The code of `22a0992f` for `json.decode` reads JSON text into `serde_json::Value` too, and was not run.
- **A raised error table fails in two texts.** The read of the fields of a raised error table failed at the first entry with a key that is no string, in `mlua`'s words for that entry, so two such entries gave two texts.
- **A failure with text that is not UTF-8 fails in `mlua`'s words.** The read of a failure took the `message` and the `kind` of a raised error table, and the traceback of any failure, as UTF-8, and failed the block with `mlua`'s text where one was not.
  `tasks.spawn('S', { ['\xff'] = 1 })` failed with `error converting Lua string to &str (invalid utf-8 sequence of 1 bytes from index 26)`, since the Engine's refusal quotes the key, and so did any failure under a function that a table holds under such a key, since the traceback names the field; both are so on `22a0992f`, and a review found the first.
- **A raised error table is copied whole.** The same read copied every string entry of the table to the Rust heap, outside the VM's memory limit, and then kept the few that are facts of the table's code.
  A review measured 1215 MiB in the process for a table inside a limit of 64 MiB, and the measure was not repeated.
- **A table under a weak metatable is a view in one run and a table in the next.** The read through views asks a metatable for `__index` and `__metatable`, which are entries of that metatable, and the collector takes them out of a weak one.
  An empty table under such a metatable converted to the data of its `__index` table while the field was there, and to an empty object after.
  `tools.offer` asks the metatable of its array for a `__len` the same way.
- **`json.encode` names an option of a weak table.** Of several unknown options it names the first, and it read the keys of a weak options table as the collector left them.
- **A metamethod makes the array of an offer weak.** `tools.offer` tested its array for a weak table once, then called the `__len` and the `__index` of the array, and then walked the array.
  A `__len` that sets `__mode` and fills the table made the walk one of a weak table.
- **The test of a weak table scans a `__mode` of any length, once for each table.** Lua reads a `__mode` up to its first zero byte, so the test does, and a `__mode` with no `k`, `v`, or zero byte in it is read whole.
  One table with a `__mode` of 1 MiB, in a list 128 times under a 4 MiB limit, made `json.encode` scan 128 MiB and count 129 values and no text: a review measured 1.43 s in a debug build, against 5.6 ms on its base.
  The review built 65,536 references under that limit, which is 64 GiB to scan in one Rust call that no cancel interrupts, and did not run the scan.
  A chain of views that share the `__mode` did the same to one call of `pairs`, `next`, or `tasks.fanout`, and the test after each entry did it to `tools.offer`.
- **A builder takes a record that no `append` takes again.** A builder converts each field by itself, each within the budget of one conversion, and a view of the record spends that budget once for the whole record.
  With a memory limit of 512 KiB, a content of 65,533 values and a `meta` of two made a record of 65,537, one more than the budget.
- **A stack overflow names a line of the Engine.** Lua raises a stack overflow in the function that makes the call there is no room for, and puts that function's position on it.
  One recursion with 48 sizes of frame failed with `?:?: stack overflow` in 24, with a line of `__impl_coro.lua` in 2, with a line of `__impl_stdlib_stock.lua` in 1, and with no position of the Engine in 21.
- **`table.concat` takes an abort for a refusal.** It joined a plain list through the stock function under `pcall` and started again on any failure, so an abort that a hook raised inside that call was caught and the join ran again.
- **A path of `var` holds every key whole.** The guard of a table in `var` keeps the path of the table as text, with each key in full, so a refusal printed a key of any length and each table under a long key held a copy of it.
- **`var` nests to any depth by one write for each level.** A write to `var` counts the levels of the value written from that value, and not from the table it writes to.
  A loop of `t.n = {}`, `t = t.n` from `var` made a value 200 deep, and one 2,000 deep overflowed the stack and aborted the test process in a debug build, on `22a0992f` too.
  One write of a value 127 deep makes a `var` of 128 levels, its own object counted, and the JSON text of that `var` failed to read back with `recursion limit exceeded`.
  `master` refuses both writes since `c235fcd6`, by a second count of the levels of the converted value, and its refusal names the key written and not the table that is too deep.
  A called chain and a task get `var` whole, and the event `TaskStarted` reports it to the caller of the Engine.
- **Every table of `var` holds its whole path.** The path of a table is text on the Rust heap, outside the VM's memory limit, and each table under it holds a longer one.
  The read-only view of `var` that a Pack prelude gets held a path for each table read through it, and a refusal text with that path in it; that was read and not run.
- **A key of `var` that is not UTF-8 passes the write.** `var['\xff'] = 1` was taken, and the read-back of `var` at the end of the block failed with `invalid type: byte array, expected a string key`, on `22a0992f` too.
  The same key inside a value written whole was refused at the write.
- **A memory error under a replacement ends the process.** A first form of the replacements asked `mlua` for the metatable of strings at each call, and `Lua::type_metatable` pushes an empty string with no protected call around the push.
  Inside a callback a memory error of that push leaves through the Rust frames of the callback.
  A block that filled its heap in a loop of `tostring(label)` aborted the test process with `Assertion failed` in `lapi.c` at some sizes of a filler, where `22a0992f` returns `not enough memory` to the `pcall` of the block; a review found it.
- **A VM that does not fit its memory limit can fail to build with a panic.** `mlua` keeps each Lua value that Rust holds in a slot of a stack of its own, and panics when that stack must grow and the memory limit refuses the growth, which is [#64](https://github.com/cppalliance/runvil-harness/issues/64).
  On `22a0992f` the stack has 34 slots when the VM is built, the build holds 24 values at once, and each table inside `args` and `var` holds one more for the life of the VM, as does each table that a Pack prelude makes a global.
  So from 11 tables inside `args` and `var`, or from 19 with those of one prelude, the stack grows under the run's limit: with 11 tables inside `args`, about 640 of the limits from 8,192 bytes below the heap after the Engine values are injected to the heap of the built VM end the build with the panic, and with 10 none does.
  A first form of this change held 8 values more at once, so the build panicked from 3 tables.
  It also made the names of the events and took the metatable of strings a second time, for the handler check of `xpcall`, after the run's limit was set.
  With no table inside `args`, the stack then grew under the limit and the build panicked at some limits, and at others the push of `Lua::type_metatable` had no memory and the process ended with `SIGABRT`: a review's pass over the limits by one byte met the panic in 127 stretches of limits and the abort at 24 limits, and neither on `22a0992f`.
- **A metamethod that calls the replacement again fails as an Engine function does.** Lua takes 200 nested C calls and counts a protected call among them, and `mlua` makes a Lua string from Rust text under a protected call when the VM has a memory limit.
  A capture of the replacements read each metamethod by Rust text, so in a nest of `__len` or `__lt` metamethods that each call the replacement again, that read was the call that met the limit.
  The failure was then a Rust callback's: a `pcall` around the nest caught an error value that reads `prompt:1: C stack overflow. The Lua code failed at runtime. Fix the code at this location.`, where `22a0992f` and stock Lua give the string `C stack overflow`; a review found it.
- **A replacement that another replacement runs as a metamethod is named by the event.** With `string.format` as the `__index` of the list of `table.concat`, the argument error read `bad argument #1 to 'index'`, where stock Lua, whose `table.concat` is a C function, reads `'string.format'`, and `22a0992f` read `'format'`; a review found it.
- **The `__concat` of an error value is a chunk with no name.** `mlua` names such a chunk by the Rust line that loads it, so its position has that line and the line of the chunk after the name, which the rule on the position of an Engine chunk does not read.
  `refused .. value`, with a `__tostring` of `value` that calls `error('boom', 3)`, read `crates/runvil-internal/lua/src/error_value.rs:594:1: boom` on `22a0992f`, and a stack overflow that Lua raised in that chunk had the position in the prose of the failure at 5 of 48 sizes of frame.
- **The `__concat` of an error value calls a `tostring` that a block replaced.** The chunk of the `__concat` was loaded at the first failure of the VM, on `22a0992f` too, by its code, and took the `tostring` global of that moment.
  A block that put its own function there first, then caught a refusal of `tools.offer` and joined it with `'!'`, got `replacedreplaced`; a review found it.
  The same block with the two statements the other way round got the message of the refusal with `!` after it.
- **A chunk name of 60 bytes is cut.** Lua writes a chunk name of more than 59 bytes as `...` and its end, which the rule does not read either.
  The longest name of an Engine chunk is 54 bytes, and nothing held a new one to the limit.
- **A wait reads a weak set.** `tasks.join` and `tasks.join_any` read their set with `ipairs` and write to the heap between two reads, so the collector can take a handle that only a weak set holds.
  `tasks.join(setmetatable({ 5 }, { __mode = 'v' }))` failed for the type of the member, so the set was read.
  A weak table of options with a `__pairs` of its own passes the key check, which walks with `pairs`, and a weak Task handle is read by key; both were read in the code and not run.
- **A `tasks` function calls a global that a block can replace.** The chunk of the `tasks` functions read `ipairs`, `select`, `tostring`, and `type` from the globals at each call, so a replacement of one runs the author's code between the check of a table and the read of it.
  A review replaced `ipairs` with a function that gives the set a `__mode` and makes garbage, and `tasks.join` read the set as the collector left it: it failed with `tasks.join requires at least one task`.
  The functions of `__impl_coro.lua` read `select`, `tostring`, and `type` the same way, among them the helpers that a `tasks` function calls to name a type and to raise a failure, and `tasks.fanout` in `__impl_fanout.lua` read `error` and `ipairs`, on `22a0992f` too, by the listing of `luac`.
  Each of the two chunks already took other globals as locals, with a comment that says a block's replacement must not move it.
- **`require` lists a cycle through a `table.concat` that a Pack prelude replaced.** The chunk of `require` loads after the Pack preludes, and a prelude shares the `table` library with the section, among seven library tables, so it can set a field of it.
  A review ran a prelude that sets `table.concat`, and a cycle of two libraries read `require cycle: HIJACKED</a -> /b -> /a>`; the chunk of `22a0992f` takes `table.concat` at the same moment, by its code.
- **The read-back of `var` asks an `__index` of the author's for the `var` global.** The read that tells whether a block reassigned `var` went through the metatable of the globals, on `22a0992f` too, by its code.
  With `var` set to nil, an `__index` that a block gave the globals ran inside `call`, `tasks.spawn`, `tasks.fanout`, and the read-back at the end of a block, and what it returned was taken for the global.
- **The Engine's `xpcall` names an internal variable in its argument error.** `xpcall(f, 5)` raised `crates/runvil-internal/lua/src/__impl_coro.lua:106: bad argument #2 to 'raw_xpcall' (function expected, got number)` on `22a0992f`, and the rule on the position of an Engine chunk leaves the name.
  `xpcall(f)` said `got nil`, where stock Lua says `got no value`, and `pcall()` returned `false` and `attempt to call a nil value`, where stock Lua raises `bad argument #1 to 'pcall' (value expected)`; a review found the first, and a probe the other two.
  With that position taken off by the rule, the text that a `pcall` around the `xpcall` catches has none, where stock Lua puts the line of the call in front.
- **An argument error names a replacement by one name.** Stock Lua names the function of an argument error as the call is written (`luaL_argerror` in `lauxlib.c`): by the variable or the field of the call, and by the name in the library where no Lua code makes the call.
  A replacement is a Lua function, and it raised the text itself, or took it from the stock function, under one name.
  `pcall(table.sort, 5)` read `bad argument #1 to 'sort'`, where stock Lua reads `'table.sort'`, and a call through a local `order` read `'sort'`, where stock Lua reads `'order'`, on `22a0992f` too, by its code; a review found it.
  A call written `('%d'):format('x')` named argument 2, where stock Lua leaves the string out of the count and names argument 1, as the first plan states.
- **A failure in the guard of the globals names a variable of the guard.** The guard reads the `__index` and the `__newindex` that the author gave the globals, and Lua refuses one that it cannot index in the guard's own function.
  `setmetatable(_G, { __index = 5 }) return undefined_name` failed with `crates/runvil-internal/lua/src/__impl_globals.lua:55: attempt to index a number value (local 'handler')` on `22a0992f`, by a review's run.
  The rule on the position of an Engine chunk takes the position off and leaves `(local 'handler')`, where stock Lua says `attempt to index a number value` and no more; a review found it.
- **A write to `var` pays for the path of its target.** On `22a0992f` each table of `var` holds its path as text, and a write builds the path of its target from it, for the conversion.
  A block that wrote 1,000,000 integers to a list took 0.50 s to 0.52 s at the top of `var`, and 1.14 s to 1.17 s under 120 tables with keys of 64 bytes, over three runs in a release build.
  A first form of the fix here kept one step for each table and walked the places above it at each write, and a review measured 0.56 s and 2.03 s for the same writes.

The review of pull request #78 found three more in `4fdbe940`, and each was run on that code before its fix:

- **A `tasks` function takes an option from a weak table.** The check of the options asks about the table given and the views it is read through, and Lua's read of a field goes on from there: a table that lacks the field hands the read to the `__index` of its metatable, and that table to the `__index` of its own.
  `tasks.spawn('a', setmetatable({}, { __index = setmetatable({ item = {} }, { __mode = 'v' }) }))` went on to its yield, with an `item` that the collector takes once nothing else holds it.
  A Task handle and a set of tasks are read the same way.
  So are four more reads of one field that the Engine makes of a table of the author's: an entry that the array of `tools.offer` lacks and its `__len` counts, the global of `{{ name }}` in prose through the `__index` that the author gave the globals, and the `compactors.fail` and the `sys.taskid` that `models.loop` reads.
  A search of the crate for a read of a table that is not a raw one found the four, and each was run: the offer was recorded, the prose had the global, and the loop started.
- **A builder takes a record with more text than one conversion spends.** The parse of a record counted the values of the record against the budget of one conversion, and not its bytes of text.
  With a memory limit of 1 MiB, a content of 524,262 bytes and a tool call whose arguments hold the same string made a record of 1,048,577 bytes, one more than the budget, and the builder took it.
  The review ran it with a limit of 256 KiB, where `messages:append(messages[1])` then refused the record.
- **The read-back of `var` runs an `__eq` of the author's.** The read of the global is a raw one, and the comparison after it was `Table::equals` of `mlua`, which calls `__eq`.
  `var = setmetatable({}, { __eq = function() return true end })` passed for the Engine's table, and the snapshot was of the `var` that the block had replaced.
  `require` compared a saved value with its mark of a load under way by `==`, and the mark was a table.
  A library that returned a table with such an `__eq` raised `require cycle: /a` at its second `require`; a search for a comparison of that kind found it, and no other.

## The rule

- **The author's code under a replacement.** `tostring`, `string.format`, `table.concat`, and `table.sort` run a metamethod or a comparison function with no protected call of their own around it.
  A failure of that code reaches the handler of the author's `xpcall`, and the Engine's own, where it is raised, with the frames of that code in place and its variables open.
  What Lua itself refuses, a `__tostring` that cannot be called, a list that cannot be read or written, two values with no order, Lua refuses in the replacement, in stock Lua's text with `?:?: ` in front, which the rule on the position of an Engine chunk takes off.
  A `__len` or a `__lt` that cannot be called is the one refusal that the Engine raises itself, in stock Lua's text with no position, after it has asked whether Lua can call the metamethod.
  A stock function runs under `pcall` only over values whose metamethods it does not use.
  Of the failures of such a call, a replacement takes only the one that Lua raised in the stock function for its own refusal, and raises any other, such as the abort of a cancelled run, as it is.
  To the author's code a replacement is one frame with no position, as the stock function is: `error(text, 2)` there puts nothing in front of the text, and `error(text, 3)` the position of the author's call.
- **The position of an Engine chunk.** A failure text that starts with the position of a chunk of the Engine, `crates/runvil-internal/lua/src/<file>:<line>: `, or with `?:?: `, which Lua writes for a function with no debug info, loses that start.
  The rule applies where a failure leaves the crate, in the failure of a block and of a Pack prelude, and where the Engine's `pcall` or `xpcall` hands a caught string to the author's code.
  So a stack overflow reads `stack overflow` wherever Lua raises it, with the author's position in front when Lua raises it in the author's function.
  Where Lua raises it inside a protected call that a capture makes, the caught failure is an error value, which "Known" states.
  The Engine's `pcall` and `xpcall` are Lua functions around the stock ones, and a refusal that the stock one raised would name a line of their chunk, which the rule takes off, and the variable of that chunk the stock one is called through.
  So each refuses its own arguments before it calls the stock one, in stock Lua's text and at the author's call, as the replacement of `tostring` does: no argument to `pcall`, and a handler of `xpcall` that is no function, with `no value` for one that is missing.
  Every chunk of the Engine that is loaded as text has a name under that opening, of at most 59 bytes after its `@`, which is the longest name that Lua writes whole.
- **The name in an argument error.** An argument error of `tostring`, `string.format`, `table.concat`, `table.sort`, `math.randomseed`, `pcall`, `xpcall`, `setmetatable`, and `getmetatable` names the function as the author's call is written, as stock Lua does.
  That is the variable or the field of the call, `for iterator` in a generic `for`, and the name in the library, as in `table.sort`, or the global, as in `pcall`, where no Lua code makes the call.
  A call written `value:name(...)` leaves the value out of the count, and a refusal of the value itself reads `calling 'name' on bad self`.
  One capture, `argument_error`, makes the text for all nine, from what Lua's stack has of the call.
  A replacement that runs a metamethod is no Lua code to that rule, as the C function it stands in for is none: a replacement that is that metamethod is named by the library, as in `string.format`.
  The guard of the globals stands in for no stock function, so a replacement that it runs for a handler that is a table is named by the event, `index` or `newindex`, as the author's read or write names it in stock Lua.
  A tail call and a call through a `__call` metamethod read otherwise than in stock Lua, and "Known" states each call that does.
- **A failure that Lua raises in the guard of the globals.** The chunk of the guard has no debug info, as the chunk of the replacements has none.
  So a handler of the author's that Lua cannot index fails in Lua's words, with no variable of the guard after them and `?:?: ` in front, which the rule on the position of an Engine chunk takes off.
- **A memory error under a replacement.** `install` takes the metatable of strings from the VM once, while the VM is new and far below its limit, keeps it with the names of the events that the captures read, and hands it to the chunk of the replacements.
  A Rust capture reads a field of it by a name that is a Lua string already, a raw read that makes no Lua value, so Lua has no memory error to raise in it.
  The function that names the type of a refused argument is made there too, once, and the chunk of `pcall` and `xpcall`, which is installed under the run's limit, takes that one from the registry, so nothing takes that metatable or makes those names again.
- **A VM that does not fit its memory limit.** A new VM holds 64 Lua values at once and lets go of them while it is far below its limit, which takes the stack that `mlua` keeps such values in to 68 slots.
  The build of a VM holds 32 values at once, and one more for each table inside `args` and `var`.
  So with 36 such tables or fewer and no Pack prelude the stack does not grow under the run's limit, and a build that the limit refuses fails with `not enough memory`.
  A Pack prelude is code, and what it holds counts too.
  The install of the preludes holds 22 values at once, one more for each table that a prelude has made a global, one more for each Pack before the one that loads, until the collector frees its environment, and what the Engine functions hold that a prelude calls while it loads.
  With one Pack whose prelude defines functions and tables alone, that is 46 tables or fewer inside `args`, inside `var`, and among the globals of the prelude.
  Past those counts the stack grows under the limit, as it does on `22a0992f` from 11 tables and from 19, and "Known" states every holder of a slot and what follows.
- **A capture under a nest of metamethods.** A capture of the replacements makes no protected call and no Lua string while it takes a value and refuses nothing, as the C function it stands in for makes none.
  So a `__len`, a `__lt`, an `__index`, or a `__newindex` that calls the replacement again fails with Lua's own `C stack overflow`, a string with no position, at 195 or 196 levels, as in stock Lua, which has 194 to 196 by the calls around the nest.
  On `22a0992f` that nest fails at 97 or 98 levels, with the author's position in front of the text or with none.
  A capture still makes a string to refuse, to read the metatable of a userdata, and to print a Rust callback's failure or a value whose `__name` has a zero byte in it; "Known" states what follows.
  `tostring` and `%s` read the `__tostring` of a string from the table in Lua, by a raw read, which makes no value and calls nothing of the Engine.
  No code of a section VM can put another metatable in its place, and a field that a block sets in it is a field of the same table.
- **A weak table in every reader.** Since the first plan, `pairs`, `next`, `tasks.fanout`, and the install of a Pack prelude refuse a table whose metatable has a `__mode` string with `k` or `v` in it.
  So does every other Engine function that reads the entries of a table: `json.encode`, for its value with the placeholder `"[weak table]"` in debug mode and for its options, a write to `var`, `tools.offer` for its array, `tools.adhoc` for its attributes and its parameters, `models.use` for its options, and the conversion of a table to data.
  The other `tasks` functions refuse a weak set of tasks, a weak Task handle, and a weak table of options, in their own name and before each read of the table, and a view that leads to a weak table and a table under a weak metatable with them.
  No function of a chunk of the Engine reads a global when it is called: each chunk takes the globals it uses as locals when it loads, before any block runs.
  The chunk of `require` loads after the Pack preludes.
  A prelude can set a field of each library table it shares with the VM, `string`, `table`, `math`, `utf8`, `tools`, `store`, and `json`, and no global that the Engine has set, so the chunk takes no function of a library.
  The read that tells whether a block reassigned `var` is a raw read of the globals, so no `__index` of the author's answers for it.
  The chunk of the `__concat` of an error value loads when the VM is made, where it loaded at the VM's first failure.
  So a global that a block replaces puts none of the author's code inside a `tasks` function, between the check of a table and the read of it, or inside the join of an error value.
  `ipairs` and the functions of `table` read a table by its indices, as `t[i]` in a block does, and take a weak one.
- **A table under a weak metatable.** Every read through a view refuses a table whose metatable is a weak table, with words of its own, before it asks that metatable for a field, and `tools.offer` refuses one as its array.
  `json.encode` writes the placeholder `"[weak metatable]"` for one in debug mode.
- **A read by key that looks into a weak table.** Lua reads `t[k]` from `t`, from the `__index` of the metatable of `t` when `t` lacks the field, from the `__index` of that table's metatable when it lacks the field too, and so on (`luaV_finishget` in `lvm.c`).
  Where the Engine reads one field of a table of the author's that way, or has Lua read it, it refuses a weak table and a table under a weak metatable that the read looks into past the table given, as in `tasks.spawn opts must not be a table whose __index leads to a weak table, ...`.
  The reads are an option of a `tasks` function, the `task` of a handle, a member of a set, an entry that the array of `tools.offer` lacks and its `__len` counts, the global of `{{ name }}` in prose that the globals lack, and the `compactors.fail` and the `sys.taskid` of `models.loop`.
  The Engine follows the read with no call into Lua: through a table that lacks the field to the `__index` of its metatable, and through an `__index` that is a string to the `__index` of the metatable of strings, which a block can set.
  It ends at a table that holds the field, at a metatable with no `__index`, and at a function, which is the author's code.
  It goes as far as Lua does: Lua asks at most 2000 values past the table the read is written for, and then raises `'__index' chain too long; possible loop`.
  The check comes before each such read, with none of the author's code between the two.
  The bytes of `__mode` that it scans count against a budget of the VM's memory limit, with those of the views, and each table on the way of the read counts as one value of the same budget, which has as many values as one eighth of that limit in bytes: the budget of the array of an offer, one for the placeholders of one `prose`, and one for each check of a `tasks` function and of `models.loop`.
  Past the values the read is refused, as in `tools.offer takes an array, got a table whose __index chain is followed through more than 8388608 tables: one call follows at most 8388608`.
- **The Engine compares by identity.** The read that tells whether a block reassigned `var` compares the global with the Engine's table as `rawequal` does, so a table in its place is a reassignment whatever its `__eq` returns.
  `require` marks a load under way with a function, where the mark was a table: Lua calls an `__eq` for two tables or for two userdata alone, so `==` with the mark runs no code of a table that a library returned.
- **The bytes of `__mode` are counted.** The test of a weak table reads a `__mode` up to its first `k`, `v`, or zero byte, and its caller counts the bytes read as text against the budget of the conversion.
  `pairs`, `next`, `tasks.fanout`, and `tools.offer` have a budget for these bytes alone, the VM's memory limit in bytes for one call, and so has the check that a `tasks` function makes of one table.
  Past the budget a conversion is too large, and those refuse the table with the bytes of `__mode` that the call came to and the most that one call scans, as in `'pairs' does not walk a table with 5242880 bytes of __mode to scan: one call scans at most 4194304 bytes`.
  `json.encode`, a write to `var`, and the `markdown` functions stop at the first fault of their walk, so they say too large only when the walk meets no other fault before the budget ends.
  The conversion of a table to data and the short form of `tools.adhoc` say too large whatever else is wrong.
- **The keyed members of a collection.** `tasks.fanout` lists the keyed members of a collection in the order of `pairs`: whole numbers from 1 up, then booleans, then the other numbers, then strings.
  So the arms of a collection come as a `pairs` loop over it meets its keys.
  The first plan listed them as `json.encode` visits keys: booleans, then every number by its value, then strings.
- **The array of an offer.** A table with a `__len` is an array of as many entries as `__len` gives, each read through `__index`.
  Any other table is an array of the entries it holds itself from 1 to the first it lacks, counted by `listed`, and its `__index` has no part in the read.
  `#t` has no part either way, so a table with a gap is refused for the same key in every run.
  The array is tested for a weak table, and for a weak metatable, before its first read and after each call of its `__len` or `__index`.
  The first plan ended the array at the first nil that the read met, and left to `#t` a table with no `__len` whose `__index` gives a value at a gap.
- **A table that becomes data.** `data_of` makes every conversion of a Lua value to data: the arguments of `tools.call`, the `item` of a spawn and of a fanout member, a field of a message, and a global in prose.
  It refuses a table that the conversion reads and that has a gap in its list, is weak, or sits more than 127 tables deep.
  A list has a gap when its positive integer keys are not 1 to their largest, so the test needs no length.
  The refusal names the place, as in `value.rows is missing list item [4]`.
  Of several entries of one table to refuse, a second walk finds the one whose key comes first, and gives it `mlua`'s own text for that entry alone.
  The conversion reads the items of a list and no other entry of that table, so what another key of a list holds is left out, as before, and is not checked.
  A key that is a table, a function, a userdata, or a thread is refused in the Engine's own words, which say its type and no more: `value.rows has a table key; object keys must be strings`.
  Both walks count the bytes of every string before they read one, keys included, so a value that holds more text than the VM's memory limit is too large, whether or not anything else is wrong with it.
  Every string key is a name: the conversion reads a table through `plain_json.rs`, which has no key of its own.
- **A key of JSON text is a name.** `json.decode`, `json.decode_lines`, and the parse of a tool's structured output read every key of an object as a name, through the same reader.
- **A record nests as data does.** The parse of a record refuses one whose JSON form nests more than 127 deep, with the record's own object and the arrays and objects around a field's value counted.
  A builder checks each field by itself first, so a `meta` 127 deep passed that check and made a record 128 deep, a view of which no `append` took.
- **A record holds what one conversion spends.** The parse of a record refuses one whose JSON form holds more values than the budget of one conversion on the VM, with the record's own object and its `role` counted.
  It refuses one that holds more bytes of text than that budget has, with the keys of its objects counted, as in `messages[3] holds 1048577 bytes of text as JSON; the limit is 1048576 bytes`.
- **A quoted key or name.** A path, and a refusal of `tools.adhoc`, quotes at most the first 64 bytes of a key, a name, or a type, cut at a character boundary, and gives the length of a longer one: `kkk... (1000 bytes)`.
  The quote ends in front of the last byte, among the 62nd to the 65th, that does not continue a character, so it holds 61 to 64 bytes and cuts no character of UTF-8.
  Where each of those four bytes continues a character, which is no UTF-8, it holds 64.
  A byte of the quote that is not UTF-8 reads U+FFFD, and the length is that of the author's bytes.
  A quoted text is then at most 384 bytes and the note of its length, since `tools.adhoc` writes a control byte as up to six characters, and a path has at most 127 of them.
  A path of `var` quotes a key the same way, in a refusal and in the path that the guard of a table keeps.
- **The fields of a raised error table.** A field is an entry whose key is a UTF-8 string and whose value is a UTF-8 string or an integer.
  The read asks the table for the facts of its code, each by its name, and reads no other entry, where it walked the table and failed at the first entry that is no field.
  A code has at most five such facts, so the read makes at most nine copies of strings, each of a field of the table, and holds at most seven of them at once: the message twice, the `code`, a `task` or a `reason`, and the facts.
  That is at most seven times the VM's memory limit on the Rust heap, 448 MiB at the default, by reckoning.
  Nothing of the read follows the order of Lua's own walk.
- **The text of a failure.** The message of a raised error table and the traceback of a failure read with U+FFFD for each part that is not UTF-8, as the text of a raised string does on `22a0992f`.
  A `kind` that is not UTF-8 is the kind of no error: the table then fails the block as a raised value, which prints as its `message`.
- **The depth of `var`.** A write to `var` counts the levels of the value from the level of the table it writes to, with `var` itself as level 1.
  So the whole of `var` nests at most 127 deep, whether one write made the levels or one write for each, and its JSON text reads back.
  A table of `var` keeps the step to it from the table that holds it, and a path is written only for a refusal, so a write that is taken costs the same at every depth.
- **A key of `var`.** A write to `var` under a string key that is not UTF-8 is refused where it is made, in the words of the conversion for such a key: `var.t.<key> is not valid UTF-8`.

## What this changes for an author

- A waiting call such as `tools.call` works inside a `__tostring`, a `__len`, and a `__lt` that `tostring`, `string.format`, `table.concat`, or `table.sort` runs, inside the `__index` of a list that `table.concat` joins, and inside the `__index` and the `__newindex` of a sorted list.
- `table.sort` takes a table or a value with the `__index`, `__newindex`, and `__len` metamethods, as stock Lua does, so `table.sort(messages)` fails at the comparison of two records or at the write, as on `bdcadfbb`.
  The sort of the first plan took a table only.
- `table.sort` of a table whose `__len` cannot be called fails with `attempt to call a boolean value`, which is stock Lua's text, where the sort of the first plan added `(metamethod 'len')` to it.
- `table.concat` takes a table or a value with `__index` and `__len`, takes integer bounds, and fails on the line of the call in stock Lua's text, where the wrapper on `bdcadfbb` read any value by index and put the position of its own chunk on a failure.
  A userdata with no `__tostring` joins as its name, where it joined with its address.
- A `__tostring` that prints its own value, or a comparison function that sorts, fails with `stack overflow` at the default memory limit of 64 MiB, where stock Lua fails with `C stack overflow` within its limit of 200 C calls.
  In the runs made, the nests through `tostring`, `string.format`, and `table.sort` took 0.12 s to 0.22 s in a debug build and 0.02 s to 0.04 s in a release build, and the nest through `table.concat` 1.7 s to 3.4 s and 1.5 s to 3.5 s, by the form of the `__tostring` and by up to a second from one run to the next.
  Under a smaller limit the nest can fill the heap before the stack, and it then fails with `not enough memory`: at 16 MiB or less through each of the four, at 24 MiB through all but a `__tostring` that returns the call of `tostring`, and at 32 MiB and 48 MiB through `string.format`.
  A `__tostring` that keeps the result of `tostring` in a local and returns it fails with `not enough memory` at 24 MiB too.
  The text has the author's position in front or none.
  On `22a0992f` the three nests through a `__tostring` failed with `C stack overflow`, at 97 or 98 levels, and the comparison function that sorts with `stack overflow`, as here.
- An error text that starts with `?:?: `, or with `crates/runvil-internal/lua/src/`, a file name, a line number, and `: `, loses that start: `error('?:?: boom', 0)` reads `boom`.
  `pcall(function() error('boom', 3) end)` gives `boom`, where it gave `crates/runvil-internal/lua/src/__impl_coro.lua:91: boom` and stock Lua names the line of the `pcall`.
- `tasks.fanout` over `{ [7] = 'x', [false] = 'y', [2.5] = 'z' }` spawns the arm of the key 7 first, where it spawned it last: a whole-number key from 1 up comes before a boolean and before every other number.
- `tools.offer` of a table with a gap, no `__len`, and an `__index` that gives a value at the gap raises an error for the first key past the gap, where one run accepted the table and another refused it.
- A list with a gap in the arguments of `tools.call`, the `item` of a spawn, a field of a message, or a global in prose raises an error that names the missing item, where the list became three items in one run and five in another.
  `{ [2] = 'b' }` and `{ a = 1, [5] = 2 }` raise `value is missing list item [1]`, where the conversion refused the integer key in its own words.
- A value nested more than 127 tables deep raises an error in those four places, where a value some thousands deep ended the process.
- A weak table raises an error in `json.encode`, a write to `var`, `tools.offer`, `tools.adhoc`, `models.use`, the `markdown` functions, and those four places, where each read what the collector had left.
  An empty weak table with `__index` and `__metatable` raises it too, where it was read as a view.
  A view that leads to a weak table raises it wherever the Engine reads through a view: in `json.encode` for its value, a write to `var`, the `markdown` functions, `pairs`, `next`, `tasks.fanout`, the other `tasks` functions, and those four places.
  The attributes and the parameters of `tools.adhoc` are read through a view since `cc55a351`, so a view that leads to a weak table raises it there too, as `tools.adhoc: the attributes are` and `tools.adhoc: parameters are` with the reason.
  `models.use`, the options of `json.encode`, and `tools.offer` for an array with no `__len` read the entries of the table itself, so such a view is an empty table to them, as before.
- A table with a key that is a table, a function, a userdata, or a thread raises `value has a table key; object keys must be strings` in those four places, with the key's type in it, where the error was `mlua`'s for what the key holds.
- A key or a name longer than 64 bytes appears in a path or in a refusal of `tools.adhoc` as its first 61 to 64 bytes, to the end of a character, and the length of its bytes, where it appeared whole.
- Of several unknown keys of the attributes of `tools.adhoc`, the error names the first in the shared order of keys, where it named the first by its text.
- A raised error table with a key that is no string, or with a field, a `kind`, or a `message` that is not UTF-8, fails the block with its `message`, where the block failed with `mlua`'s text for that entry.
- A failure under a function that a table holds under a key that is not UTF-8 fails the block with its own message and line, where the block failed with `mlua`'s text for the traceback and no line.
  `tasks.spawn('S', { ['\xff'] = 1 })` reads `tasks.spawn: unknown key "\u{fffd}"; accepted keys are args, item, index`, with the character U+FFFD written here as Rust writes it.
- A value whose strings, keys included and each counted every time it occurs, hold more bytes than the VM's memory limit raises `the value is too large to convert`, where a value with a fault named the fault and a value with none converted.
- A section whose VM does not fit its memory limit fails with `not enough memory` when `args` and `var` hold 36 tables or fewer inside them and no Pack has a prelude, where the build panicked in `mlua` at some limits from 11 tables.
  With one Pack whose prelude defines functions and tables alone, the count is 46 with the table globals of the prelude, where it was 18.
  Each further Pack, and each Engine function that a prelude calls while it loads, takes from that count, as "Known" states.
  A section VM takes 528 bytes more of its limit for that.
- The key `$serde_json::private::RawValue` is a name like any other in a table that becomes data, in `var`, in `json.decode` and `json.decode_lines`, and in a tool's structured output, where the text under it was read as JSON.
- A write that would put a table of `var` more than 127 levels deep, `var` itself counted, raises an error that names the table and ends `is nested more than 127 levels deep`, where a write for each level nested `var` to any depth.
  On `cc55a351` the write is refused too, and the error names the key written, where this change names the table inside the value that is too deep.
  One write of a value 127 tables deep raises it too, where the value was stored and the JSON text of `var` no longer read back: a value under a field of `var` nests at most 126 tables deep, one fewer than `json.encode` takes.
- A write to `var`, or to a table in it, under a string key that is not UTF-8 raises an error that ends `is not valid UTF-8`, where the write passed and the block failed at its end.
- A write to `var` that is taken costs the same under 120 tables as at the top of `var`: a block that writes 1,000,000 integers to a list takes about 0.28 s in a release build, where it took about 0.51 s at the top and about 1.15 s under the tables.
- A block that replaces the global `error`, `ipairs`, `select`, `tostring`, or `type` no longer has its function called by a `tasks` function, by `models.infer`, by `xpcall`, or by the failure of a call that waits.
  `refused .. text`, where `refused` is an error value, gives the message and the text whatever a block put in place of `tostring`, where a `tostring` replaced before the VM's first failure was called for both sides.
- A block that sets the `var` global to nil has reassigned it, whatever an `__index` that the block gave the globals returns for `var`: the next `call`, `tasks.spawn`, `tasks.fanout`, or read-back raises `` the `var` global was reassigned ``, where such an `__index` was called and its answer taken.
- A `require` cycle lists its libraries whatever a Pack prelude put in `table.concat`.
- `xpcall` with a handler that is no function raises `bad argument #2 to 'xpcall' (function expected, got number)`, and `got no value` for a handler that is missing, where it named `raw_xpcall` and said `got nil`.
  `pcall()` raises `bad argument #1 to 'pcall' (value expected)`, where it returned `false`.
  Each is stock Lua's text, and a `pcall` around the call catches it with the line of the call in front, as in stock Lua.
- An argument error names `tostring`, `string.format`, `table.concat`, `table.sort`, `math.randomseed`, `pcall`, `xpcall`, `setmetatable`, and `getmetatable` as the call is written, as stock Lua does.
  `local order = table.sort order(5)` raises `bad argument #1 to 'order' (table expected, got number)`, and `pcall(table.sort, 5)` gives `bad argument #1 to 'table.sort' (table expected, got number)`, where both named `sort`.
  `('%d'):format('x')` names argument 1, where it named argument 2, and `text:format()` on a table `text` reads `calling 'format' on bad self (string expected, got table)`.
- `setmetatable(_G, { __index = 5 })` and then a read of an unset global raises `attempt to index a number value` on the line of the read, where the text had `(local 'handler')` after it.
  A `pcall` around the read catches the text with no position, where stock Lua has the line of the read in front.
- Under a `pcall`, a `__close` handler that runs while a failure passes through `tostring`, `string.format`, `table.concat`, or `table.sort` reads `?:?: ` in front of a refusal that Lua raised in one of them, as in `?:?: attempt to index a number value`; the `pcall` around it gives the text without that start.
  A handler reads the same start on a refusal that Lua raised in the guard of the globals.
- A weak set of tasks raises `tasks.join expects a set of tasks, got a weak table, ...`, and a weak Task handle raises `tasks.cancel expects a Task handle or task id, got a weak table, ...`, in the name of the function called, where each was read.
  A weak table of options raises `tasks.spawn opts must not be a weak table, ...`, where it raised `'pairs' does not walk a weak table, ...`, and where one with a `__pairs` of its own was read.
- Options, a Task handle, or a set of tasks whose `__index` leads to a weak table for the field that the call reads raise an error in the name of the call, as in `tasks.spawn opts must not be a table whose __index leads to a weak table, ...`, where the field was read as the collector had left it.
  `tools.offer` raises it for an entry that its array lacks and its `__len` counts, as in `tools.offer takes an array, got a table whose __index leads to a weak table, ...`.
  `models.loop` raises it for its reads of `compactors.fail` and `sys.taskid`, as in `models.loop reads compactors.fail, and compactors must not be a table whose __index leads to a weak table, ...`.
  A placeholder `{{ name }}` raises it for a global that the globals lack, with `` prose reads the unset global `name` through _G, which must not be a table whose __index leads to a weak table, ... `` after the words of a global that is not JSON data.
  A table that holds the field itself, and one whose `__index` is a function, are read as before.
- A builder refuses a record that holds more bytes of text than the VM's memory limit, the keys of its objects counted, as in `messages[3] holds 1048577 bytes of text as JSON; the limit is 1048576 bytes`, where the builder took the record and no `append` took a view of it.
- A table in the place of `var` with an `__eq` that returns true raises `` the `var` global was reassigned `` at the next `call`, `tasks.spawn`, `tasks.fanout`, or read-back, where the block went on and the chain it called got the `var` that the block had replaced.
  A library that returns a table with such an `__eq` comes back from its second `require`, where that call raised `require cycle:`.
- `refused .. value`, where `refused` is an error value and the `__tostring` of `value` calls `error('boom', 3)`, reads `boom`, where it read `crates/runvil-internal/lua/src/error_value.rs`, two numbers, and `boom`.
- A table that holds itself through a sealed table raises `recursive table detected` in those four places, where it became data that ended in an empty object, and a sealed table that only a function can read counts as a level of the 127.
- A view of a message record inside a value counts as the record's data in those four places: records that each hold a view of the one before raise an error at the one whose view would end more than 127 deep, where they nested to any depth, and the values and bytes of a record count once for each view of it.
- A builder refuses a record whose data nests more than 127 deep with the record's own object counted, as in `messages[37] is nested 128 levels deep as JSON; the limit is 127 levels`, where the builder took the record.
  A `meta` nests at most 126 deep, and the arguments of a tool call at most 124.
- A value of one large table converts at the cost it had: a list of 4,000,000 integers took 290 ms to 293 ms in a release build, against 278 ms to 289 ms on `22a0992f`, over three runs of each.
  A review measured 302 ms to 308 ms against 289 ms to 304 ms.
  Both trees walk the value and then convert it.
  A value of many small tables costs 0.04 µs to 0.09 µs more for each table: over six rounds, `messages:user` of a list of ten lists of two integers took 6.7 µs to 7.1 µs against 6.0 µs to 6.2 µs, and of the same list from `json.decode` 8.4 µs to 8.8 µs against 7.8 µs to 8.1 µs, which is 5% to 16% more.
  At 0.09 µs that is 0.75 s for the 8,388,608 values of one walk at the default limit, by reckoning and not run.
  The cause was read from the code and not measured apart: the walk reads the length and the metatable of each table, to tell which entries the conversion reads, and counts the keys of its list, to tell a gap.
- `json.encode`, `pairs`, and `next` take 4% to 17% less time than on `22a0992f` for a table that has a metatable, in the four blocks measured over six rounds: a read through views makes the name of a metatable field once, where it made a Lua string of the name at each table.
  For a table with no metatable, `pairs` and `next` differ from `22a0992f` by less than their runs differ from each other, by the blocks under "What the build found".
  A value that is refused is walked once more, by the walk that names the fault: a table of 300,000 string keys with two functions among its values was refused in 74 ms to 126 ms, over four VMs, and in 144 ms in a review's one VM.
- `tools.offer` counts an array with no `__len` by one read for each entry, where it took `#t`: 78 ms for 4,000,000 integers and 128 ms for 4,000,000 strings in a release build, which are then refused for their count.

## Decided before the build

- An entry of "Left alone" in the first plan is closed here where the fix brings no fault of its own.
- The guide in `runvil-docs` changes with the code.
- Comments are written in this change wherever they help.
- One plan covers the whole change, and it is the first commit.

## Decisions

- **A replacement holds no protected call around the author's code.** A `pcall` in the replacement catches a failure of a metamethod and raises it again.
  The frames of the metamethod are gone by then: a `<close>` variable in it closes before the handler of the author's `xpcall` runs, and the Engine reads the failing line from the raise in the replacement, which is the line of the call.
  `tostring(value)` with a `__tostring` that holds such a variable and fails gave the handler a closed variable, where stock Lua gives it an open one.
  A failure that the metamethod raised with no position in it was placed on the line of the call; a review of the first plan's change reported that, and it was not run again on the code before the fix.
  The `pcall` was there to keep the chunk's position off a failure that Lua raises in the chunk.
  The rule on the position of an Engine chunk now does that: Lua raises its own refusal in the replacement, with `?:?: ` in front, and the rule takes that off wherever the Engine reads the failure.
  A first form of this change asked the Engine before each read, each write, each call of a `__tostring`, and each comparison whether Lua would refuse it, and raised the refusal itself with no position.
  With those asks taken out, the comparison with stock Lua gives the same answers, so they are out, and with them a Rust call for each item of a list with a metatable.
  Two asks stay.
  Of a `__len` or a `__lt` that cannot be called, Lua says in a Lua function which metamethod it was, as in `attempt to call a boolean value (metamethod 'len')`, and in a C function it does not, so no rule on the front of a text gives stock Lua's words.
  The replacement therefore asks the Engine whether Lua can call that metamethod, before it takes a length and before it compares two values that are not both numbers or both strings: the Rust side reads the metamethod raw, as Lua finds it, and follows a `__call` chain as Lua does.
  The ask and the operation are next to each other, with none of the author's code between them.
  The cost of the cut is what a `__close` handler reads, which "Known" states.
- **A replacement has no debug info and calls the author's code from its own body.** A Lua function with no debug info has no line, and `error(text, level)` puts nothing in front of the text for it, as for a C function.
  The chunk of the replacements is compiled that way, so level 2 from a metamethod has no position, as under the stock function.
  Level 3 is the author's call only when the replacement's frame is the one frame between the two, so a replacement calls a metamethod or a comparison function in its own body and never in a function it calls.
  The check of what a `__tostring` returned is a function over the result, and the whole merge sort is the body of `table.sort`.
  A stock C function between the replacement and the metamethod, such as the stock `tostring`, gives level 2 no position and level 3 the replacement, so it is no fix.
  A coroutine around the metamethod has no frame at level 3 and unwinds before the author's handler runs.
  The calls of the stock functions stay in a chunk with debug info, `__impl_stdlib_stock.lua`: the position that Lua puts on a failure there is how `from_here` knows the failure for the stock function's own.
  Lua names a stock function in its argument error by the upvalue of that chunk it was called through, so `raise_from_here` makes such an error again, with the name of the author's call.
  With no debug info Lua also names no variable of the replacement in a refusal: a `__tostring` that is a number fails with `attempt to call a number value`, as under the stock function, where a chunk with debug info adds `(local 'rendering')`.
  The cost is in the Engine's own diagnostics: a traceback names no line of the chunk, and a failure that Lua raises in a replacement starts with `?:?:`, which the rule on the position of an Engine chunk takes off.
- **`table.sort` reads and writes a list with a metatable one item at a time.** `table.move` is a C function, so an `__index` or a `__newindex` under it sees the replacement at level 3, where the stock sort has the author's call.
  The sort therefore reads and writes such a list as plain Lua in its own body, one item at a time.
  A list with no metatable has no code of the author's to run, and goes through `table.move`.
  The comparison function can give the list a metatable or take it away, so the sort looks again before the writes.
  The default comparison is a plain `a < b` when the elements are all numbers or all strings, which Lua compares with no metamethod, and for every other pair the sort first asks the Engine whether Lua can call the `__lt`.
- **`table.sort` takes the list the stock function takes.** The stock function takes a value with `__index`, `__newindex`, and `__len` that is no table, and names a refused argument by the `__name` of its metatable.
  The replacement of the first plan took a table only and named a comparison function that is no function by `type`.
- **`table.concat` hands a list with no metatable to the stock function first.** The stock function runs none of the author's code over such a list, so it may run under `pcall`.
  When it fails, for a value with `__tostring` or for a value it refuses, nothing has happened that the author's code can see, and the replacement starts again with its own loop.
  The loop reads each element as plain Lua, so a call that waits works inside the `__index` of the list, as with the wrapper on `bdcadfbb`.
- **`table.concat` knows the stock function's refusal by its position.** The stock call is in a chunk with debug info, so Lua puts that chunk's position on a refusal of the stock function, and `raised_here` tests for it.
  An abort is a value that is no string, and a failure with no position was not raised there, so neither starts the join again.
- **`table.concat` and `table.sort` still measure by `#list`.** Both are the Engine's own functions and could count the list as `listed` does.
  `table.insert`, `table.remove`, `table.unpack`, and `#list` itself cannot, so a block that mixes them would then see two lengths of one table, and each call would cost one read for each item.
- **`%s` asks for an argument's text when the stock function reaches it.** The replacement walks the conversions in order, and before it runs a `__tostring` it gives the stock function the part of the format in front of that conversion.
  A `__tostring` therefore runs only when every conversion before it is accepted, as under the stock function, and the results of the parts are joined.
  A stand-in that holds the text takes the place of the argument for the stock function, so the stock function asks nothing of the author's value or of the string metatable.
  In the first plan the stand-in had a `__tostring` of its own, which the stock function called when it reached the conversion.
- **An argument error reads the name of the call from Lua's stack.** Lua finds the name of a function in the code of its caller, and gives it to C code through `lua_getinfo`, which is how a stock function names itself.
  A section VM has no `debug` library, so a replacement asks a Rust capture, which reads the frame of the replacement through `mlua` and returns the text, and nothing else of the stack.
  The capture takes the level of the replacement, its name in the library, the number of the argument, and the reason, so the nine functions share one wording of stock Lua's text.
  A first form of this change kept one name for each function, the field of the library, and stated the difference under "Known"; a review showed that a read of the stack gives stock Lua's name for every call but those below.
  A replacement that is tail-called has a frame with no name: Lua keeps none for a Lua function that it tail-calls (`getfuncname` in `ldebug.c`), where a stock function that is tail-called keeps the frame of its caller.
  The name is then the field of the library, as in `sort`, which is what stock Lua has for `return table.sort(5)`.
  Where no Lua code makes the call, as under `pcall(table.sort, 5)`, the frame has no name and was not tail-called, and the name is the one in the library.
  Stock Lua finds that name in its table of loaded libraries when the call fails, and the capture is given it, so the two differ once a block has moved the function to another field.
  Under a `__call` metamethod Lua puts the called value in front of the arguments and tells C code how many values it put there, `extraargs` of `lua_Debug`.
  `mlua` hands that count on only in `mlua-sys`, where a read of it is unsafe code, and the workspace denies unsafe code in this crate, so the count of the capture has that value in it.
- **The chunk of the guard of the globals has no debug info.** Lua names the variable that holds a value it cannot index, and with no debug info it has no name to give, as in the chunk of the replacements.
  A test of the handler's type in the guard would have to tell every value that Lua can index, a string and a value with an `__index` of its own among them, and would word Lua's refusal a second time.
  The guard raises its own refusals with `error` at the level of the author's call, which reads no debug info of the chunk, and it calls a handler that is a function as a tail call, so a failure of that handler has no frame of the guard under it.
  The cost is in the Engine's own diagnostics, as for the replacements: a traceback names no line of the guard.
  A text that a `pcall` catches has no position, where stock Lua has the line of the read, which "Known" states.
- **The position of an Engine chunk goes by a rule on the text.** A stack overflow has the position of whatever function Lua was in, so no choice of which chunk keeps its debug info ends it.
  The rule is on the text, as the rule on an address is: every chunk of the Engine has a name that starts with `crates/runvil-internal/lua/src/`, a test holds each name to that, and `?:?:` is what Lua writes for the chunk with no debug info.
  It is applied where the address rule is, to a text that leaves the crate, and to a string that the Engine's `pcall` or `xpcall` caught, so the author's handler and the failure of the block read one text.
  A protected call around a replacement would take the position off as well, and would close the variables of the author's code before the author's handler ran.
  The cost is a text of the author that starts the same way, which loses that start and nothing else.
- **The rule on an address starts where Lua's text starts.** The start of the text is the start of what Lua raised: where `mlua` put its `runtime error: ` in front, which the kind of the failure tells, the rule reads what follows it.
  A rule that took those words off any text it was given read the name `runtime error: Thing` as the name `Thing` in a failure's prose, and kept it whole in the failure's message.
  It also read the label itself as a name: the message of `error('0x1f', 0)` was `error object is a runtime error value`.
- **A weak table is never a view.** A table is a view while it is empty, and a weak table is empty or not as the collector leaves it.
  An empty weak table with `__index` and `__metatable` was read as its `__index` table, and was refused as weak while it still held an entry.
  The read through views makes the check, so that each reader gets it and none can read a view without it.
- **A table under a weak metatable is refused, not read as its own data.** Read as its own data, an empty table with an `__index` would convert to an empty object with no word to its author, while `t.x` in the block still read the data.
  The refusal has its own wording, `WEAK_METATABLE`, since the table's own metatable sets no `__mode`.
  One level is enough: whether a metatable is weak comes from a `__mode` that is a string, and the collector takes no string out of a weak table.
  `models.use`, the options of `json.encode`, and a prelude's environment ask their table's metatable for nothing, so they keep the test of a weak table alone.
  The attributes and the parameters of `tools.adhoc` are read through a view since `cc55a351`, and that read asks the metatable what the table is, so each is refused under a weak metatable with the readers of a view.
- **`tools.offer` tests its array after each call of the author's code.** The test is two reads of a metatable and a scan of each `__mode` they find, and the array has at most 100,000 entries.
  One test before the walk would pass an array that an `__index` made weak for the reads between and plain again before the last.
  A read of an entry that the table itself holds runs none of the author's code, so no test follows it.
  With a test after every read, an array with no `__len` was refused once its `__mode` passed the memory limit divided by the count of its entries, 672 bytes for 100,000 entries at the default limit, by a review's run: a table that Lua does not treat as weak, and whose read runs no code of the author.
- **The tables on the way of a read by key are counted, not kept.** `tools.offer` reads each entry that its array lacks, and one `prose` each global of a placeholder that the globals lack, in one Rust call that no cancel interrupts, and the check follows the way of each read anew: the code of an `__index` at the end of a way can change every table of it.
  Each table followed costs the call one value of its budget, as a view that a walk passes costs it, so one call follows at most 8,388,608 tables at the default limit, and the array or the global past that is refused.
  The entries that one table behind an array holds always fit: each takes 9 bytes of the heap or more, and the budget has a value for every 8 bytes of the limit.
  A check of a `tasks` function or of `models.loop` is one read in a Rust call of its own, and follows at most 2,000 tables.
  A walk that kept the tables of a way from one read to the next was built and measured, and is not in the change.
  It made one read of a kept table where the check makes five, but it had to follow the way anew after each read that a function answers, so 100,000 entries that a function gives behind 1,999 tables took the 21.3 s that they took with no table kept.
  It also had to keep its tables in a Lua list that a Lua function read, since a Rust reference to each of 2,000 tables makes `mlua` panic close to the memory limit ([mlua-rs/mlua#733](https://github.com/mlua-rs/mlua/issues/733)).
  The count holds every shape to one bound with no state between two reads.
- **The scan of a `__mode` is counted, not remembered and not cut short.** Each read that tests a table reports the bytes it scanned, and the walk it is part of spends them as it spends the bytes of a string, so the scans of one call are at most the memory limit and the one scan that passes it.
  A walk that remembered the answer for a `__mode` by the string's address would need the string to stay where it is: `tools.offer` runs the author's code between two tests, which can put another string at that address, and a walk that held each string to keep it there would hold a Lua reference for each.
  A limit on the length of a `__mode` would refuse a table that Lua does not treat as weak, or call a table weak that is not: Lua reads a `__mode` of any length.
  The count changes one result: a value that holds more bytes of `__mode` than the memory limit, counted once for each table that has it, is refused where `bdcadfbb` converts it.
  The answer is the same in every run: what a table costs comes from the table alone, and both walks of the conversion to data count it for every table they meet, read or not.
- **A read asks a metatable by a name made once.** `mlua` puts a name given as Rust text on Lua's stack under a protected call when the VM has a memory limit, and a string that the VM holds with none.
  The test of a weak table is one more read of a metatable, and by Rust text it took 0.09 µs to 0.15 µs for each table that has one: `json.encode` of ten lists from `json.decode` took 11% to 14% more than on `22a0992f`, and `next` on a table of `var` 10% to 12% more.
  So a walk keeps the names `__mode`, `__metatable`, and `__index` as Lua strings, each from the first metatable it asks by it, and `pairs`, `next`, and the check of the `tasks` functions keep theirs for the life of the VM.
  A walk that asks no metatable makes no name, so a value with no table in it pays nothing.
  A name kept in the VM's app data was built and measured first: the lock, the lookup, and the copy of the string at each read left 45 ns to 80 ns of the cost for each table, and it was taken out.
  The reads of `__metatable` and `__index` were by Rust text on `22a0992f`, so with the names a read of a table that has a metatable takes less time than there.
  The other reads of a metatable by Rust text, such as the `__pairs` that `pairs` asks for, are as on `22a0992f`.
- **A weak array of a parameter's type and description is refused whatever it holds.** With a description that is a table, the array was refused while the table was there and taken as a type alone once the collector had removed it.
- **The keyed members of a collection go in the order of `pairs`.** Issue #69 gives three ways to end the two orders: say that they differ, sort the keyed members as `pairs` does, or refuse a collection with a gap.
  The second is the one taken.
  The walks cannot take the order of the fanout: `next` has no state, so the place of a key cannot follow where the list ends.
  The fanout counts its list, so it can take the order of the walks, and `WalkPlace` is then the one place of a key in every walk of a table.
  The cost is that the number keys of a collection are no longer in the order of their values: `7` comes before `2.5`, as it does in `pairs`.
  A refusal of a keyed member still names the one whose key comes first by `FirstRefusal`, the one rule of every function that checks a table, and `json.encode` still visits keys by `SortKey`.
- **A list with a gap is refused, not read by a rule.** A rule that ends the list at the first gap drops the items past it, and a rule that fills the gap with nulls makes items the author did not write.
  `json.encode` and a write to `var` refuse the same table in the same words, so the conversion does too.
- **A table with a gap and no item 1 is named as a list with a gap.** `{ a = 1, [5] = 2 }` has no list by `mlua`'s rule, which refuses its integer key, when its length is 0.
  Lua does not promise that length for a table whose item 1 was removed, so one rule covers every table with a positive integer key: its integer keys are 1 to their largest, or it is refused.
- **The fault is found by a second walk.** `mlua` can sort the keys of a map before it reads them, and that holds a Lua reference to every pair, which is the panic of `models.use` that the first plan describes.
  The second walk runs only for a value that is refused, holds one refusal, and gives a key `mlua`'s text for a table that holds that key alone.
- **The second walk counts text, read or not.** It reads every string that the conversion reads, each time the string occurs, where the conversion stops at its first fault.
  Without a count, the walk checked each of 4,096 references to one string of 8 MiB that is not UTF-8, and `mlua` checked each again for its text, 64 GiB in all, which took 1.27 s in a release build.
  With the bytes of each string counted before it is read, the same list is refused in 1.7 ms, as too large.
  What the conversion reads under a table with a gap follows `#t`, so a count of the strings it reads would differ between two runs.
  The walk counts every string it visits, and checks for UTF-8 only those the conversion reads.
  The text of `mlua` for a string that is not UTF-8 does not depend on the string, so the walk asks for it with a string of one byte and reads the author's string once.
- **Both walks of the conversion count text.** The data is a copy of the strings outside the VM's memory limit, so the count is on every conversion and not only on the one that names a fault.
  It refuses a value that converted before: one whose strings, each counted every time it occurs, hold more bytes than the limit.
- **Neither walk goes into a weak table, read or not.** A walk that went into a weak table only where the conversion does not read it spent a budget that followed `#t`.
  With a weak table larger than the budget past a gap, one VM answered `the value is too large to convert` and the others named the gap.
- **The first walk passes every table that the conversion reads.** `mlua` converts what the walk hands it, so a table that the walk hands on unwalked is unchecked.
  The walk handed on three: a view only a function can read, a chain of views that leads back to itself, and a table already on the path.
  The first two convert to an empty object, so 127 tables around one made data 128 deep.
  The third is a table that holds itself: where the walk had copied the data of a view above it, `mlua` did not know the table for one it had met, read it a second time, and made data 171 deep from a value whose tables nest 86 deep.
  A view of the first two kinds is now walked as the empty table it is, and a table on the path is refused where the conversion reads it, as `recursive table detected`.
  A table that holds itself through a view converted before, to data that ended in an empty object at the view.
- **A record view counts as the data it converts to.** A view of a message record converts to the record's JSON form, and that form can hold what an earlier conversion made of another view.
  Counted as one value, a view let each record nest four levels deeper than the one before it: 40 such records made a record 161 deep, whose JSON text does not read back, and 3,000 overflowed the stack of the process, on `bdcadfbb` too.
  Both walks take the size of the form from the record, without building the form: its nesting adds to the tables the view sits under, its values are spent from the budget, and the second walk spends its bytes as it does those of a string.
  The second walk does not convert the view, which always converts.
  The record's JSON and its size are made from one description of the form, so the size is that of the JSON.
- **The depth of a record is checked where the record is parsed.** `parse_record` is the one way a record of Lua's enters the list, from a builder, `append`, and `replace`, and it has the whole record.
  A check on each field of a builder with a smaller limit would be three limits for one rule.
- **The size of a record is checked with its depth.** The limit is the budget of the conversion that reads a view, so a record a builder takes is one whose view converts, at any memory limit.
  A view that cost nothing would let 150,000 references to one record convert to 150,000 copies of it.
- **A key with no place is refused by its type alone.** Two keys of one type that are tables, functions, userdata, or threads tie in the shared order, so `FirstRefusal` keeps the one that Lua's own walk met first.
  The refusal of such a key may then say nothing of the key but its type, and `mlua`'s says more.
  The Engine's text for it is the one `json.encode` has for the same key.
- **A fault is handed up as data and worded once.** The second walk visits every value for the count, so it finds every fault, and `FirstRefusal` drops all but one for each table.
  A fault with a place is its reason and the steps to it, and takes one step at each table that hands it up: at most 64 bytes of a key and the note of its length.
  The path is written once, for the fault the conversion reports, so a dropped fault costs one step and not a path.
  The value of the problem above takes 0.65 s, against 0.59 s without the 125 tables.
- **The depth limit is that of `json.encode`.** 127 is the deepest value whose JSON text `serde_json` reads back, and a recorder stores that text.
- **A long key is cut, not refused.** A key of any length is valid data, and only its print in a message has a limit.
- **`serde_json::Value` reads neither a table nor JSON text.** Its rule for the key `$serde_json::private::RawValue` is on in every build where one crate asks for the `raw_value` feature, and `runvil-lua` cannot turn it off.
  `PlainJson` reads what `serde_json::Value` reads, without that rule, and a failure has the same words.
  That holds in a build without `serde_json`'s `arbitrary_precision` feature, which no crate of the workspace asks for, by `cargo tree`; "Left alone" has the other build.
  20,000 texts from a fixed seed, valid and broken, gave the same data or the same failure under both readers.
  Of 4,000 Lua values from a fixed seed, 3,840 gave the same result under both, and 160 a failure for a key that is no string in other words, which the conversion reports in its own words before either.
  The tests of `runvil-lua` ask for `raw_value` themselves, so the rule is on in a build of the crate alone.
- **The fields of a raised error table are read by name.** A walk of the table costs what the table holds, and a read of the facts of its code costs at most five reads.
- **`var` itself is the first level.** The limit is on what a recorder stores: `TaskStarted` carries `var` whole, and `serde_json` reads back 127 levels of JSON text.
  Counted from the value written, as on `22a0992f`, the limit lets one write make a `var` of 128 levels.
  The cost is that `var.x = value` refuses a value 127 deep, which `json.encode(value)` takes.
- **A table of `var` keeps one step, not its path.** A path as text costs each table the length of its path: under 126 tables with keys of 64 bytes a path is about 10 KB, so a list of 100,000 empty tables there would hold about 1 GB of paths outside the VM's memory limit, which was reckoned and not run.
  With one step and the place above it shared, a table holds at most 64 bytes of its key and the note of the key's length.
  A refusal walks the places above its table for the path, at most 127 steps.
  The read-only view of `var` that a Pack prelude gets keeps the same place for each table read through it, and writes the path and the text of a refusal only to refuse.
  A write that is taken walks none: the conversion gets a function that writes the path of the target, and calls it only to refuse.
  A path handed over as text, built at each write, made a write under 120 tables cost more than three times one at the top of `var`.
- **A key that is not UTF-8 is refused in the words of the conversion.** A value written whole with such a key inside is refused by the conversion, as `var.s.<key> is not valid UTF-8`, so the key of the write itself has the same text.
- **The metatable of strings is taken once.** `mlua` gives the metatable of a type only through a push of a value of that type, and it makes that push with no protected call.
  A read through the registry would need a key that the Engine puts there, which is the same one table held another way.
  The table cannot change: `setmetatable` takes a table as its first argument, and a section VM has no `debug` library.
  The chunk of the replacements gets the same table as an argument, and reads the `__tostring` of a string from it with `rawget`, as Lua finds a metamethod.
  The hardening takes `rawget` out of the globals, so it installs the replacements first, and still after the guard of the globals and the walk by key.
  A first form asked the Rust capture about each string, and `tostring('abc')` then took about 0.22 µs a call in a release build, against 0.17 µs on `22a0992f`, as a review found; with the raw read it takes 0.09 µs.
- **A capture reads a metamethod by a name made once.** `mlua` pushes Rust text as a Lua string under a protected call when the VM has a memory limit, and pushes a name that is a Lua string already as it is.
  So the captures share the names of the five events they read, made when the replacements are installed, beside the metatable of strings, and the app data of the VM no longer holds that metatable.
  `tostring`, `%s`, and `table.concat` get the `__name` of a metatable as the string it is, and a nil for a value that prints as its type, so the capture makes no string for either; it made a copy of the name for each call.
  `mlua` reads the metatable of a userdata by Rust text alone, so that read keeps its protected call.
  Its walk of the fields of that metatable makes no protected call, and was not built: a sort of userdata values would walk a metatable at each comparison, for a failure that differs inside a nest of about 196 metamethods alone.
  A test of a caught text for `C stack overflow`, to make the string of it again, was not built either: it is a rule on one text, and a Rust function of the Engine that meets that limit, such as `json.encode`, gives an error value on `22a0992f` too.
- **The stack of `mlua` grows before the run's limit, and the names stay held.** The captures hold the names of five events and the metatable of strings, and `pairs`, `next`, and the check of the `tasks` functions hold up to three names each from their first use, so that a read of a metatable makes no string and no protected call.
  Each is a value that Rust holds for the life of the VM, and the install of the coroutine shims holds two values more than on `22a0992f` while it runs.
  A capture that held none of them would make each name at each call, under the protected call that the rule on a capture takes out; a read of each from the registry at each call was not built or measured.
  So a new VM takes the stack to its next size where the growth cannot fail: 68 slots hold the 32 values of a build and 36 tables, where the 34 slots of `22a0992f` hold 24 values and 10 tables.
  The stack doubles, so 68 slots are the least that hold the build of this change with the tables that `22a0992f` takes, and they cost each VM 528 bytes of its heap.
  A larger stack moves the count of tables at which it grows and leaves the fault, which is `mlua`'s: its 0.12.3 has a growth that returns an error, for a call that can return one, and that upgrade is [#64](https://github.com/cppalliance/runvil-harness/issues/64)'s to make.
- **A replacement that runs a metamethod writes no name.** Lua names a function that its code calls for an operator by the event, as in `index`, and a replacement is Lua code.
  Stock Lua has a C function there, which writes no name, so `argument_error` takes a caller with no debug info for one: the chunk of the replacements is compiled so, and no code of an author's or of a Pack's is.
  The guard of the globals is compiled so too, and it runs a handler that is a table for the author's own read or write, which stock Lua names by the event.
  So the capture tells the guard's two functions apart, by a raw read of the metatable of the globals, which no code of a section VM can reach.
  A replacement that the guard tail-calls, as it calls every handler that is a function, has no frame of the guard to be told by, and "Known" states that call.
- **Each chunk name has a test that holds it to the opening and to 59 bytes.** Lua's limit is a constant of its build, `LUA_IDSIZE`, so one test also raises an error under a name of 59 bytes and of 60 and reads what Lua wrote.
  The Engine loads ten chunks as text: eight files, and two Rust strings, the recording `yield` of the shims and the `__concat` of an error value.
  Each name is a constant that a test holds, as the seven names of the files were on `22a0992f`: the names of the two chunks of the replacements share one test, so the ten names have nine, and the one check they share now holds a name to the 59 bytes too.
  A chunk with no name compiles and runs like any other, and no test finds a new one.
  A scan of the crate's code for a load of text with no name was built and taken out: a review passed a load by it in three ways, by a name with no `@`, by an argument spelled as the allowed load spells its own, and by a load called through a variable.
  A lint that refuses `mlua::Lua::load` is set for the whole workspace, where the tests of `runvil-lua` and of `runvil-runtime` call it, or in a `clippy.toml` of the crate, which would take the place of the workspace's; that was not tried.
- **The chunk of the `__concat` loads when the VM is made.** The hardening of a VM builds the error metatable after it installs the replacements, so the chunk takes the Engine's `tostring`.
  Built at the VM's first failure, as on `22a0992f`, it took whatever a block had put there, and the rule that no function of an Engine chunk reads a global when it is called did not hold for it.
  The chunk could take `tostring` as an argument instead, and would still load at a moment that a block chooses.
- **A `tasks` function asks about a table before it reads it, in Lua's own read.** The set, the handle, and the options are read by index and by key in the shim, so that a view of `var` works as a table does, and a Rust capture says whether the read may look into the table, by the read through views that every other reader uses.
  The check of the options comes before the walk of their keys, since a `__pairs` of the table would keep the table from the Engine's `pairs`.
  The check comes before each read of the table, with none of the author's code between the two, as in `tools.offer`: the `__index` of a member, of the set, or of the options can make the table weak before the next read.
  That is one Rust call for each member of a set, two for a member that is a handle, and four for the options of a spawn.
  The chunk takes `ipairs`, `select`, `tostring`, and `type` as locals when it loads, before any block runs, as it took `pairs`, so a block that replaces one of those globals has no code of its own between a check and a read.
  `__impl_coro.lua` and `__impl_fanout.lua` take the globals that their functions still read, so the rule is one for the eight chunks that are files, and `luac -l -l` lists a read of a global in the main function of each and in no other.
  The two chunks that are Rust strings are two lines and one line: that of the recording `yield` reads no global, and that of the `__concat` reads `tostring` in its main function alone.
  The rule has no test of its own over every chunk: a test needs the `debug` library or a reader of bytecode to see what a function reads, so two tests call the `tasks` functions and a fanout under replaced globals.
- **A read by key is checked along the way that Lua takes, and stays Lua's read.** The review asked for a check of the tables that the lookup visits.
  A raw read in place of Lua's would end the fault too, and would change what a prompt can pass today: options that take a default through an `__index` table, a set that is a view of `var`, and a `compactors.fail` given through a metatable, as "Left alone" has it.
  A refusal of every `__index` that is a table would refuse the same prompts.
  So the read stays Lua's, and one function of `weak.rs` goes where Lua goes, before the read, with no call into Lua.
  It asks each table before it looks for the field in it, so the way up to a refusal is through tables that the collector takes nothing out of, and the refusal is the same in every run.
  It ends at a function: what the author's code reads is the author's read, as `t[k]` in a block is.
  It follows a string to the `__index` of the metatable of strings, since a block can set that field to a table, and it refuses a metatable of strings that is a weak table itself, whose `__index` is there or not as the collector leaves it.
  A userdata on the way is the Engine's, since `setmetatable` takes a table alone, and the walk ends at it.
  Its `__index` is a function, or a table that `mlua` made with no metatable, and `getmetatable` gives `false` for a userdata, so no code of the author reaches that table; this was read in the source of `mlua` 0.12.0.
  Its length is Lua's own, 2000 values past the table the read is written for, with the views that the read passed counted among them: a shorter way would pass a weak table that Lua reaches, and a longer one would refuse for a weak table a read that Lua gives up first.
  The check costs a `tasks` function no Rust call more, since the capture that asked about the table now takes the key too, and for a field that the table itself holds it adds one raw read.
  The set is read in a loop of the chunk's own, where `ipairs` read it, so that the check before a read knows the index of that read.
- **A placeholder's global is checked on the guard's way.** The guard of the globals is a function to Lua, so the walk from the globals would end at it.
  The guard reads `handler[key]` for an `__index` of the author's that is no function, so `globals.rs` asks about that read: the author's `__index` first, and 2000 values past it.
  It refuses an author's metatable that is a weak table itself, since the guard reads the `__index` out of it.
- **The byte limit of a record is the conversion's.** The limit on values is the budget of the conversion that reads a view, and the bytes of text are the other part of that budget, counted by the same `JsonSize`.
  A record within both, and within the depth, is one whose view converts.
- **The mark of a load under way is a function.** Lua compares two functions by identity, and calls no metamethod for a function and a table, so `==` is a raw comparison once one side is a function.
  A capture of `rawequal` would do the same with one more argument of the chunk, and the mark needs none.
- **`ipairs` and the functions of `table` take a weak table.** A read by index is a read by key, which a weak table allows, and `table.concat` and `table.sort` do what the stock functions do.
  A refusal in those two would leave `t[i]`, `ipairs`, `table.unpack`, `table.insert`, and `table.remove` as they are, as with the length of a table with a gap.

## Code

All in `runvil-harness`, in `crates/runvil-internal/lua/src/` of `runvil-lua` where no other place is named.
Neither `public-api.txt` listing changes.

1. **The replacements as one frame.** New `__impl_stdlib_stock.lua` holds the calls of the stock functions, with its debug info, and `__impl_stdlib.lua` holds the replacements, with none.
   `stdlib.rs`: `install` runs `__impl_stdlib_stock.lua` and hands the table it returns to `__impl_stdlib.lua`.
   `program.rs`: `compile_internal_stripped` compiles an embedded chunk with no debug info, and shares `compile_embedded` and `compile_chunk` with `compile_internal`.
   `__impl_stdlib_stock.lua`: `from_here` moves a failure that Lua raised in the chunk of the stock calls to the author's line and passes every other failure on as it is.
   `__impl_stdlib.lua`: the merge sort is the body of the replacement of `table.sort`, with its reads, its comparisons, and its writes.
   `stdlib.rs`: `length_refusal` and `order_refusal` say whether Lua can call the `__len` of a list and the `__lt` of two values, through `call_refusal`, which follows a `__call` chain as Lua does, and `is_bare` says whether a list has no metatable.
   `stdlib.rs`: `unaddressed` reads the `__name` and `__tostring` of a metatable, as Lua code cannot for a userdata, and cuts the name at its first zero byte.
   `stdlib.rs`: `Events` holds the names of the five events that the captures read, as Lua strings, and `metamethod` reads the metatable of a table by one; `Shown` is what `unaddressed` answers, the `__name` as the string it is among them.
   `string.format` hands the stock function a stand-in for each argument that can print with an address or has a `__tostring`, one part of the format at a time.
   `tostring`, `string.format`, and `table.concat` each call a `__tostring` in their own body, and `text_of` checks what it returned.
   `type_refusal` names the type of a refused argument as stock Lua does, by the `__name` of its metatable, for the list of `table.concat` and for the list and the comparison function of `table.sort`.
2. **The label of a failure.** `error.rs`: `lua_text` tells by the kind of the failure whether `mlua` wrote the `runtime error: ` in front, and the rule on an address reads what follows that label.
3. **A weak table in every reader.** New `weak.rs` holds the test of a weak table, which is no part of the order of keys: `is_weak`, which `order.rs` held, `WEAK_TABLE`, the one wording of the reason, and `refuse_weak`, which raises the wording for a weak table.
   `models.rs` calls `refuse_weak`, and `json/encode.rs` calls `is_weak` and puts the wording after its own words for the options.
   `tools.rs`: `read_table` reads the attributes and the parameters of `tools.adhoc` through `view_data`, where `cc55a351` read them through a view with no test of a weak table, and puts the words of `tools.adhoc` in front of the reason that `view_data` returns.
   `weak.rs`: `weakness_of` gives the wording for a metatable that makes its table weak, `WEAK_TABLE`, and for one that is weak itself, `WEAK_METATABLE`.
   `proxy.rs`: `unseal` ends at either with `Unsealed::Weak`, which holds the wording, before it looks for an entry in the table or asks the metatable for a field, and `view_data` and `view_data_within` return the wording for one.
   `tools/offer.rs`: a `refuse_weak` of its own makes the test of `weakness_of` for the array before the first read, after the call of `__len`, and after each read of an entry that the table itself lacks, which is a call of `__index`.
   `json/encode.rs`: `debug_option` refuses weak options.
   Every reader of a view therefore refuses a weak table that is the table given, a view on the way, or the data: `json/encode.rs`, `data.rs`, `iteration.rs`, `collection.rs`, `tools/decode.rs`, and `markdown/walk.rs`, which did not refuse one before.
   `weak.rs`: `scan` reads a `__mode` and says how many bytes it took, and `weakness_of` hands that count to its caller.
   `walk_budget.rs`: `ReadCost` is what `unseal` asks its caller to spend, a view or the bytes of a `__mode`, and `charge_read` spends it.
   `weak.rs`: `ModeBudget` counts the bytes of `__mode` that one call scans against the byte budget of the VM, which it makes at the first `__mode` that has bytes, and past it gives the text of the refusal: the bytes that the call scanned, reckoned from what `bytes` of `walk_budget.rs` says the budget has left, and the limit.
   `proxy.rs`: `view_data` takes the VM, counts the bytes against a `ModeBudget` of its own, and returns the reason for a table that the read must not look into, which past that budget is that text.
   `iteration.rs`, `collection.rs`, and `coro.rs` put their own words in front of that reason, and `refuse_weak` of `tools/offer.rs` counts against a `ModeBudget` for one read of an array and refuses with the same text past it.
   New `meta_names.rs`: `MetaNames` keeps the names `__mode`, `__metatable`, and `__index` as Lua strings of the VM, each made at the first read by it.
   `weakness_of`, `unseal`, `view_data`, and `view_data_within` take the VM and a `MetaNames` from their caller.
   The walks of `data.rs`, `json/encode.rs`, `markdown/walk.rs`, `tools/decode.rs`, and `tools/offer.rs` keep one for the walk, the `next`, `pairs`, and `weakness` functions that `iteration.rs` and `coro.rs` install keep one for the VM, and `collection.rs` makes one for its one read.
   `iteration.rs`: `walked` reads through a view and refuses a weak table, where `refuse_weak` refused the table given.
   `tools/decode.rs`: `declared` reads a parameter's value raw and refuses a weak one, where `type_and_description` read it raw.
4. **The keyed members of `tasks.fanout`.** `order.rs` holds `WalkPlace`, which `iteration.rs` held, and `keyed_member` in `collection.rs` gives the place of a key by it.
5. **The array of `tools.offer`.** `tools/offer.rs`: `array` counts an array with no `__len` by `listed`.
6. **The unknown keys of `tools.adhoc`.** `tools.rs`: the unknown keys of the attributes go to `FirstRefusal`, where they went to a sorted list of their texts.
7. **The conversion to data.** New `data.rs`: `data_of`, `plain` with its walk, which moved from `proxy.rs`, and `Faults`, the walk that names the fault.
   `messages.rs`: `viewed_size` gives both walks the size of the record a view stands for, a `JsonSize` that `protocol/record.rs` makes from the same description as the record's JSON.
   `protocol/record.rs`: `parse_record` refuses a record whose `JsonSize` is more than `DEPTH_LIMIT` deep, or holds more values than the count its caller gives.
   `messages.rs` gives it `WalkBudget::values`, what one conversion on the VM may spend.
   `Reads` says which entries of a table the conversion reads, and `Items` finds a gap by the count of the positive integer keys and the largest of them.
   `Fault` is what `Faults` hands up: a failure in the words of `from_value`, or a reason and the steps to its place, which `into_error` writes as a path once.
   `proxy.rs`: `key_step` gives one step of a path, and `key_path` uses it.
   `prose.rs`, `messages.rs`, `collection.rs`, `protocol/parse/tasks.rs`, and `protocol/parse/tools.rs` call `data_of`.
   `walk_budget.rs` holds `DEPTH_LIMIT`, which `json/encode.rs` held and `markdown/walk.rs` had a copy of, and `json/keys.rs` gives `missing_list_item` and `key_type` to both.
   Both walks spend the bytes of a string or a key through one `charge_text`, and those of a record through `charge_value`.
8. **Quoted keys and names.** `proxy.rs`: `quoted`, and `name_step` and `index_step`, the two steps of a path, which `key_step` is made of.
   `tools.rs` and `tools/decode.rs` quote a name, an unknown key, and an unknown type through `quoted`.
   `quoted` looks for the start of a character among the last four bytes it may take, and `adhoc_info` in `tools.rs` hands it the bytes of the name's Lua string, which `attr_text` now returns.
   `var.rs`: `guarded_json_value` writes the step to a table under a key or an index through the two steps.
9. **The fields of a raised error table.** `error_value.rs`: `raised_from` hands `field_facts` a read of one field by its name, and `field_facts` asks for the facts of the table's code.
   `raised_from` reads the `message` with `to_string_lossy` and takes a `kind` that is not UTF-8 for no kind, and `take_failure` in `coro.rs` reads the traceback with `to_string_lossy`.
10. **A key is a name.** New `plain_json.rs`: `json_of` reads a Lua value, and `json_of_text` and `json_of_each_text` read JSON text.
    `data.rs`, `var.rs`, and `protocol/parse.rs` call the first, and `json/decode.rs` and `protocol/answer.rs` the other two.
    `Cargo.toml` of the crate: the tests ask for `serde_json`'s `raw_value`.
11. **The position of an Engine chunk.** `chunk_line.rs`: `ENGINE_CHUNK_OPENING`, `engine_position_len`, and `without_engine_position`.
    `error.rs`: `raised_text` applies it and then the address rule, and `root_text` and the failure of a Pack prelude in `prelude.rs` go through `raised_text`, so `without_address` is private.
    `rendered` applies the two rules itself, to what follows the label of `mlua`, with the address rule in the shape of the platform it is given.
    `error_value.rs`: `normalized` applies it to a string that `pcall` or `xpcall` caught.
12. **`table.concat` and a failure that is not the stock function's.** `__impl_stdlib_stock.lua`: `raised_here`, which `raise_from_here` and `part_outcome` use.
    `__impl_stdlib.lua`: `table.concat` raises a failure of the stock call that `raised_here` does not take.
13. **The depth and the keys of `var`.** `var.rs`: `Place` is the place of a guarded table, with its step, its level, and the place above it, and `path` writes the path for a refusal.
    `var.rs`: the level that `c235fcd6` gave `guarded_json_value` and `guarded_table`, its `json_depth`, and its test of the two against the limit go, and `json.rs` hands `var.rs` no limit.
    `guarded_json_value` and `guarded_table` take a `Place`, where they took the text of a path, and `__newindex` hands `to_stored_json` the level of the value it writes and a function that writes the path of its target.
    `json/encode.rs`: `to_stored_json` starts at that level, where it started at 1, and `Encoder` calls the function only to refuse, where it held the path as text.
    `var.rs`: `write_refusal` refuses a string key that is not UTF-8 with `NOT_UTF8`, the wording of `json/encode.rs`.
    `prelude.rs`: `read_only_var` keeps a `Place` for each table of the view, where it kept the text of a path and of a refusal.
    `var.rs`: `var_snapshot_table` reads the `var` global raw, where an `__index` of the author's on the globals answered for a `var` that a block had taken out of them.
    `var.rs`: in a test build, `Place::path` counts the paths it writes in `PATHS_BUILT`, for the test of a write that is taken.
14. **The metatable of strings.** `stdlib.rs`: `Events` holds the metatable of strings, which `install` takes once, and `metamethod` reads the metamethod of a string from it.
    `install` hands the same table to `__impl_stdlib.lua`, where `tostring` and `string.format` read the `__tostring` of a string from it with `rawget`.
    `hardening.rs`: `harden` calls `stdlib::install` before it takes `rawget` out of the globals, where it called it after.
15. **The chunk of the `__concat`.** `error_value.rs`: `CONCAT_CHUNK_NAME` and `CONCAT_SOURCE`, and `metatable` loads the `__concat` under that name.
    `error_value.rs`: `install_metatable` builds the error metatable, and `harden` in `hardening.rs` calls it after `stdlib::install`.
16. **A weak table in the `tasks` functions.** `coro.rs`: `tasks_shim` runs the `tasks` chunk with a `weakness` capture, which gives the words that `view_data` has for a table it must not look into.
    `__impl_tasks.lua`: `refuse_weak` raises those words after the words of its caller, and `check_keys`, `option`, `task_id`, and `task_set` call it before each read of a table.
    The chunk takes `ipairs`, `select`, `tostring`, and `type` as locals at its start, `__impl_coro.lua` takes `select`, `tostring`, and `type`, and `__impl_fanout.lua` takes `error` and `ipairs`.
    `__impl_require.lua`: `cycle` joins the paths of a cycle with `..`, and the chunk no longer takes `table.concat`.
17. **The argument errors of `pcall` and `xpcall`.** `__impl_coro.lua`: `protected_call` and `protected_xcall` take their arguments as `...`, and each raises stock Lua's text at level 2 for an argument that the stock function refuses, before it calls that function.
    `stdlib.rs`: `install` makes the Lua function that names the type of a refused argument, gives it to the replacements, and keeps it in the registry under `MISTYPED_REGISTRY`; `mistyped` reads it there, and `coro.rs` hands it to the chunk for the handler of `xpcall`.
18. **The name in an argument error.** `stdlib.rs`: `argument_error` makes the Lua function that writes stock Lua's text for the replacement at a level of the stack.
    `stdlib.rs`: `stands_for_c` says whether the caller of that replacement is a function of an Engine chunk with no debug info and no part of the guard, and `is_guard` in `globals.rs` says whether a function is the guard's `__index` or `__newindex`.
    It reads the name of the call, the kind of that name, and whether the frame was tail-called, through `Lua::inspect_stack` of `mlua`.
    `__impl_stdlib.lua`, `__impl_coro.lua`, and `__impl_globals.lua` call it where they raise an argument error themselves.
    `raise_from_here` in `__impl_stdlib_stock.lua` and `named` in `__impl_globals.lua` call it for an argument error that a stock function raised, with the number and the reason of that error.
    `stdlib.rs`, `coro.rs`, and `globals.rs` hand the capture to their chunks.
19. **The guard of the globals with no debug info.** `globals.rs`: `GLOBALS_PROGRAM` compiles `__impl_globals.lua` through `compile_internal_stripped`.
20. **The stack of `mlua` before the run's limit.** `vm.rs`: `SectionVm::new` holds `HELD_AT_ONCE` handles of the globals at once and drops them, after it sets the default limit and before it hardens the VM.
21. **A read by key that looks into a weak table.** `weak.rs`: `weak_behind` follows the read of one field from an `__index`, asks `weakness_of` about each table before it looks for the field in it, and ends at a table that holds the field, at a function, and after as many values as its caller allows.
    `fallback_weakness` starts it at the `__index` of a table that lacks the field, for `INDEX_CHAIN` values less those that the read has passed, and `index_weakness` at an `__index` itself, for one value more.
    `leads_to` words what either found, and `WEAK_STRINGS` is the wording for a metatable of strings that is weak.
    `weak_behind` reports each table that it goes on to and each scan as a `Spent`, and `ModeBudget::spend` counts one: a table as one value of the budget, and a scan as its bytes, with the words of the refusal past either.
    `ModeBudget::counting` counts what a read spends against the budget and gives the words of a read that passed it, for `leads_to` and for `view_data`.
    `stdlib.rs`: `install` keeps the metatable of strings in the registry under `STRINGS_REGISTRY`, and `strings_metatable` reads it there.
    `proxy.rs`: `field_weakness` reads a table through its views, counts them, and asks `fallback_weakness` about the data, with one `ModeBudget` for both.
    `coro.rs`: the `weakness` capture takes a key, and with one it asks `field_weakness`.
    `__impl_tasks.lua`: `option` and `task_id` hand `refuse_weak` the key they read, and `task_set` reads its members in a loop of its own, with the check before each read, for the index of that read.
    `tools/offer.rs`: `array` asks `fallback_weakness` before the read of an entry that the table lacks.
    `globals.rs`: `read_weakness` says what a read of a global looks into, through `index_weakness`, against the budget of its caller, and `prose.rs` makes one budget for a render and asks it before the read of each placeholder's global.
    `models_loop.rs`: `weak_read` asks `field_weakness` before the reads of `compactors.fail` and `sys.taskid`, and `begin` refuses with its words.
22. **The bytes of a record.** `protocol/record.rs`: `parse_record` takes the bytes its caller gives, and `within_limits` refuses a record whose `JsonSize` holds more.
    `messages.rs` gives it `WalkBudget::bytes`.
23. **Comparisons by identity.** `var.rs`: `var_snapshot_table` compares the `var` global with the proxy as two values of `mlua`, which compares two tables with `lua_rawequal`.
    `__impl_require.lua`: `LOADING` is a function.

## Documents

- `decisions/prompt-language.md`: a new entry says that `tostring`, `string.format`, `table.concat`, and `table.sort` run a metamethod as stock Lua does.
  It states what `error(text, 2)` and `error(text, 3)` give in that code, that a stack overflow names the author's line or no position, and what the rule on an Engine position costs.
  It says that Lua raises its own refusal in the Engine's function, that a `__close` handler under a `pcall` reads that start, and under an `xpcall` what the message handler returned, and that the Engine asks only whether Lua can call a `__len` or a `__lt`.
  It says that an argument error names each of the four, `math.randomseed`, `pcall`, `xpcall`, `setmetatable`, and `getmetatable` as the call is written, gives the tail call and the `__call` metamethod that read otherwise than in stock Lua with the reason for each, and says that the Engine's `pcall` and `xpcall` refuse their own arguments in stock Lua's text on the line of the call.
  It says that a nest without end fails with `stack overflow` at the default memory limit, and can fail with `not enough memory` under a smaller one, and where the caught failure of a nest is an error value.
  The entry on a call that waits says where a metamethod under a replacement can wait.
  Three more entries are new: the Engine refuses a weak table wherever it reads the entries of one, a path and a refusal of `tools.adhoc` quote at most 64 bytes of a key or a name, and a table that the Engine converts to data has no gap, is not weak, and nests at most 127 deep.
  The second says that the quote leaves out a character that the cut would split and that other errors quote one key whole, and the third that NaN and the infinities convert to null.
  The entry on the length of a table with a gap names the five stock functions that still follow `#t`, and says why `table.concat` and `table.sort` stay among them.
  The conversion entry says that every string key is a name and that the strings of a value count against the memory limit, the `json` entry says the same of the keys of a decoded object, and the 64-byte entry names a path of `var`.
  The conversion entry also says that what another key of a list holds is left out, whether the key is a name, a boolean, a number with a fraction, or a whole number below 1.
  It says that what is left out counts toward the two limits on the size of a value all the same, with the reason.
  The entry on a weak table gives the most bytes of `__mode` that one walk or one call reads as twice the limit, and four times for a conversion to data that is refused, and says which functions report a value past it whatever else is wrong, and that the others give the bytes that the call came to and the most that one call scans.
  It names the `tasks` functions among the readers, and says that `ipairs` and the functions of `table` take a weak table, with the reason.
  It says which places read through a view, and so refuse a view that leads to a weak table, and which take the entries of the table itself.
  The attributes and the parameters of `tools.adhoc` are among the first, since `cc55a351` reads them through a view.
  The entry on `var` says that a write counts the levels from `var` itself and refuses a key that is not UTF-8, with the reason for each, and the entry on the limits names the 127 levels of `var`.
  The entry on the limits also gives the memory that the data of one conversion takes outside the VM's limit: about 670 bytes for a value, 5.2 GiB at the default.
  The entry on the guard of the globals says that the Engine's own Lua code calls no global that the author's code overwrote, with the reason, and that a `var` global that the author's code replaced or removed raises an error whatever the author's metatable answers for it.
  It says that an `__index` or a `__newindex` of the author's that Lua cannot index fails in Lua's own text on the line of the read or the write, with no position in a text that a `pcall` catches, and gives the reason.
  The entry on a failure that Lua catches says that a failure that ends a block reads U+FFFD for the bytes of its message and of its traceback that are not UTF-8, with the reason.
  The entry on a weak table says that a read of one field raises the error for a weak table that the `__index` of the table leads to, names those reads, and gives the 2000 values and the reason.
  It gives the most tables that one call of `tools.offer` and one read of a `prose` follow for those reads, with the reason, and counts the bytes of `__mode` of a `prose` for one read of it.
  The entry on the guard of the globals says that the Engine tells its own `var` from another table by identity, and the entry on a library that runs once says the same of a library that still loads, with the reason.
  The entry on the order of `pairs` says that a `__pairs` of a collection has no part in the order of a fanout, and the entry on a weak table says that `json.encode` with `debug` writes its placeholder for one; a review comment on pull request #78 asked for each.
- `decisions/models-and-conversations.md`: the check of a record at the builder call refuses a record nested more than 127 deep, one that holds more values than one conversion may, and one that holds more bytes of text, and a record of the loop is outside that check.
- `decisions/tasks-and-fanout.md`: the keyed members of a collection come in the order of `pairs`, which a `__pairs` of the collection has no part in, the determinism entry names the five stock functions that follow the length of a table with a gap, and the entry on `tasks.spawn` says that a weak handle, set, or `opts` table raises an error, and so does one whose `__index` leads to a weak table.
  The entry on a collection names `prompt.section(path, "list").items` as the read of a list section, where it named a `prompt.list` that the Engine does not have.
- `decisions/tools-and-packs.md`: the array of `tools.offer` is stated by the new rule, an entry that the array lacks raises an error when its read would look into a weak table or when the call follows more tables than its budget has, a weak array of a parameter's type and description raises an error, the bytes of a `__mode` count in the budget of the short form, and an error quotes at most 64 bytes of a key, a name, or a type.
  The entry on `tools.adhoc` says that the attributes and the parameters may each be a view, which `cc55a351` made so and left out of the page.
- `decisions/engine.md`: the planned work on JSON conversion is a read of the cancel flag inside a walk, since every conversion of an author's table now has a budget.
  The entry on the one lookup of a heading names a `prompt.section` or `prompt.section_or` read, where it named `prompt.list`.
- The guide in `runvil-docs` changes on its own branch `kb/lua-fixes`: chapters 3, 4, 5, 6, 7, 8, 10, 11, 12, 14, 15, 16, and 17 of the language part.

## Tests

In `runvil-lua` where no other crate is named.

- `tests/sandbox.rs`:
  - a metamethod that holds a `<close>` variable and fails leaves it open for the handler of an `xpcall`, as in stock Lua, under each of the four replacements, and a failure with no position in it is placed on the metamethod's line;
  - a call that waits works in a `__tostring`, a `__len`, a `__lt`, the `__index` of a joined list, and the `__index` and the `__newindex` of a sorted list;
  - `error('boom', level)` for the levels 1 to 4 gives the metamethod's line, no position, the line of the call, and no position, in a `__tostring` under `tostring`, `%s`, and `table.concat`, in a `__len` and an `__index` under `table.concat`, and in a `__len`, an `__index`, a `__newindex`, a `__lt`, and a comparison function under `table.sort`; a stock `Lua::new()` gives the same for each but the element of `table.concat`, which it does not take;
  - `table.concat` fails on the author's line in stock Lua's words, as the sort does;
  - the cases that give the same answer in a section VM and in a stock `Lua::new()` gain the parts of a format around a `__tostring`, a `__call` chain of 15 and of 16, a `__lt` and a `__len` that Lua cannot call, an `__index` chain that ends in a value, in no value, and in a loop, and the order in which `table.concat` checks its arguments;
  - a raised value whose `__name` reads `runtime error: Thing` fails the block without its address.
- `error-tests.rs`: the label that `mlua` wrote stays in front of a failure of Lua and of a callback's failure, and the same words in a name or in another failure's text are text like any other.
  In that test a failure that reads `?:?: stack overflow` is rendered as `runtime error: stack overflow`; without the rule on the position of an Engine chunk in the rendering, the test fails with the `?:?: ` kept, where no test failed before, as a review found.
- `collection.rs`: over four tables, with a gap, booleans, floats, numbers below 1, and no index 1, the keys of the members are the keys that the Engine's `pairs` visits, in its order.
  That test and the three tests of the order of the keyed members fail with the members sorted by `SortKey`.
- `data-tests.rs`: over 16 fresh VMs a list with a gap is refused for the same missing item, and of several faults the one under the first key is named; a value with one fault is refused in the words of `mlua`'s own conversion, which the test runs beside it; a value 127 deep reads back from its JSON text, and one 128 deep and one 100,000 deep are refused; a value past the budget is too large in 8 VMs, with two gaps in it; a table with a gap that holds, past the gap, a weak table larger than the budget is refused for the gap in every VM; with a memory limit of 4 MiB, a value with a function and 64 references to one string of 1 MiB, as values or as keys, is too large in 8 VMs, and with 3 references it is refused for the function; a table with 32 key tables, half of them with a list, and a table with a table, a function, and a thread as keys, are refused for the type of the key in every VM.
  The test of the text fails without the count, where the walk names the function.
  Of the several faults, one value has a list with a gap under a name of a table that has a list, which the conversion does not read, and a function after it: the function is named.
  With the second walk taken into the entries that the conversion leaves out, the test fails with `value.rows.bad is missing list item [2]`, where no test failed before, as a review found.
  A fault three tables down, with another fault beside it at each table, is named with its path from the value in every VM, and the test fails when the steps are written in the order the walk met them.
  The time of the walk that names a fault was measured by a probe and has no test, because a test of a time is not a yes or no.
- `data-tests.rs`, more: 126 tables around a view that only a function can read convert and 127 are refused; a table that holds itself through a view is refused; and of 256 values built from a fixed seed each, a chain of 100 to 130 tables with views, a last view that only a function can read, and up to two more links, every one that converts reads back from its JSON text.
  All three fail without the change, the last at seed 13.
  `tests/views.rs`: a table that contains itself is refused by `data_of` in the words of `mlua`'s own conversion.
- `messages-tests-view.rs`: records that each hold a view of the one before are refused at the 32nd, where the view would end 128 deep, and the deepest record reads back from its JSON text; a builder takes a record 127 deep, by its arguments or by its `meta`, whose view is appended again and whose JSON text reads back, and refuses one 128 deep, which it took before the change; 65 views of 4,096 bytes beside a function are too large, 10 are refused for the function, and 40 views of 1,009 values are too large; the size of each record shape is the size of its JSON.
  With a budget of 32,768 values, lists of a view of a record and lists of the table of the same data convert at 32,768 values and are too large at 32,769, in the walk before the conversion and in the walk that names a fault.
  The test fails when a view costs one value more than its record, as it did: both walks spent one value for the view and then the values of the record, which count the record's own object.
  With a memory limit of 512 KiB, a builder takes a record of 65,536 values, a view of which is appended again, and refuses one of 65,537 whose content and `meta` each convert.
  The test fails without the check, where the builder takes the record.
- `protocol/tests/parse_tasks.rs`: a spawn `item` with a gap in a list is refused with the whole message.
- `tools/tests-offering.rs`: the table whose `__index` fills a gap, and the same with a second gap, are refused for the key 5 in every VM, and a weak array is refused.
  The test fails on the code before the change.
- `data-tests.rs`: NaN and the infinities become null in a list and in an object, which the pages now state; the conversion did the same before the change.
- `tests/json/errors.rs`, `tools/tests-adhoc.rs`, `models-tests.rs`: a weak table is refused by `json.encode`, with its placeholder in debug mode, by `tools.adhoc` for the attributes and for the parameters, and by `models.use`.
  `tools/tests-adhoc.rs`: `tools.adhoc` refuses attributes that are an empty weak table in the shape of a view, and parameters that are a view under a weak metatable, which passes with no parameter when the parameters are read with the test of a weak table alone.
- `proxy.rs`: the read through views ends at a weak table that is the table given, with an entry and without, a view on the way, or the data.
  `data-tests.rs`: a weak table with an entry, `__index`, and `__metatable` is refused in the same words before and after two collections that take the entry.
  `iteration-tests.rs`, `collection.rs`, `tests/json/errors.rs`, `tests/markdown/views.rs`, `tools/tests-adhoc.rs`: an empty weak table in the shape of a view is refused by `pairs`, `next`, `tasks.fanout`, `json.encode`, `json.encode_lines`, the `markdown` functions, and as a parameter of `tools.adhoc`, and a weak array of a type and a description is refused with a table and with a string as its description.
  All seven tests fail when the read looks for an entry before it tests for a weak table.
- `proxy.rs`, more: the read ends at a table under a weak metatable, empty or not, and at a view of one.
  `data-tests.rs`: an empty table under a weak metatable with `__index` and `__metatable` is refused in the same words before and after two collections that take the `__index`.
  `tests/json/errors.rs`: `json.encode` refuses such a table, writes its placeholder in debug mode, and refuses weak options, as `json.encode_lines` does.
  `tools/tests-offering.rs`: an array that its `__len` makes weak, one that its `__index` makes weak, and one under a weak metatable with a `__len` are refused.
  Without the test of the metatable all four fail, the conversion with the data of the `__index` table.
  Without the test after `__len` the offer is refused for the key `p`, an entry that the collector can take, and without the test after `__index` the offer is recorded.
  Without the test of the options the options are read.
- `tools/tests-offering.rs`, more: under a limit of 4 MiB, with a `__mode` of 1 MiB that makes no table weak, both offer verbs refuse an array of three entries behind a `__len` and an `__index`, and take one of two entries and a table that holds 100 entries itself.
  With the test after every read the table of 100 entries is refused.
- `proxy.rs`, `tests/json/limits.rs`, `tests/markdown/views.rs`, `data-tests.rs`, `tools/tests.rs`, `iteration-tests.rs`, `tools/tests-offering.rs`: the read reports the bytes of each `__mode` up to the first `k`, `v`, or zero byte, and a `__mode` shared past the budget is too large for `json.encode`, `markdown.render`, the conversion to data in every VM with a fault beside it and without, and the short form of `tools.adhoc`, and is refused by `pairs`, `next`, and both offer verbs, while one within the budget is read.
  All seven fail when the read reports no bytes.
  `coro.rs`, `collection.rs`: `tasks.join` refuses 65 views that share a `__mode` of 1 MiB, at the default limit, and `tasks.fanout` refuses five such views under a limit of 4 MiB.
  With each scan counted as one byte at most in `view_data`, three tests fail, those two among them.
  `iteration-tests.rs`, `collection.rs`, `coro.rs`, `tools/tests-offering.rs`: each of those refusals is compared whole, with the bytes scanned and the limit: 5242880 against 4194304 for five scans of 1 MiB under a limit of 4 MiB, and 68157440 against 67108864 for the 65 views.
- `tests/sandbox.rs`, more: `table.sort` names a comparison function with a `__name` as stock Lua does, takes the message list as a list and fails at the comparison of two records and at the write, and refuses a record, which has no `__len`.
  Three cases of the comparison with stock Lua write a sorted list through a `__newindex` that is a chain of tables, a number, and a loop.
  A `__close` handler, which runs before the `pcall` around it returns, reads `boom` for `error('boom', 2)` in a `__tostring` under `tostring`, and `?:?: attempt to index a number value` for a list whose `__index` is a number under `table.concat`, where the `pcall` gives the second text without its start.
  Under an `xpcall` whose message handler returns its argument, the handler reads `boom` and `attempt to index a number value`, the texts that the `xpcall` gives.
  With debug info in the chunk of the replacements that test fails, where the handler reads `crates/runvil-internal/lua/src/__impl_stdlib.lua:196: boom`, and the comparison with stock Lua fails with `attempt to call a number value (local 'rendering')` for a `__tostring` that is a number.
  The test of the levels passes with debug info, since the rule on the position of an Engine chunk takes that position off the text that a `pcall` hands on.
  With the two asks answered with no refusal, the comparison fails with `attempt to call a boolean value (metamethod 'lt')`, and the test of the caller's line with `(metamethod 'len')` in the text.
  With the asks before a read, a write, a call of a `__tostring`, and a comparison of two values with no `__lt` taken out, as they now are, every test of the crate passes.
- `tests/json/errors.rs`, `tools/tests-adhoc.rs`: a key of 165 bytes with a two-byte character at byte 64, one with a four-byte character at byte 62, a name and a key of 1000 bytes, and a type of 1,048,576 bytes, are quoted by their start and their length.
  With a step back over one byte or two at most, the key with the four-byte character fails with U+FFFD after its 61 letters, where no test failed before, as a review found.
- `error_value-tests-raised.rs`: a raised error table with entries that are no fields gives the same error in every VM.
  The test fails on the code before the change with `mlua`'s text for a boolean key.
- `tests/views.rs`: the case of a list with a gap left the test of a value with no view inside, since `plain` now refuses it.
- `data-tests.rs`, `tests/sys_and_var.rs`, `tests/json/decode.rs`, `protocol/tests/answer.rs`: the key `$serde_json::private::RawValue` is a name in a table that becomes data, alone and beside a second key, in 16 VMs, in `var`, in `json.decode` and `json.decode_lines`, where a text 126 deep around the key decodes 126 deep, and in a tool's structured output.
  All four fail with `serde_json::Value` as the reader, with the data of the text in place of the object.
- `data-tests.rs`, more: under a limit of 4 MiB, three strings of 1 MiB convert and five are too large, in a value with a fault and in one without.
  Without the count in the first walk the five convert.
- `error_value-tests-raised.rs`: the read of a table asks for the fields of its code and for no other, and a string that `pcall` caught loses the position of an Engine chunk in front of it.
  `chunk_line.rs`: the position of an Engine chunk goes, and every other start of a text stays, a name with a line break in it among them.
  With the line break taken out of the rule, that text loses its start and the test fails.
  `tests.rs`: every embedded chunk has a name under which the rule takes a position off.
- `tests/sandbox.rs`, more: over eight sizes of frame, a stack overflow has no position of an Engine chunk in the prose of the failure or in the text that a `pcall` caught.
  Without the rule the test fails with `?:?: stack overflow` in the prose.
- `tests/sys_and_var.rs`, more: a path of `var` shows 64 bytes of a key of 100, whether the key was the key of a write or a key inside the value written.
- `tests/cancellation.rs`: an abort that a hook raises once, inside the stock call of `table.concat`, ends the call.
  Without the test of `raised_here` the join starts again and returns.
- `tests/sandbox.rs`, more: a `__len` that takes the length again, with `string.format('%d', 1)` or `table.concat({ 'a', 'b' })` at each level, fails with the string `C stack overflow`, and a `pcall` around the nest catches that text with no position.
  The call of the stock function is the C call that meets Lua's limit there, so the failure is none of the stock function's own.
  With that failure raised at level 4 by the stock calls, or at level 2 by the join, the test fails with the author's position in front, where no test failed before, as a review found.
- `tests/sys_and_var.rs`, more: 126 tables under `var`, written whole or one table at a time, are stored, and the JSON text of the `var` that the VM reads back is parsed again, 127 deep; 127 tables are refused either way, for the table at level 128.
  With the level of the value counted from 1 the test fails, where the write for each level is taken.
  A probe on that code wrote a value 127 deep whole and one table at a time: both were stored, and the text of `var` failed to read back with `recursion limit exceeded`.
  A write under a key that is not UTF-8, to `var` and to a table in it, is refused; a probe before the change had both taken and the read-back of `var` failed.
- `tests/sandbox.rs`, more: over 16 lengths of a filler string, a block that fills a heap of 4 MiB in a loop of `tostring(hold)`, for a table `hold`, or of `pcall(table.sort, { label, 1 })`, catches `not enough memory` and goes on.
  Each call asks a Rust capture about a value: `tostring` about the `__tostring` and the `__name` of the table, and the sort about the `__lt` of a string, which reads the metatable of strings.
  With the names of the events and that metatable taken anew inside each capture, so that `Lua::type_metatable` runs inside a callback, the test process aborts with `Assertion failed` in `lapi.c` at 14 of the 16 lengths of the `tostring` and at 12 of the 16 of the sort.
  A loop of `tostring(label)`, which the test had first, asks no capture about a string since the raw read in the chunk, and a loop of `string.format('%s', label)`, which it had next, asks none either; a review found each.
- `tests/sandbox.rs`, `tests/diagnostics.rs`: `refused .. value` with a `__tostring` that calls `error('boom', 3)` reads `boom`, and over ten sizes of frame a stack overflow under the `__concat` of an error value has no position of an Engine chunk in the prose.
  A chunk named with 59 bytes loses its position by the rule where one named with 60 keeps a cut name.
  With the `__concat` chunk loaded with no name the first test fails with `crates/runvil-internal/lua/src/error_value.rs:614:1: boom`, where 614 is the Rust line of the load.
  `tests.rs`, `coro.rs`, `error_value-tests.rs`: the check of a chunk name asks the rule itself about a position under the name, so a name in a directory below fails it, and holds the name to 59 bytes after its `@`, and the names of the yield chunk and of the `__concat` chunk have a test each; the eight names of the files have seven, since the two chunks of the replacements share one.
- `tests/sandbox.rs`, more: a block that replaces `tostring` and then joins a caught refusal with `'!'` gets the message of the refusal and the `!`.
  With the error metatable built at the VM's first failure the test fails with `replacedreplaced`.
- `tests/sandbox.rs`, more: the caught value of a stack overflow is the string `stack overflow`, with the line of the author's call in front or nothing, where the test took any text without an Engine position.
- `tests/sandbox.rs`, more: three cases of the comparison with stock Lua nest a sort through its `__len` and through its `__lt`, and a join through its `__len`, and each fails with the string `C stack overflow` in both.
  With the metamethod of a table read by Rust text the comparison fails: the section VM gives `failed table prompt:2: C stack overflow. The Lua code failed at runtime. Fix the code at this location.`.
- `tests/sandbox.rs`, more: three more cases have `string.format` as the `__index` of a joined list, as the `__lt` of two sorted values, and as the `__index` of a table that is the `__index` of the globals, and the test of the calls that read otherwise has the guard's tail call of `string.format`, `'format'` where stock Lua reads `'index'`.
  With no rule on a caller that is a replacement the first case reads `'index'`, where stock Lua reads `'string.format'`, and with the guard taken for a replacement the third reads `'string.format'`, where stock Lua reads `'index'`.
- `coro.rs`: `tasks.join` and `tasks.join_any` refuse a weak set and a set under a weak metatable, `tasks.join` and `tasks.cancel` a weak Task handle, and `tasks.spawn` weak options, each in its own name; a set that the `__index` of its first member makes weak, and options that their own `__index` makes weak, are refused before the next read.
  With no check of a set the first case fails, where the empty set is refused as empty, and without the check before each read the cases of a table that a read makes weak do.
  Without the refusal in the check of the keys, the options of `tasks.spawn` fail in the name of `pairs`, where no test failed before, as a review found.
  Eight calls of `tasks` functions that fail before their first yield call none of the globals `error`, `ipairs`, `pairs`, `select`, `tostring`, and `type` that the block replaced; without the locals of the `tasks` chunk they call `type`, `ipairs`, `tostring`, and `select`, and without those of `__impl_coro.lua` they call `type`.
- `runvil-runtime`, `execute/tests/scheduler/fanout/arms.rs`: a fanout of two arms, with a write to a result after it, calls none of the globals `error`, `ipairs`, `select`, `tostring`, and `type` that the block replaced; without the locals of `__impl_fanout.lua` it calls `ipairs` twice and `error` once.
- `tests/sys_and_var.rs`, more: five writes to `var` that are taken write no path, and a refused one writes one, by a count of the paths written that a test build keeps.
  The test fails when a write makes the path of its target before the conversion, with 4 paths for the writes that are taken.
  A `var` global set to nil fails the read-back as reassigned under an `__index` of the globals that returns the table; with the read through the metatable the read-back passes.
- `tools/tests-adhoc.rs`, more: of the unknown keys `zz`, `1`, and `true`, the refusal names `"boolean"`, where the order of their texts names `"1"`, and the two refusals of a weak table are compared whole.
- `data-tests.rs`, more: under a limit of 4 MiB, five tables that each hold the same key of 1 MiB, with no fault, are too large; without the count of a key in the first walk they convert.
- `prelude-tests.rs`, `prelude-tests-environment.rs`: a prelude that raises `?:?: boom` while it loads fails with `boom` in its message, and with `?:?: boom` without the rule there; a refusal of the read-only view of `var` shows 64 bytes of a key of 100 in its path, and the whole key without the cut.
- `protocol/tests/parse_tool_call.rs`, `prose.rs`, `collection.rs`: a list with a gap in the arguments of a tool call, in a global in prose, and in a member of a collection is refused for the missing item.
  With the first walk and the conversion alone in place of `data_of`, each is refused as `the value has no JSON form`.
- `protocol/tests/parse.rs`: the `var` of a request holds the key `$serde_json::private::RawValue` as a name, and with `serde_json::Value` as the reader it holds the data of the text.
- `runvil-runtime`, `execute/tests/suite/require/failures.rs`: a cycle lists its libraries under a Pack prelude that sets `table.concat`; with a call of `table.concat` in the chunk the text is `require cycle: replaced`.
- `tests/sandbox.rs`, more: eight calls of `pcall` and `xpcall` join the cases that give the same answer in a section VM and in a stock `Lua::new()`: no argument to each, `pcall` under `pcall`, and a handler that is missing, nil, a number, a table with a `__call`, and a table whose `__metatable` covers its `__name`.
  With the wrappers of `22a0992f` the test fails at the first, where `pcall()` raises nothing.
  A `pcall` around `pcall()` and around `xpcall(type, 5)` catches the refusal with the line of the call in front, in a section VM and in a stock `Lua::new()`.
  With the stock function called through a local of its stock name, that test fails with no position in the text, and the comparison fails at a call of `xpcall` through a local `guarded`, which then reads `'xpcall'`.
- `tests/sandbox.rs`, more: sixteen calls that fail with an argument error join the comparison with stock Lua.
  They are a call through a local and through a field of a table of the author's; `table.sort`, `table.concat`, `string.format`, `math.randomseed`, `tostring`, and `xpcall` under `pcall(f, ...)`; three method calls, one of them on a bad self; a replacement as the iterator of a generic `for`; a handler of `xpcall` that is a light userdata; and `setmetatable` and `getmetatable` through a local.
  With the name of the call left unread, so that each function has the one name of its field, the comparison fails, and so it does with the value of a method call counted and without the arm that names a light userdata.
  A test of its own holds the calls that differ from stock Lua, each with stock Lua's answer beside it: `local order = table.sort return order(5)`, a read of an unset global after `setmetatable(_G, { __index = string.format })`, `xpcall(function() error({}) end, string.format)`, and `callee(5)` for a table whose `__call` is `table.sort`.
  With the name in the library given under a tail call, that test fails.
  Two cases give the globals an `__index` that is a number and a `__newindex` that is a boolean; with debug info in the chunk of the guard both fail with `(local 'handler')` in the text.
  One case gives the metatable of strings a metatable whose `__index` answers for `__tostring`; with a plain read of the field in the chunk it fails, where `tostring('abc')` reads `indexed`.
- `tests/budgets.rs`: a VM is built under every sixteenth limit from 8,192 bytes below the heap after the Engine values are injected to the heap of the built VM, with 8 tables inside `args` and with 36, and each build passes or fails with `memory error: not enough memory`.
  A build that panics or ends the process does not return, so the test fails there.
  It takes 6.2 s in a debug build.
  Without the growth of the stack in `SectionVm::new` it fails at 8 tables with the panic of `mlua` at 34 slots, with 37 tables in place of 36 it fails with that panic at 68 slots, and with the `mistyped` of the first form, which makes a second `Events`, the test process ends with `SIGABRT`.
  As many limits refuse one allocation as it has bytes, and the smallest that the test is for is the empty string of `Lua::type_metatable`, 25 bytes, so a step of 16 bytes meets it; a pass by one byte is under "What the build found".
- `tests/sandbox.rs`, more: three more cases of the comparison with stock Lua sort a list whose `__len` is behind 15 `__call` metamethods, which is called, compare two values whose `__lt` is behind 16, which is refused as `'__call' chain too long`, and sort a list whose `__len` gives 2,147,483,647, which is refused as `array too big`.
  With `CALL_CHAIN` at 14 the first fails, with 16 the second, and without the check of the length the third fails with `not enough memory`.
- `data-tests.rs`, more: of the several faults, `{ [true] = function() end }` is refused for its key.
  With the fault of the value taken first the test fails with ``unsupported value type `function` ``.
- `error_value-tests-raised.rs`, more: a field of the table's code that is not UTF-8 gives no fact, and the error keeps the table's `message`.
  With the read of the field failing on such text, the test fails with `mlua`'s text for it.
  The same test reads a `message` that is not UTF-8 with U+FFFD in it, and a table with a `kind` that is not UTF-8 as no error table; with either read strict again it fails.
- `tests/sandbox.rs`, more: a block that calls `tasks.cancel(5)` in a function held under the key `'\xff'` fails with the refusal of `tasks.cancel` on line 2, and `tasks.spawn('S', { ['\xff'] = 1 })` with the refusal of the key on line 1.
  With the traceback read as UTF-8 the first fails, and with the message read so the second, each with a prose that has no text of the failure.
  With a traceback that is not UTF-8 left out, the first fails with no line, where no test failed before, as a review found.
- `tools/tests-adhoc.rs`, more: a name of 60 letters and 40 bytes that continue a character is quoted as the letters, four U+FFFD, and `(100 bytes)`.
  With the walk back to the start of the text it fails with 59 letters, and with the name quoted from its lossy copy with `(180 bytes)`.
- `data-tests.rs`, more: a list that holds itself twice under a key that is no item converts to its items.
  Without the test of a table on its own path in the first walk, that walk goes on to the depth limit, and the test fails after 8 s with `the value is too large to convert`, where no test failed before, as a review found.
- More cases, each for a change of one line that a review made and that no test failed for.
  `data-tests.rs`: with a memory limit of 4 MiB, a list of one item that holds five references to one string of 1 MiB under a name is too large in 8 VMs, with a string as its item and with a function.
  With the text that the conversion does not read left uncounted in the first walk the value converts, and in the second walk the function is named.
  A table that `json.array` marked and that holds a list with a gap under a name converts to an empty list, and a list of a function that holds a weak table under `true` is refused for the function.
  With the mark left out of the choice of what a table reads the first fails with the gap, and with the weak table that is not read taken as a fault the second fails with `value[boolean] is a weak table`.
  `messages-tests-view.rs`: a list that holds a view of a record 127 deep under a name converts to its items, and beside a function under index 1 such a view under `true` leaves the function named.
  With the depth of what is not read counted, the first walk refuses the first and the second walk names `value[boolean][1]` as nested too deep in the second.
  The size of the record of `system("be terse")` is one level, 3 values, and 25 bytes; without the bytes of the keys it is 14.
  `iteration-tests.rs`: four views that share a `__mode` of 1 MiB and one byte are refused with 4194308 bytes scanned, the three scans made and the fourth; with the limit and the last scan added it reads 5242881.
  `coro.rs`: `tasks.join` refuses an empty weak set as a weak table; without the check before the first read it fails with `tasks.join requires at least one task`.
  The options of `tasks.spawn` and of `tasks.join` that their `__pairs` makes weak, with an `__index` that takes the weakness off again, are refused in the name of the call; with the first option read unchecked each goes on to its yield.
  `tests/sandbox.rs`: a write to an undefined global that a handler table of `_G` hands to `string.format` names the function `newindex` as stock Lua does, and with the guard known by its `__index` alone it names `string.format`.
  A sort of a table whose `__len` is a table with a `__name` fails with `attempt to call a Named value` as in stock Lua, and with the `__name` of the metatable left unread in the Engine's refusal with `a table value`.
  A list that its comparison function empties and gives a `__newindex` that calls `error('boom', 3)` fails with the line of the author's call in front, here and in stock Lua; with the list looked at once, before the sort, the text is `boom`.
  `tests/json/errors.rs`: a key of 64 letters is quoted whole.
  `tests/sys_and_var.rs`: a write to `var` of a weak table, of a table that holds a view that leads to one, and of a table under a weak metatable is refused with its path and stores nothing; with a weak table read as its own data in a write to `var`, the first is stored.
  `data-tests.rs`, more: with a memory limit of 64 KiB, a list of one item that holds 1,171 references to one table of six numbers under a name is too large in 8 VMs, with a string as its item and with a function.
  With the values that the conversion does not read left uncounted in the first walk the value converts, and in the second walk the function is named.
  A list of a function that holds a thread, or text that is not UTF-8, under `true` is refused for the function; with a value that is not read taken as a fault it is refused for the thread, and with text that is not read tested for UTF-8, in the words of `mlua` for a byte array.
  `coro.rs`, more: options of `tasks.spawn` that the read of `args`, or of `item`, makes weak, with an `__index` that takes the weakness off again, are refused before the next read; with the read of `item` or of `index` unchecked the call goes on to its yield.
- More cases, each for a fault that the review of pull request #78 found.
  `coro.rs`: options, a Task handle, and a set of tasks that are not weak and take `item`, `task`, and a member from a weak table are refused in the name of the call, with `a table whose __index leads to` in front of the words of a weak table.
  Options that take `timeout` from a table under a weak metatable, two tables on, are refused in the words of that table, and options whose `__index` is a string are refused when the `__index` of the metatable of strings is a weak table and when that metatable is one.
  A handle that takes `task` from a table that holds it is read, whatever lies past that table.
  Options behind 2000 tables that lack the field are refused, and behind 2001 the call raises Lua's `'__index' chain too long; possible loop`; behind three views the two counts are 1997 and 1998, and a handle that is its own `__index` raises Lua's error.
  Options behind 65 tables that share a `__mode` of 1 MiB, which makes none of them weak, are refused with 68157440 bytes of `__mode` to scan.
  Without the walk the test fails at its first case, where the call goes on to its yield.
  `tools/tests-offering.rs`: an array with no entry of its own, a `__len` that gives 1, and a weak table as its `__index` is refused in the name of the verb; without the walk the offer is recorded.
  Under a memory limit of 4 MiB, an array of 512 entries that the last of 1,024 tables behind it holds is taken, and one of 513 is refused with the 524,288 tables that one call follows; without the count both are taken.
  `globals-tests.rs`: the lookup of `prose` refuses a global that `_G` lacks when the `__index` of the author's metatable is a weak table and when that metatable is one, and reads a global that `_G` holds, `args`, and a global that a table or a function of the author's gives.
  Without the walk the read under the weak `__index` gives `1`.
  It refuses a global whose weak table is the last of 2,001 tables, the last that Lua asks; with the walk one value shorter the read gives `1`.
  Under a memory limit of 4 MiB, one `prose` reads a global through 1,024 tables 512 times, and the next read is refused; with a budget for each read, and without the count, all 513 are answered.
  `models_loop-tests.rs`: a loop with no compactor is refused when `compactors` takes `fail` from a weak table and when `compactors` is one, a loop is refused when `sys` takes `taskid` from a weak table, and a loop with a compactor of the author's, or with tables that are not weak, starts.
  Without the walk the first five start, and the sixth is refused by the check of the table given.
  `messages-tests-view.rs`: with a memory limit of 1 MiB, a builder takes a record of 1,048,576 bytes of text, whose content and whose argument are one string of 524,262 bytes in the heap, and `append` takes a view of it; the builder refuses the record of 1,048,577 bytes, and the list keeps its two records.
  Without the limit the builder takes that record.
  `tests/sys_and_var.rs`: a `var` global that holds a table whose `__eq` returns `true` fails the read-back as a reassigned `var`; with `Table::equals` the read-back passes.
  `tests/require.rs`: a library that returns a table whose `__eq` returns `true` loads once, the second `require` returns the saved table, and the `__eq` does not run; with a table as the mark the second call raises `require cycle: /a`.

Each new refusal was taken out of the code in turn, and the tests of `runvil-lua` run without it.
Without the weak-table refusal of `tools.adhoc` and `models.use`, of `json.encode`, of `tools.offer`, and of the conversion, the tests of each fail.
Without the cut at 64 bytes four tests of a long text fail, without the gap check seven tests fail, and without the second walk sixteen.
The test of a weak table past a gap failed in its first VM with `the value is too large to convert` before the walks left a weak table alone.

Probes that are not tests, each run once: the three measures of the conversion over 24 fresh VMs before the change, the stack overflow of a value 1000 deep before the change, and the times above in a release build.

More probes, each run once on the three commits made after the review, and none kept as a test.
A handle behind 1500 views and then 400, 500, and 501 tables that end in a weak table is refused, refused, and raises Lua's `'__index' chain too long; possible loop`.
A handle behind 1500 and 1999 views that end in a view whose `__index` is a function is read through that function, and behind 2000 it raises Lua's error.
A string as `__index` gives a field that the `string` table holds, a userdata gives nil, and a number and `json.null` raise Lua's `attempt to index`.
A set whose first member it holds, and whose end is read from a weak table, is refused, and so is a `sys` that is a weak table itself, in the words of a weak table.
A `var` global that holds a table with an `__eq` is refused by `tasks.spawn`, and the `__eq` does not run.
The record of the review, 140,000 bytes of content and as many of an argument under a memory limit of 262,144 bytes, is refused with 280,052 bytes of text.
Of the records that a builder, or `append` of a table, took within 130 bytes of a limit of 1 MiB, 485 in five shapes, `append` took a view of each, and the first record that a builder refused held 1 to 3 bytes more than the limit.
By the listing of `luac` 5.5.1, no function of an Engine chunk reads or writes a global: every such read is in the body of a chunk.

One more probe, run on the last tree and not kept as a test, takes its answer from the collector and not from the check.
It makes ways of 0 to 2,003 tables from a fixed seed, with weak tables and tables under a weak metatable among them, and at their end nothing, a function, a number, a string, a weak table, a table under a weak metatable, or a table of the way again.
The function changes the way when a read calls it, in one of eleven manners, and each weak table and each weak metatable holds values that nothing else uses, so a collection takes them out.
It makes 6 to 65 reads of four keys on each way, twice: once with the collector stopped, and once with a full collection before each check and before each read.
Over 20,000 ways behind a table and 20,000 behind the globals, the two gave the same refusals and the same values: 1,419,224 reads, 500,893 of them refused and 154,108 answered by a function.
With the check of a weak table, of a weak metatable, or of the metatable of strings taken out, with the check made after the look into a table, and with no table asked past the one that the read is written for, a read gave one value with the collector stopped and another after a collection.

## Verification

On the last tree, the change on `cc55a351`:

- `cargo nextest run --locked --workspace --all-features`: 4042 run, 4042 passed, 25 skipped.
  The run had a build directory of its own: the one the machine shares between worktrees gave a test build of `runvil-runtime` a `runvil-types` of another branch.
- `cargo clippy --workspace --all-targets --all-features` with `CARGO_BUILD_WARNINGS=deny`: clean.
- `cargo fmt --all --check`: clean.
- `cargo doc --workspace --no-deps --all-features` and `cargo doc -p runvil-engine --no-deps`, both with `RUSTDOCFLAGS="-D warnings"`: clean.
- `cargo +nightly-2026-09-05 xtask api --check`: 0 violations for each facade, and each listing matches its `public-api.txt`.
- `cargo test -p build-xtask`: 204 passed.
- `node --test tools/docs-claims.mjs`: 3 passed.

`cc55a351` with the two plans alone runs 3965 tests, and all pass, so the change adds 77.
`92abc305` with the two plans ran 3957, and the change added 76 there: the five commits of `master` between the two add eight tests, and the move adds one, of a view of `tools.adhoc` that the collector can change.
`5d9c7bf8` with the two plans ran 4060: the twelve commits of `master` between the two change the tests of `harness-gateway-client`, `runvil-engine`, and `harness-runner`.

Each commit of the code was checked alone on `cc55a351`, in its order: the format check and clippy were clean, and every test of the workspace passed, 3966 at the first commit and 4042 at the comparisons by identity, which has the code of the last tree.
Those runs were made under `cargo test`, with the tests of `harness-plugin-loader` under `cargo nextest`.
`cargo test` runs the tests of one binary in one process, and with those of `harness-plugin-loader` it runs the last tree with the 4042 passes and the 25 ignored tests that `cargo nextest` reports.
A test of `harness-plugin-loader` counts the loads of a Plugin in statics of its process, so those tests need a process each.
A run of the workspace under `cargo nextest` took 169 s alone and 18 to 33 minutes beside another run of `cargo nextest` on the machine, which is why the commits ran otherwise; the run of the last tree took 22 minutes.

On `92abc305`, before the move, each commit of the code was checked alone in the same way: the three made after the review under `cargo nextest`, with 4031, 4032, and 4033 tests, and the fifteen before them under `cargo test`, with 3958 tests at the first commit and 4026 at the last.
On `5d9c7bf8`, before the review, each of the fifteen was checked alone by the format check, clippy, and `cargo nextest`: 4061 tests at the first commit and 4129 at the last.
One run failed there, at the commit on the unknown keys of `tools.adhoc`: `transport::tests::limits::a_single_event_trickled_past_the_timeout_completes` of `harness-gateway-client`, a test of a timeout of 100 ms, while other builds held the load average of the machine above 90.
The test passed alone on that tree under the same load, and the run passed whole when it was repeated.
The runs from that commit on let a failed test run twice more, and no test did.

Each test of a fault that the review found was run with the fix taken out, as "Tests" tells for each, and failed.
With the three fixes taken out together on the last tree, the nine tests of them fail and the other 898 of `runvil-lua` pass: the seven that the three commits add and the two that they change.
For that run the code of the read by key and of the comparisons was put back as the commit before the three has it, and the check of the bytes of a record was taken out of `parse_record`, whose other tests give it the new argument.
The two tests of the count fail with the count taken out, with a byte of text counted in its place, and with its refusal dropped, and the test of a `prose` fails with a budget for each read.

The measures of this plan were taken on the trees that "Basis" names, and were not repeated on `5d9c7bf8`, on `92abc305`, or on `cc55a351`.
The times of the count of the tables that a read by key follows were taken on the change on `92abc305`, and of the files of `runvil-lua` the move changes `tools.rs` and two files of tests alone.
The instructions of the loop over a set of tasks were counted on the code of the read by key.

Not run on this machine: `cargo dylint`, a build on Rust 1.95, and any test on Windows.
The CI of pull request #78 ran the three for `4fdbe940` and for `b306088e`, with its other jobs, and every job passed.
`b306088e` is the change on `92abc305` without the count of the tables that a read by key follows: the last tree differs from it in that count and its two tests, in one more case of the test of a placeholder's global, in the move to `cc55a351` with the test that the move adds, in this plan, and in the decision pages.

## What the build found

- **A C function that Lua tail-calls keeps the caller's frame.** `return base_tostring(v)` therefore put the chunk's position on the failure, which is why a stock function that can fail is called from a chunk function under `pcall`, over values whose metamethods it does not use.
- **Stock Lua's sort puts no position on a failure of the comparison.** Neither does the replacement, and the Engine places it on the line of the call by the traceback.
- **A string has a length with no `__len`.** Lua takes the length of a string without a look at the string metatable, which the stock `table.concat` still requires to hold a `__len` for a string as its list.
- **Lua 5.5 words the end of an `__index` chain `possible loop`.** The comparison with the stock Lua found the text, which was first written from memory.
- **A replacement that the author tail-calls loses the author's frame,** as the first plan found.
  `error(text, 3)` in a metamethod under `return tostring(value)` names the caller's line too, where stock Lua names the line of the `return`.
  The Engine's `pcall` and `xpcall` are Lua functions too: `return xpcall(f, 5)` in a function puts the refusal on the line that called the function, and on no line when a `pcall` called it, where stock Lua names the line of the `return`; a probe ran the three cases beside stock Lua.
  An argument error of any replacement that is tail-called has that position, and the name that "Known" gives.
- **What `mlua` gives of a frame of Lua's stack.** `Lua::inspect_stack` hands a closure the frame at a level, and `Debug::names` and `Debug::is_tail_call` give the name of the call, the kind of that name, and whether the frame was tail-called (`debug.rs` of `mlua` 0.12.0).
  The name is text that `mlua` makes from Lua's bytes, with U+FFFD for each part that is not UTF-8.
  `mlua` gives no count of the values that a `__call` put in front of the arguments, which is `extraargs` of `lua_Debug` in `mlua-sys`.
- **A Lua function with no debug info is a frame with no position.** `error(text, level)` reads the line of the frame at `level` and puts nothing in front of the text when there is none (`luaL_where` in `lauxlib.c`, `luaG_getfuncline` in `ldebug.c`).
  A probe gave the same four texts for the levels 1 to 4 under such a function and under the stock `tostring`.
- **`mlua` pushes a value of a type with no protected call to give that type's metatable.** `Lua::type_metatable` in `state.rs` of `mlua` 0.12.0 pushes an empty string for the metatable of strings, and a memory error of that push inside a callback ends the process.
- **`mlua` keeps the Lua values of Rust on a stack that grows under the memory limit and does not shrink.** Each handle of a table, a function, a string, or a userdata takes a slot of a Lua thread of `mlua`'s own, and a copy of a handle shares the slot (`stack_value` in `state/raw.rs` and `types/value_ref.rs` of `mlua` 0.12.0).
  The stack starts at 17 slots and doubles when a handle finds none free, and `mlua` panics with `cannot create a Lua reference, out of auxiliary stack space` when Lua cannot grow it, which under a memory limit is when the heap has no room for the growth (`state/extra.rs`).
  A slot whose handle is dropped is free for the next handle, and the stack keeps its size (`drop_ref` in `state/raw.rs`).
  A probe took handles of the globals on a built VM until the panic: the stack of `22a0992f` has 34 slots with 26 free, and each table inside `args` and `var` takes one for the life of the VM, on both trees.
  A table inside `sys` takes none on either tree, and a table inside `item`, a bound model, and an offered tool take none on this change.
  A second probe held handles through the build until the stack grew: the build of `22a0992f` holds 24 values at once, where the coroutine shims install, and the build of this change 32.
  A table that a Pack prelude makes a global is an empty proxy over the prelude's table, and takes a slot on both trees, from the install of the preludes on, where the build holds 16 values at once on `22a0992f` and 22 on this change.
  With the growth in `SectionVm::new` the stack has 68 slots with 54 free, and it grows to 136 slots from 37 tables inside `args` or `var`, and to 272 from 105; on `22a0992f` it grows to 68 from 11 tables, to 136 from 45, and to 272 from 113.
  With the tables of one prelude counted in, it grows to 136 slots from 47 tables, and on `22a0992f` to 68 from 19.
  A review found three more holders, and a probe that counts the free slots of a built VM after two collections gave these counts, the second of each pair on `22a0992f`.
  The environment of a prelude holds the `var` table, through the read-only view of `var` that each prelude gets, until the collector frees the environment, and a function of the prelude that reads a global keeps it for the life of the VM: 54 and 26 slots are free with `function f() end` for a prelude, 53 and 25 with `function f() return string end`, and 52 and 24 with two such Packs.
  So a second Pack takes one from the count of tables: with 36 tables inside `args` the stack stays at 68 slots with 9 table globals in the first of two Packs and grows with 10, and on `22a0992f` with no table inside `args` it stays at 34 slots with 17 and grows with 18.
  `mlua` keeps the object that carries a failure out of a Rust function in a slot, from the first such function that returns, and one for each level of such calls inside one another (`state/util.rs`): 53 and 25 slots are free after one `pairs` that runs a `__pairs`, 52 and 24 after two levels of it, and 49 and 21 after five.
  The names of `pairs`, `next`, and the check of the `tasks` functions take three slots each on this change, from the first table with a metatable that each reads: 50 slots are free after a prelude that calls `pairs` on a view, where `22a0992f` has 25, and 44 after all three, by a review's run.
  So this change never grows the stack more often under the run's limit than `22a0992f`, and grows it as often, by twice the slots, for 37 to 44 tables, for 105 to 112, and so on at each doubling.
- **A pass over the limits by one byte.** A probe built a VM under every limit from 8,192 bytes below the heap after the Engine values are injected to the heap of the built VM, 61,023 limits with no table inside `args`, 71,762 with 36, and 72,786 with 37.
  On `22a0992f`, no limit panics with 10 tables, 641 do with 11, 958 with 12, and 967 with 16.
  With the one `mistyped` function and no growth in `SectionVm::new`, none panics with no table, 310 do with 3 tables, 964 with 4, 954 with 8, and 320 with 16.
  On the final tree every limit gives a built VM or `memory error: not enough memory` with 0, 3, 4, 8, 16, and 36 tables, and 1,147 limits panic with 37.
  With a Pack prelude and the `require` function installed too, and 46 tables in all, as 36 inside `args` and 10 of the prelude, as 36 inside `var` and 10, as 18 inside each and 10, and as 46 of the prelude, every limit gives a built VM or a memory error, and with 36 inside `args` and 11 of the prelude 576 limits panic.
  A review's pass with two Packs met the panic at 763 limits with 36 tables inside `args` and 10 table globals in the first Pack, at 576 with 5 in each Pack, and at 1,144 with no table inside `args` and 46 in the first Pack.
  With one Pack whose prelude calls `json.decode` before it makes 10 tables global, and 36 tables inside `args`, it met the panic at 1,191 limits.
  Each count of limits that panic is that of one pass, and two passes of one binary differ: six more of three reviews gave 1,161, 1,151, 1,145, 1,162, 1,149, and 1,130 for 37 tables, and three gave 637, 636, and 646 for 11 on `22a0992f`.
  The limits that give a built VM were the same in each of them, so a limit that the build does not fit ends it with the panic in one run and with `not enough memory` in the next.
  The cause was not looked for.
  Through the scheduler, with `lua_memory_bytes` at 1 MiB, a child whose `item` holds nine references to a string of 100,000 bytes and a filler of 30,000 to 99,999 bytes runs with a filler of up to 39,190 bytes and fails with `not enough memory` from 39,191, with no panic and no abort; on `22a0992f` it runs up to 42,493, by a review's run.
  With `args` of one string of 985,000 to 995,999 bytes under that limit, every build fails with the memory error of the install it was in.
  The heap of a built VM with empty `args` is 111,723 bytes, and 100,752 after two collections, where `22a0992f` has 102,843 and 97,215; 528 of those bytes are the growth of the stack.
- **A chunk that `mlua` loads with no name is named by the Rust line that loads it.** `Lua::load` takes the place of its caller for the name, so the position of such a chunk has a file of the crate, that line, and then the line of the chunk (`state.rs` of `mlua` 0.12.0).
- **Lua writes a chunk name of at most 59 bytes whole.** `LUA_IDSIZE` is 60 in `luaconf.h`, with the zero byte at the end counted, and `luaO_chunkid` in `lobject.c` writes a longer name as `...` and its end.
- **A path of `var` costs a write that is taken nothing.** 1,000,000 integers written to a list took 0.27 s to 0.30 s at the top of `var` and 0.27 s to 0.28 s under 120 tables with keys of 64 bytes, and 2,000,000 took 0.54 s to 0.55 s in both places, over three runs in a release build.
  On `22a0992f` the same run gave 0.50 s to 0.52 s and 1.14 s to 1.17 s for 1,000,000, and 1.01 s to 1.03 s and 2.31 s to 2.33 s for 2,000,000.
  Each time is that of a whole block, `for i = 1, count do list[i] = i end return #list`, with the VM made for it and the read-back of `var` at its end.
  A review timed the loop alone for 1,000,000: 0.16 s in both places, against 0.36 s to 0.39 s and 1.01 s to 1.04 s on `22a0992f`.
  A write at the top of `var` builds no text for its target either; what part of its gain that is was not measured.

Costs, in a release build, over stock Lua, measured when the replacements became one frame.
The two rows of 1,000 strings and the two of a sort with no `__lt` were measured again on the final tree, as the least of five runs, and the other rows were not:

| Call                                                       | Stock | Replacement                           |
| ---------------------------------------------------------- | ----- | ------------------------------------- |
| `tostring` of a number or a table                          | 1×    | 1.7× to 2.1×, 0.07 to 0.11 µs more    |
| `tostring` of a value with `__tostring`                    | 1×    | 11×, about 0.31 µs more               |
| `string.format('%d:%d', ...)`                              | 1×    | 2.3×, about 0.24 µs more              |
| `string.format('%d:%s', ...)` of a string or a table       | 1×    | 5.4× to 7.7×, 1.1 to 1.2 µs more      |
| `string.format('%d:%s', ...)` of a value with `__tostring` | 1×    | 12×, about 1.9 µs more                |
| `table.concat` of 1,000 strings                            | 1×    | 1.06×                                 |
| `table.concat` of 10 strings                               | 1×    | 3.7×, about 0.47 µs more              |
| `table.concat` of 1,000 strings in a list with a metatable | 1×    | 5×, about 0.035 µs more for each item |
| `table.sort` of 100,000 integers                           | 20 ms | 76 ms                                 |
| `table.sort` of 100,000 integers and one table with `__lt` | 22 ms | 139 ms                                |
| `table.sort` of 100,000 integers behind a metatable        | 21 ms | 77 ms                                 |

On `22a0992f` the same four rows took 5.6× for 1,000 strings, with a metatable and without, and 83 ms for the sort, with a metatable and without.
The integers of the sort are pseudo-random from a fixed seed, and the metatable is an empty one.
A review measured eleven of the rows on the tree of its round, with a number for the first row and a string for the fourth: 2.2×, 9.4×, 2.5×, 7.7×, and 12× for the five of `tostring` and `string.format`, 1.04×, 4.3×, and 3.7× for the three joins, and 76 ms, 151 ms, and 81 ms for the three sorts, against 22 ms to 23 ms.

Costs against `22a0992f`, in a release build, on the final tree.
Each row is a whole block of 1,000,000 calls, where the row names no other count, run on a copy of `22a0992f`, on the final tree, and in a stock `Lua::new()`, in milliseconds.
The two numbers are the least and the most: of ten runs on `22a0992f`, of fifteen on the final tree, and of twenty-five in stock Lua.
Where a row here is a row of the table above, the times here are the later ones.

| Block                                       | Stock      | `22a0992f`   | This change  |
| ------------------------------------------- | ---------- | ------------ | ------------ |
| `pcall(f)`                                  | 20 to 24   | 90 to 106    | 96 to 102    |
| `xpcall(f, handler)`                        | 22 to 25   | 185 to 190   | 209 to 214   |
| `pcall(f)` that fails, 100,000 calls        | 27 to 32   | 36           | 39 to 44     |
| `tostring(i)`                               | 109 to 113 | 191 to 200   | 193 to 222   |
| `tostring('abc')`                           | 26 to 35   | 171 to 175   | 92 to 95     |
| `tostring(t)` of a table                    | 101 to 109 | 198 to 201   | 143 to 157   |
| `string.format('%d:%d', i, i)`              | 175 to 191 | 487 to 501   | 430 to 439   |
| `string.format('%d:%s', i, 'abc')`          | 156 to 165 | 935 to 962   | 1066 to 1081 |
| `string.format('%d:%s', i, t)` of a table   | 260 to 271 | 1379 to 1393 | 1389 to 1431 |
| `table.concat` of 10 strings                | 157 to 188 | 1004 to 1049 | 519 to 534   |
| `table.sort` of 100,000 integers, one sort  | 22 to 26   | 86 to 88     | 80           |

So, by the least time of each row, a `pcall` costs about 0.006 µs more than on `22a0992f`, an `xpcall` about 0.02 µs, a `pcall` that fails about 0.04 µs, `%s` of a string about 0.13 µs, and `%s` of a table about 0.01 µs.
`tostring` of a string or of a table, a format with no `%s`, the join, and the sort cost less than on `22a0992f`, and `tostring` of a number costs 0.002 µs more, which is inside the spread of the runs.
Before the captures read a metamethod by a name made once, `tostring` of a table took 222 ms to 241 ms and `%s` of a table 1498 ms to 1514 ms: the capture made a Lua string of the name of the type at each call.
The cost of `pcall` and `xpcall` is the check of their arguments.
The causes of the other costs were read from the code and not measured apart: a caught text now goes through the rule on the position of an Engine chunk, and the walk of a format now takes where each conversion starts and ends, to hand the stock function a part of the format.
A test of the arguments before that walk, which hands a format to the stock function whole when no argument can print with an address or through a `__tostring`, was built, measured, and taken out.
It took `%s` of a string to 694 ms to 738 ms and `%s` of a table to 1603 ms to 1746 ms, so it trades the cost of one row for that of another.
The build of a section VM of the test support takes 0.19 ms in a release build and 0.56 ms in a debug build, against 0.17 ms and 0.54 ms on `22a0992f`: the least of three rounds of 200 VMs, at a load average of 6.

The readers of a table that has a metatable, in a release build, in milliseconds.
Each number is the least of seven runs of a block, and a row gives the least and the most of three rounds, each round one tree after the other, at load averages of 4.5 to 6.8: on a copy of `22a0992f` and on the final tree.

| Block                                                                      | `22a0992f` | This change |
| -------------------------------------------------------------------------- | ---------- | ----------- |
| `json.encode` of ten lists of two integers from `json.decode`, 100,000     | 718 to 726 | 665 to 678  |
| `messages:user` of the same list, 50,000                                   | 390 to 403 | 423 to 427  |
| `messages:user` of ten plain lists of two integers, 50,000                 | 301 to 307 | 337 to 340  |
| `next` over a table of `var` with three keys, 1,000,000                    | 899 to 945 | 785 to 786  |
| `pairs` over a table with three keys and an empty metatable, 300,000 loops | 562 to 585 | 518 to 526  |
| `json.encode` of a table of `var` that holds three tables, 100,000         | 410 to 419 | 364 to 375  |

Three earlier rounds, at load averages of 3 to 8, had the tree as it read the fields of a metatable by Rust text beside the other two, before `view_data` made its budget at need.
In them `22a0992f` took 705 to 726, 391 to 394, 302 to 308, 907 to 928, 569 to 603, and 403 to 424, the tree that read by Rust text 797 to 821, 491 to 510, 336 to 340, 1000 to 1033, 568 to 613, and 439 to 463, and the tree with the names 658 to 684, 422 to 438, 333 to 354, 813 to 830, 528 to 547, and 369 to 383.
A round before those three, begun as the build of the tree with the names ended, gave 764, 459, 356, 935, 603, and 428 on it, and is left out.
With the test of a weak table taken out of the read, the tree that read by Rust text took 696, 438, 334, 906, 535, and 381 in one round, against 794, 498, 343, 1004, 562, and 439 with the test in that round: 0.09 µs to 0.15 µs for each table that has a metatable.
With the name of `__mode` kept in the VM's app data, it took 784, 474, 341, 949, 547, and 414.
The second and the third row are a conversion to data, which takes 5% to 16% more than on `22a0992f` over the six rounds: 0.04 µs to 0.09 µs for each of the eleven tables of a call.
The other four rows take 4% to 17% less.

The same readers of a table with no metatable, in a release build, in milliseconds: the least of nine runs of a block, in three rounds at load averages of 3.4 to 5.2.

| Block                                                        | `22a0992f` | This change |
| ------------------------------------------------------------ | ---------- | ----------- |
| `next` over a table with three string keys, 1,000,000        | 613 to 630 | 592 to 597  |
| `next` of an empty table, 2,000,000                          | 221 to 228 | 198 to 214  |
| `pairs` over a table with three string keys, 300,000 loops   | 461 to 468 | 471 to 515  |
| `pairs` over a list of three items, 300,000 loops            | 341 to 347 | 344 to 347  |
| `pairs` of an empty table, 1,000,000                         | 537 to 550 | 538 to 552  |

`view_data` made a `WalkBudget` at each call, which a review measured at 13 ns, and in that review `pairs` of an empty table took 546 to 560 against 532 to 542; it now makes one at the first `__mode` that has bytes.
The third row is 0.7% to 11% more in these rounds, and its runs differ more than the trees do: 1,000,000 calls of `pairs` on that table from Rust took 1507 ns and 1601 ns a call on `22a0992f` and 1480 ns and 1302 ns on the final tree, in two rounds.
The `next` rows take 3% to 12% less, and the cause of neither difference was found.

Of the depth of the replacements: a replacement calls the author's `__tostring` or comparison function as a Lua function, which takes none of Lua's 200 C calls.
A `__tostring` that prints its own value with `tostring`, `string.format`, or `table.concat`, and a comparison function that sorts, therefore nest until Lua's own stack is full, and fail with `stack overflow` at the default memory limit of 64 MiB, where stock Lua fails with `C stack overflow` within its limit of 200 C calls, `LUAI_MAXCCALLS` in `ldo.h`: a review counted 196 calls on a stock Lua 5.5.1.
The nests through `tostring`, `string.format`, and `table.sort` fail after 0.12 s to 0.22 s in a debug build and after 0.02 s to 0.04 s in a release build.
The nest through `table.concat` fails after 1.7 s to 2.6 s in a debug build and after 1.5 s to 2.5 s in a release build where the `__tostring` is `return table.concat({ self })`, and after 2.8 s to 3.4 s and after 2.7 s to 3.5 s where it is `local s = table.concat({ self }) return s`.
Each range is the least and the most of all the runs made, by the author and by reviews, at load averages of 2.5 to 13.
The time of that nest does not follow the load: one release build of one tree gave 1.6 s to 1.8 s, 2.0 s to 2.1 s, and 2.3 s to 2.5 s for the first form in three runs, the second at a load average of 2.8 and the third at one of 7 to 12, and the cause was not found.
Each level of the nest through `table.concat` has a failure of the stock function, which the replacement catches, and Lua walks its whole call stack after a failure that a `pcall` catches (`stackinuse` under `luaD_shrinkstack` in `ldo.c`).
So the time of that nest goes with the square of its depth, which Lua's limit of 1,000,000 stack slots bounds, `LUAI_MAXSTACK` in `ldo.c`.
Lua raises that failure in the frame that asks for more stack, so its text starts with `?:?:`, which Lua writes for a function with no debug info, when that frame is the replacement's, and with the author's position when it is the metamethod's.
The rule on the position of an Engine chunk takes the `?:?:` off.
A `__len`, a `__lt`, an `__index`, or a `__newindex` that calls the replacement again is a metamethod that Lua runs for an operator of the replacement, which takes one of Lua's C calls at each level, so that nest fails with `C stack overflow` and no position, as in stock Lua.
A probe ran six such nests, through the `__len`, the `__lt`, the `__index`, and the `__newindex` of a sort and through the `__len` and the `__index` of a join: a `pcall` around each caught the string `C stack overflow` with no position, after 196 levels, and after 195 with one more `pcall` around the probe.
The stock `lua5.5` 5.5.1 of the machine has 196 levels for that nest in its main chunk, 195 in a coroutine, and 194 with one more `pcall` around it there.
On `22a0992f` each of the six fails after 98 levels with no position, and after 97 with the author's position in front of the text when one more `pcall` is around the probe.
With the read of a metamethod by Rust text, which `mlua` pushes under a protected call, five of the six gave an error value, since that protected call was the C call that met the limit.
A `__len` that takes the length again, with `tostring({})` at each level, fails with the author's line in front of `C stack overflow`, as in stock Lua, where `22a0992f` has no line.
With `string.format('%d', 1)` or `table.concat({ 'a', 'b' })` at each level the text has no position, on `22a0992f` too, where stock Lua has the author's line: each calls the stock function under a `pcall`, and that is the C call that meets the limit.
The failure is the same in every run.
Lua goes on from a failure that a `pcall` caught inside a coroutine with its count of C calls set back (`precover` under `lua_resume` in `ldo.c`), and a block is a coroutine.
So a nest of such metamethods with a caught failure at each level passes the 200 C calls and runs until Lua's stack is full: a `__len` that calls `pcall(error, 'x')` and takes the length again fails after 250,036 levels and 37 s in a release build, on `22a0992f` and here.
The join of a value that is no string and no number has a caught failure, that of the stock function, so a `__len` that joins a list with the message list in it and takes the length again fails with `stack overflow` after 249,982 levels and 38 s in a release build, where `22a0992f` fails at once with `C stack overflow` at 195 levels.
Under a smaller memory limit the nest can fill the heap before the stack, and it then fails with `not enough memory`.
A probe ran the four nests at limits of 1, 2, 4, 8, 16, 24, 32, 48, and 64 MiB: the nest through `tostring` fails with `not enough memory` up to 16 MiB, those through `table.concat` and `table.sort` up to 24 MiB, and that through `string.format` up to 48 MiB, and each fails with `stack overflow` at the limits above.
Each `__tostring` of that probe returns the call, as in `function(self) return tostring(self) end`, which is a tail call.
With the result kept in a local first, `local s = tostring(self) return s`, the nest through `tostring` fails with `not enough memory` up to 24 MiB, and those through `table.concat` and `string.format` up to 24 MiB and 48 MiB as before.
A function of the author's that calls itself with no replacement between, `local function f() return 1 + f() end`, fails with `not enough memory` at each of the nine limits, on `22a0992f` too.

Of the order of other containers, read and left as they are:

- **The hash containers of the Engine and Harness crates.** No order of a `HashMap` or `HashSet` reaches an effect, an event, or a record.
  The withdrawn ids of a step are sorted when the step is taken, the ids that wait on a task are sorted, the registries of tools, streams, and Packs are ordered maps, and the loops over the file system and the admission add up or check, in any order.
  The `headers` map of the MCP Plugin reaches the HTTP request and a sorted debug print only.
- **The Lua shims.** The option check of a task walks with the Engine's `pairs`, a task set is read by index from 1 to the first nil, as `ipairs` reads it, and the forwarder of the globals copies.
  The `tasks` chunk reads a set, a handle, and the options of the author's by index and by key, and it now refuses a weak one.
  The guard of the globals reads the metatable that the author gave the globals, as Lua reads a metatable.

## Left alone

Closed here, of what the first plan left alone:

- **The conversion of a table with a gap,** by the rule on a table that becomes data.
- **An offer whose `__index` fills a gap,** by the rule on the array of an offer.
- **The size of one refusal of `tools.adhoc`,** by the rule on a quoted key or name.
- **A weak table that another Engine function reads,** by the rule on a weak table in every reader.
- **The guide in `runvil-docs`,** which changes with this plan.

Known:

- **The position rule changes a text that only looks like a position.** A text that starts with `?:?: `, or with `crates/runvil-internal/lua/src/`, a name with no `/`, `:`, or line break in it, `:`, digits, and `: `, loses that start, whoever wrote it.
  The loss is that one start: `error('?:?: boom', 0)` reads `boom`.
  Under a `pcall`, a `__close` handler gets a failure as Lua raised it, with the position, because it runs before the Engine's `pcall` reads the failure.
  That is a refusal that Lua raises in a replacement and one that it raises in the guard of the globals for a handler it cannot index, each with `?:?: ` in front.
  It is also a stack overflow, with `?:?: ` or with an Engine file and a line in front, and an `error` at level 3 in a function under the Engine's `pcall`, with a line of `__impl_coro.lua` in front, where the `pcall` gives `boom`.
  Under an `xpcall` a handler gets what the message handler returned, as in stock Lua, and that handler got the text without the position.
  The refusals in a replacement are the read or the write of a list that takes none, the call of a `__tostring` that cannot be called, and the comparison of two values with no order.
  Stock Lua gives a handler those refusals with no position, so a handler that compares the text finds the difference, and the `pcall` around it finds none.
  The text without the start is a new string, so a caught text of N bytes that has such a start needs N bytes of the heap for it, and with less free the Engine's `pcall` raises `not enough memory`, where `22a0992f` returns `false` and the text.
  A review ran `pcall(error, s, 0)` under a limit of 4 MiB for an `s` of 2,240,000 bytes that starts with `?:?: `: the block fails with the memory error, and the handler of an `xpcall` gets it as an error table.
  A `pcall` that returned the text with its start when the copy fails would hand the author's code the position of an Engine chunk, or not, by the memory that is free.
- **An argument error under a tail call or a `__call` metamethod.** An argument error names a replacement as the author's call is written, and the calls below read otherwise than in stock Lua, for the reasons under "Decisions".
  A replacement that is tail-called is named by the field of the library: `local order = table.sort` and then `return order(5)` reads `sort`, where stock Lua reads `order`, and `return table.sort(5)` reads the same in both.
  The guard of the globals tail-calls a handler that is a function, so a replacement that is the `__index` or the `__newindex` of the globals is named that way too: after `setmetatable(_G, { __index = string.format })`, a read of an unset global reads `bad argument #1 to 'format'`, where stock Lua reads `'index'`, and `22a0992f` reads `'format'` too.
  The tail call keeps the levels of the handler as stock Lua has them, and leaves no frame of the guard for the name to be read from.
  The Engine's `xpcall` tail-calls the author's handler, on `22a0992f` too, so a replacement that is the handler is named that way as well: `xpcall(function() error({}) end, string.format)` returns `bad argument #1 to 'format' (string expected, got table)`, on both trees, where stock Lua reads `'string.format'`.
  That tail call leaves no frame of the Engine's either.
  A method call that is a tail call counts its value, and a refusal of that value is no bad self: `return ('%d'):format('x')` names argument 2, where stock Lua names argument 1, and `return text:format()` on a table reads `bad argument #1 to 'format'`, where stock Lua reads `calling 'format' on bad self`.
  A replacement that is the `__call` of a value has that value among its arguments: with `callee = setmetatable({ 2, 1 }, { __call = table.sort })`, `callee(5)` names argument 2, where stock Lua names argument 1, and a refusal of the value itself reads `bad argument #1`, where stock Lua reads `bad extra argument #1`.
  A replacement that a block took out of its field of the library, or put in another, and that no Lua code calls, as in `pcall(sort, 5)` for a local `sort`, is named `table.sort`, where stock Lua reads `?`, or the field that it finds the function in.
  A name of the call that is not UTF-8 reads U+FFFD for each part that is not, where stock Lua writes the bytes.
  Each of those texts is the same in every run, and a probe ran each beside stock Lua.
  The count of a method call that is no tail call, `#2` in the first plan where stock Lua says `#1`, is now stock Lua's.
- **A capture that makes a Lua string at Lua's limit of nested C calls, or of its stack, fails as an Engine function does.** A capture of the replacements makes a Lua string, under a protected call of `mlua`'s, to refuse, to read the metatable of a userdata, which `mlua` reads by Rust text alone, to print a Rust callback's failure, and to print a value whose `__name` has a zero byte in it.
  A capture reads the metatable of a userdata to print it, to take it as the list of a join or of a sort, and to compare it in a sort.
  Inside a nest of about 196 metamethods that Lua runs for an operator, that protected call can be the C call that meets Lua's limit of 200.
  A `pcall` around the nest then catches an error value that reads `prompt:2: C stack overflow. The Lua code failed at runtime. Fix the code at this location.` for a call on line 2 of the block, where Lua's own failure is the string `C stack overflow`.
  A probe ran a `__len` that takes the length again, with one call more at each level: `tostring` of the message list gives the error value at 196 levels, where `22a0992f` gives the string, and so does `tostring` of a table whose `__name` has a zero byte; `string.format('%s', ...)` of the message list gives the string on both trees, at 195 levels here and 194 there.
  A join of the message list at each level, and a sort of a list that holds a table with `__lt` and the message list, give the error value at 196 levels, where `22a0992f` gives the string with the author's position in front at 195.
  `tostring` of an error value at each level gives the error value at 197 levels, where `22a0992f` gives the string at 195: the `__tostring` of an error value is a Rust function of the Engine, which reads the message by Rust text.
  `json.encode({ 1 })` at each level gives the error value on both trees, as a Rust function of the Engine does at that limit.
  The end of Lua's own stack can be met in the same protected call.
  A `__tostring` that prints the message list and then its own value with `tostring` was run with 0 to 23 more lines of a local in it: a `pcall` around the nest caught an error value that reads `prompt:13: stack overflow. The Lua code failed at runtime. Fix the code at this location.` at one of the 24 sizes of frame, and the string `stack overflow`, with the author's position or none, at the other 23, where `22a0992f` gives the string `C stack overflow` at all 24; a review found it.
  The other reads of the metatable of a userdata are the same call, and were not run at that limit.
  The failure is the same in every run, and a block with no such nest does not meet it.
  A fix reads the metatable of a userdata with no protected call, which `mlua` 0.12.0 does only in a walk of all its fields; the walk was not built, for its cost at each comparison of a sort of userdata values, which was not measured.
- **A `C stack overflow` under `string.format` or `table.concat` has no position.** Each calls the stock function under a `pcall`, on `22a0992f` too, and a `pcall` is a nested C call.
  So a `__len` that takes the length again, with `string.format('%d', 1)` or `table.concat({ 'a', 'b' })` at each level, meets Lua's limit in that `pcall`: the caught text is `C stack overflow`, on both trees, where stock Lua puts the author's line in front.
  The failure is the same in every run, and a test holds the text.
- **A nest of operator metamethods that joins a value that is no string runs to the end of Lua's stack.** The replacement of `table.concat` tries the stock function first, under a `pcall`, and a value that is no string and no number fails it.
  Inside a coroutine, which a block is, Lua goes on from a caught failure with its count of C calls set back, so a `__len` that joins a list with the message list in it and takes the length again does not fail at 200 C calls: it fails with `stack overflow` after 249,982 levels and 38 s in a release build, where `22a0992f` fails at once with `C stack overflow` at 195 levels.
  Lua's limit of 1,000,000 stack slots bounds the depth, and the time goes with the square of the depth, since Lua walks its call stack after each caught failure (`stackinuse` under `luaD_shrinkstack` in `ldo.c`).
  An author's own `pcall` in such a nest does the same on `22a0992f`: a `__len` that calls `pcall(error, 'x')` and takes the length again fails after 250,036 levels and 37 s on both trees.
  A fix tests each value of a list before the stock function is tried, which costs every join a read of each value; it was not built, and that cost was not measured.
- **A failure under the guard of the globals has no position in a caught text.** The `__index` and the `__newindex` of the guard are Lua functions, on `22a0992f` too, and the guard reads a handler of the author's that is no function in its own frame.
  So a handler that Lua cannot index fails with `?:?: ` in front, which the position rule takes off: after `setmetatable(_G, { __index = 5 })`, a `pcall` around a read of an unset global gives `attempt to index a number value`, where stock Lua has the line of the read in front.
  A handler that is a table with an `__index` function of its own runs that function over the guard's frame, so `error('boom', 2)` in it gives `boom`, where stock Lua names the line of the read, and a block that fails with it is placed on the line of that function.
  A handler that is a function is tail-called, and its levels are stock Lua's.
  A fix follows the chain of `__index` and `__newindex` in the guard as Lua does, and calls the function at its end as a tail call; it is a change of its own.
- **`pairs` and `next` word their own argument errors.** Both are Rust functions of the Engine, as on `22a0992f`: `pairs(5)` raises `bad argument #1 to 'pairs' (table expected, got integer)`, where stock Lua's `pairs` takes any value, and `next(5)` raises `bad argument #1: error converting Lua integer to table`, in `mlua`'s words.
  This change touches neither text.
- **Level 3 under the Engine's `pcall` and `xpcall` has no position.** Both are Lua functions around the stock ones, with debug info, on `bdcadfbb` too, so `error('boom', 3)` in the called function names a line of `__impl_coro.lua`, which the position rule takes off.
  `pcall(function() error('boom', 3) end)` gives `boom`, and the same under `xpcall`, where stock Lua names the line of the `pcall`.
  The stock `pcall` is a frame of its own under the Engine's, so no debug info there would give level 3 no position and level 4 the author's line, one level off.
- **A record of the loop can nest deeper than 127, or hold more values or more text than one conversion spends.** `models.loop` appends the tool calls of a model as the model gave them, and `parse_record` does not see them.
  A call whose arguments nest more than 124 deep makes a record more than 127 deep, a view of which is refused wherever a value converts, `append` and `replace` among them.
  So is a view of a record whose arguments hold more values than one eighth of the memory limit in bytes, or more bytes of text than the limit, which no test runs.
  The arguments come from the caller's answer to a chat, and a check of them is a change to the loop, not to the conversion.
- **`#t` of a table with a gap, and what stock Lua measures by it.** `#t`, `table.insert`, `table.remove`, `table.concat`, `table.unpack`, and `table.sort` give such a table a length that differs between two runs, as on `bdcadfbb`.
  `#t` is an operator, and `mlua` seeds the hash of a new state itself and takes no seed from its caller, so the Engine cannot fix the length.
  `table.concat` and `table.sort` stay with `#list` for the reason under "Decisions".
  The lists that the Engine itself measures by `#t`, in a batch answer and in the calls of a model reply, are made without a gap by the Engine's own code; that was read from their comments and not tried.
  A list in `var` has no gap: a write is refused that would leave one, by the code of the guard, which was read.
- **A list's other keys in the conversion.** A table with an item 1 converts to a list, and what its other keys hold is left out with no error, as on `bdcadfbb`: a name, a boolean, a number with a fraction, the key 0, and a negative index.
  `{ [1] = 'a', [2] = 'b', [2.5] = 'x' }` converts to `["a","b"]`, and `{ [-1] = 'a', [1] = 'b' }` to `["b"]`, on `22a0992f` too.
  The result is the same in every run, so it is no fault of determinism, and `json.encode` refuses the same table.
  A refusal here would change what a prompt can pass today, and is a change of its own.
- **The time of the scans of `__mode` in one call.** One walk scans at most the memory limit and one scan more, so less than 128 MiB at the default: 75 ms in a release build and 0.41 s in a debug build on this machine, with no cancel inside.
  The conversion of a table to data walks a value that it refuses a second time, so it scans less than 256 MiB, which is twice those times by reckoning and was not run.
  Outside that count, `json.encode` scans the `__mode` of its options once, `models.use` that of its options, and the prelude install that of the environment.
  `tools.adhoc` reads its attributes and its parameters through a view, with a budget for each of the two reads, so each scans as much as one walk does at most.
  A `tasks` function scans the `__mode` of a table before each read of it, each scan a Rust call of its own that reads as many bytes as one walk does, the bytes above, since the `__mode` that passes the limit is read to its end before it is counted: one scan for a member of a set that is a task id, and two for a member that is a handle.
  The check of a read by key scans the tables on the way of that read too, within the budget of the same Rust call.
  That check also takes time for each table on the way, whatever its `__mode`: about 0.11 µs in a release build and about 0.92 µs in a debug build on this machine, where the read that Lua makes takes about 6 ns and about 16 ns.
  `tools.offer` makes the check for each entry that its array lacks, and one `prose` for each global of a placeholder that the globals lack, in one Rust call with no cancel inside, and the budget of that call holds the tables of all its checks to 8,388,608 at the default limit.
  So one call of `tools.offer` took at most 1.02 s in a release build and 8.2 s in a debug build, for an array past the budget, which it refused, and for 100,000 entries behind 83 tables, the most that fit; one `prose` took at most 0.97 s in a release build.
  With no count, 100,000 entries behind 2,000 tables took 21.3 s in a release build, and before the check the same call took 1.2 s.
  The time follows the memory limit that the Host sets: about 15 ms for each MiB in a release build, up to those 21.3 s, which a limit of 1.5 GiB allows, reckoned and not run.
  A run that is cancelled inside such a call ends when the call does.
  A `tasks` function makes the check in a Rust call of its own for each read, so at most 426 checks come between two runs of the hook: about 0.1 s in a release build and about 0.8 s in a debug build, by 2,000 reads of a handle behind 2,000 tables, which took 0.46 s and 3.7 s.
  A cancel takes effect at the hook, which runs once in 10,000 instructions of Lua, and not between two scans.
  A member that is a handle is 47 instructions of the chunk, and one that is an id 33, by the listing of `luac` 5.5.1, so at most 426 scans come between two runs of the hook: about 32 s in a release build and about 175 s in a debug build at the default limit, reckoned from the times above and not run.
  With the set read by `ipairs`, as in `4fdbe940`, the two were 40 and 28 instructions, and the scans at most 500.
  A review measured a `__mode` of 16 MiB with no `k` or `v` in a debug build: 46 ms for one scan, 9.22 s for a `tasks.join` of 200 members and 18.59 s for one of 400, and 9.20 s for 200 calls of `pairs` in a loop of a block, which scans as much.
  The count of members is that of the author's set, so the scans of one call have no limit of their own, as those of a loop in a block have none.
- **The time of a conversion that names a fault.** The second walk visits every value, makes a reason for each fault it finds, and asks `from_value` once about each key and each string that `from_value` refuses.
  4,000,000 references to one table took 2.4 s with a key table in it and 3.7 s with a float key in it, against 2.5 s for a table that converts, and 8,000,000 references to a string that is not UTF-8 took 4.3 s, in a release build on this machine with no cancel inside.
  One conversion visits at most 8,388,608 values at the default limit.
  A walk that asked only about the fault it keeps would make a number of tables that follows Lua's own order; that was reasoned and not tried.
- **A walk passes the empty slots of a table at each visit.** A table that held items and holds none keeps its slots, and Lua's own walk passes each slot to find no entry.
  Every walk of a conversion makes that pass at each visit of the table, and a visit costs the budget one value, whatever the slots.
  The read through views first asks whether a table with `__metatable` has an entry, which is one pass more.
  `json.encode` makes one pass, and two with a `__metatable` on the table.
  A conversion to data makes two, by its walk and by the conversion, and three with a `__metatable`, as on `22a0992f`.
  A conversion that is refused makes one more, by the walk that names the fault, and that walk asks about a `__metatable` too: three passes, and five with a `__metatable`, where `22a0992f` makes two and three.
  On `22a0992f`, 4,000 visits of a table that had held 400,000 items, which left 524,288 slots, took `json.encode` 0.58 s in a release build and 2.10 s in a debug build, and 1.14 s and 4.21 s with a `__metatable` on the table.
  That is about 0.28 ns for a slot in a release build and about 1.0 ns in a debug build.
  A review measured both trees in a debug build, over 400 visits of a table with 100,000 emptied slots.
  A conversion took 107 ms on each tree, and 158 ms on each with a `__metatable`; a refused one took 158 ms against 105 ms on `22a0992f`, and 265 ms against 158 ms with a `__metatable`.
  The most was reckoned and not run.
  A slot of the list part of a table is 9 bytes, and a slot of the other part is larger, by `ltable.c` and `lobject.h`, so a table inside the default limit of 64 MiB has fewer than 7,456,541 slots, and one walk visits at most 8,388,608 values, so one pass reads fewer than 6.3e13 slots: 4.9 hours in a release build and 17 hours in a debug build.
  One refused conversion of a value whose tables have a `__metatable` is five such passes, about 24.5 hours in a release build, in one Rust call that no cancel interrupts, where `22a0992f` makes three, about 14.7 hours.
  `local t = emptied for i = 1, 22 do t = { t, t } end`, with an `emptied` of 4,194,304 slots, is 4,194,304 visits inside that budget and 1.8e13 slots for a pass, which is 1.4 hours and 4.9 hours.
  The pass more for a refused value is the walk that names the fault, which "Decisions" has.
  A budget counts values and cannot count slots, which `mlua` does not give, and a read of the cancel flag inside a walk is a change of its own: every caller of a walk would tell a cancel from a value that is too large.
- **Other texts that a message quotes whole.** The 64-byte rule covers a path and the refusals of `tools.adhoc`.
  Seven other messages were run with a key of 1,000 bytes, and each quotes it once and whole: an unknown key of `tools.offer`, of the options of `json.encode`, and of the options of `tasks.spawn`, a write to `args` and to `sys`, a read of an unknown field of `sys`, and a key of a `tasks.fanout` collection.
  Such a text is the key and the words around it, so it is at most the VM's memory limit, 64 MiB at the default, and a block can raise a text of that size with `error`.
  The other messages of the Engine that quote a string of the author were not read for their size.
  One function for the text of a key serves these messages and the lookup of a tool by its name, so a cut there would change the lookup.
- **A fault in `mlua`'s words names no place.** The conversion gives a fault that `mlua` names `mlua`'s own text, which has no path: `tasks.spawn('S', { item = { f = function() end } })` raises `item must be a JSON-representable value: deserialize error: unsupported value type` and the type, as on `22a0992f`.
  The short form of `tools.adhoc` does the same for a parameter's name or description that is not UTF-8: `error converting Lua string to &str (invalid utf-8 sequence of 1 bytes from index 0)`, on the line of the call, as on `22a0992f`.
  The second walk has the steps to that fault, and a text with the path in front would differ from the one `mlua` gives for the same value, which the rule on a table that becomes data keeps and a test compares.
  It is a change of the wording of every such refusal, and of the chapters that quote them, and is a change of its own.
- **NaN and the infinities in the conversion.** `{ 0/0, 1/0 }` converts to `[null,null]` with no error, as on `22a0992f`, where `json.encode` and a write to `var` refuse such a number.
  The result is the same in every run, the pages now state it, and a refusal would change what a prompt can pass today.
- **A key that is no string or integer is shown as its type.** `tools.adhoc` with the key `true` among its attributes raises `unknown key "boolean"`, and with the key `2.5` it raises `unknown key "number"`, as on `22a0992f`.
  With booleans first in the shared order, `"boolean"` is the key the refusal names when one is there.
  The guide states the rendering.
- **A key of the options of a `tasks` function that a walk has no place for is refused in the name of `pairs`.** The check of the keys of `opts` walks the table with the Engine's `pairs`, which refuses a table that holds a table, a function, or a userdata as a key before it gives the first key.
  So `tasks.spawn('S', { bogus = 1, [{}] = 1 })` raises `'pairs' walks boolean, number, and string keys only: the table has a table key, which has no order that is the same in two runs`, and not the refusal of the unknown key `bogus`, as on `22a0992f`; a review found it.
  The text is the same in every run, and the guide now states it where it gives the refusal of an unknown key.
  A refusal in the name of the call needs a walk of the options that takes such a key, which the Lua code of the Engine does not have, and is a change of its own.
- **A refusal for size names no limit.** `the value is too large to convert` says neither which limit the value passed, of values or of bytes of text, nor what the limit is, as on `22a0992f`.
  With this change it is also the refusal of a value whose strings, or whose `__mode` strings, hold too many bytes.
  `value is too large to encode`, `is too large to store`, `blocks are too large to walk`, and `parameters are too large to convert` have the same form on `22a0992f`, and the pages give the two limits.
  A text that names the limit and the count is a change to all five and to the chapters that quote them, and is a change of its own.
- **`models.loop` reads `sys.taskid` from the `sys` global.** The loop takes the first line of a message that arrives during it from whether the section runs in a task, and reads that from the `sys` global, as on `22a0992f`.
  A block that puts a table of its own in `sys` chooses between the two lines, and an `__index` of that table runs once at the start of each `models.loop`; a failure of it reads as the main chain, by the code, which was not run.
  The line is in the block's own conversation, to which the block can append any text, and the same block gives the same line in every run.
  A `sys` whose `__index` leads to a weak table for `taskid` is refused at the call of `models.loop`, by the rule on a read by key, so the collector has no part in the line.
  A fix hands the loop the task of the VM from the Engine's own copy of `sys`, through the install of the shims, and is a change of its own.
- **`models.loop` reads its default compactor from `compactors.fail`.** A call with no compactor reads the `fail` field of the `compactors` table at each call, as on `22a0992f`, and the guide gives that field as the default and says that what it holds is not checked.
  The Engine takes the table when it installs `models.loop`, so a `compactors` global that a block replaces has no part in it.
  The read is not a raw one: with the field taken out, an `__index` of a metatable that a block put on the table is asked for `fail` once at the start of each `models.loop`, inside the Engine's call, which was run.
  That code is the block's own, the same block gives the same compactor in every run, and a failure of it is raised at the call of `models.loop`, by the code, which was not run.
  An `__index` that is a table and leads to a weak one for `fail` is refused at that call, by the rule on a read by key.
  A raw read would change what a prompt can give as the default today, and is a change of its own.
- **A new chunk of the Engine with no name, or with a name that has no test.** Each of the ten names has a test, and nothing finds a load of text that is added without one; "Decisions" has the scan that was taken out.
- **The first `var` of a VM is not counted.** `guarded_var` takes the data it is given with no count of its levels.
  The Engine gives a VM an empty object or the `var` that an earlier read-back gave, which writes made, and neither facade has a way for its caller to give one.
- **JSON text that other crates parse.** `serde_json::Value` reads the key `$serde_json::private::RawValue` as JSON text wherever a crate parses into it, and `plain_json.rs` covers `runvil-lua` alone.
  `harness-gateway-client` parses the arguments of a model's tool call and each chunk of a stream into `serde_json::Value`, `harness-plugin-loader` the parameter schema of a Pack tool, and `runvil-prompt` the arguments of a run.
  The first of the four was run with the feature on, on a thread with a stack of 2 MiB: arguments of 4,634 bytes with the key inside itself parsed to data 1,211 deep, and arguments of 5,257 bytes overflowed the stack, which aborts the process, in a debug build.
  In a release build 47,665 bytes parsed to data 5,688 deep, and 78,993 bytes overflowed the stack.
  The other three were read and not run.
  The arguments of a tool call are text that a model writes, so with the feature on a model reply can end the process of a Host.
  A fix is one reader for the four crates.
  `harness-gateway-client` depends on the `runvil` facade alone, and `harness-plugin-loader` on neither `runvil-lua` nor `runvil-types`, so the reader is a new item of a facade or a new edge between crates, which this plan does not name.
  It is a change of its own.
- **The data of a conversion is held outside the VM's memory limit.** The budget of a conversion counts values and the bytes of strings, and one value of the data takes more memory than the 8 bytes the count gives it.
  The most is an object of one key, about 670 bytes: 66,576 references to a chain of 126 tables with one key each, 1,183 KiB of the Lua heap and 8,388,577 values, converted in 4.48 s with 5.6 GB in the process, which is 5.2 GiB, in a release build with the default limit of 64 MiB.
  `json.encode` of the same value returned 50 MiB of text in 6.28 s with 5.7 GB in the process.
  That is about 84 times the limit, and the page on the limits states it.
  4,194,303 references to `{ a = 1 }` converted with 3.0 GB, and a write of them to `var` ended in `not enough memory` after 2.6 s with 3.3 GB.
  A review measured 669.6 bytes for a value over 1,048,573 values of the first shape, by the peak memory of the process.
  The count of values is that of `22a0992f`, by its code, which was not run for these sizes.
  A fix counts what the data itself takes, as a budget of bytes for the data of one conversion, and refuses values that convert today.
  It changes the budget of every walk, so it is a change of its own.
- **`serde_json`'s `arbitrary_precision`.** With that feature on, `serde_json` hands a number of JSON text to a reader as a map of one key, `$serde_json::private::Number`, and writes a number to Lua the same way.
  No crate of the workspace asks for it, by `cargo tree` over the workspace with all features and all targets.
  A review built both trees with it on.
  On `22a0992f`, `json.decode('[1]')` gives a table for the number, and so does a read of a number from `var`, so the Engine does not work in such a build before this change.
  With this change two more values are that table: a number in a tool's structured output, and a number in the `var` that the VM reads back, since `PlainJson` reads the map as an object.
  A reader that tells the map of a number by its key would make that key more than a name in every build, which the rule on a key of JSON text ends.
  A fix reads a number without `serde_json`'s map, in both directions, and is a change of its own.
- **The Rust copy of `fail`.** `envelope_failure` in `models_loop.rs` calls the `tostring` global for a failure of a loop round that is no table.
  The Engine answers a round with an error table, so no run reaches that call; it was read and left as it is.
- **Engine functions that call each other through the author's code can end a debug build.** A Rust function of the Engine that calls a metamethod takes a part of the thread's stack for each level, and Lua stops the nesting at 200 C calls with `C stack overflow`.
  A `__pairs` that calls `pairs` on its own table needs between 2,368 KB and 2,432 KB of stack to reach that failure in a debug build, on this change and on `22a0992f`, and between 512 KB and 768 KB in a release build.
  `tools.offer` under a `__len` that calls `tools.offer` needs between 1,664 KB and 1,728 KB in a debug build, where `22a0992f` needs between 1,472 KB and 1,536 KB, and between 256 KB and 384 KB in a release build on both.
  On a thread with less, the process ends with `thread has overflowed its stack`.
  The Harness steps a run on the thread that polls its future, which the Host chooses, and no crate of the workspace sets a stack size; a thread of `std` and a worker of `tokio` 1.54.1 have 2 MiB when their maker names no size.
  So a debug build of a Host that polls the run on such a thread ends for the first of those blocks, and a release build does not.
  A fix is a count of the Engine functions on the stack, or a stack check, in every Engine function that calls a metamethod, and is a change of its own.
- **A table that the loop of `pairs` makes weak.** `pairs` tests its table when the walk begins, and the iterator then reads each value by its key.
  A loop body that gives the walked table a `__mode` leaves the entries not yet visited to the collector.
  A test before each step is a Rust call for each step of every `pairs` loop, and the read is one by key, as the entry on a weak table read by key says.
- **Two orders of keys.** A walk of a table, by `pairs`, `next`, or `tasks.fanout`, puts the whole numbers from 1 up first.
  `json.encode` visits the keys of a table, and a refusal names one of several entries, with every number by its value, after the booleans.
  So a refusal of two keyed members of a collection can name the one whose arm would come second.
  Each order is the same in every run, and the guide states both.
- **Two shapes of item in one collection.** A member of the list is the arm's item as itself, and an index past a gap is a pair, so `{ 'a', nil, 'c' }` hands its arms a string and a table.
  Issue #69 names it, and only a refusal of a collection with a gap ends it.
- **A weak table read by key.** What it holds follows the collector, as the first plan says.
  This is a read in a block.
  The Engine's own reads of a table refuse a weak one, and so does a read of one field that Lua makes for the Engine through an `__index` of the author's that is a table, by the rule on a read by key.
  An `__index` that is a function is the author's code, and what it reads from a weak table is a read in a block.
  It is also a metamethod that Lua looks up in a weak metatable, for `t.x`, `#t`, `tostring`, and `pairs`: the replacements read the field that stock Lua reads.
  The guard of the globals looks up the fields of the metatable that the author gave the globals in the same way, and copies them when the author sets it.
  The read of a placeholder's global refuses such a metatable, by the same rule.
- **A table that was weak earlier.** The Engine tests a table when it reads it, and a table that is no longer weak holds what the collector left.
- **The panic of `mlua` near the memory limit, past 68 slots.** [#64](https://github.com/cppalliance/runvil-harness/issues/64) stands: the stack of the Lua values that Rust holds grows under the run's limit when a call needs a slot and none is free, and `mlua` 0.12.0 panics when the heap has no room for the growth.
  A built VM with no data and no Pack prelude has 54 of its 68 slots free, where `22a0992f` has 26 of 34.
  Each of these takes one slot for the life of the VM, on both trees: a table inside `args` or `var`; a table that a Pack prelude makes a global; a Pack whose prelude has a function that reads a global; and each level of Engine functions called inside one another, from the first that returns.
  On this change alone, `pairs`, `next`, and the check of the `tasks` functions take three each, from the first table with a metatable that each reads.
  The build takes 18 more while the coroutine shims install, and 8 more while a prelude loads, with one for each Pack before it until the collector frees that environment.
  So the build grows the stack from 37 tables inside `args` and `var`, and with one Pack whose prelude defines functions and tables alone from 47 with its table globals, where `22a0992f` grows it from 11 and from 19.
  Each further Pack takes one from that count, a prelude that calls an Engine function while it loads takes one, and a prelude that calls `pairs` or `next` on a table with a metatable takes three more.
  With 37 tables inside `args`, 1,130 to 1,162 of the limits that a pass by one byte tries ended the build with the panic in the seven passes made, which is a record of those passes and no bound, and with 36 tables and 10 table globals in the first of two Packs 763 did in one.
  So with a stack of one size this change has 6 slots fewer free than `22a0992f`, and 15 fewer after those three reads, and a call that holds that many values more at once grows the stack where `22a0992f` does not.
  That is a VM whose build grew the stack on `22a0992f`, with 11 to 36 tables inside `args` and `var`; a VM with 10 or fewer has a stack of twice the size here, with 28 slots more free, and 19 more after those reads.
  A call of an Engine function takes a slot for each table, function, string, and userdata among its arguments, and for each one it holds while it runs.
  One handle of `var` for all the preludes would take one slot where each Pack takes one, and was not built: it moves the count by the number of Packs and leaves the fault, as a larger stack does.
  `mlua` 0.12.3 has a form of the growth that returns an error, in its `state/extra.rs`, and a call that returns no `Result`, such as `Lua::globals`, still panics, by its maintainer's note on [mlua-rs/mlua#733](https://github.com/mlua-rs/mlua/issues/733).
  The upgrade also changes how `mlua` walks a table and converts it, by the titles of its commits, and none of that has been run against this change, so it is a change of its own.
- **The other entries of "Left alone" in the first plan.** Each one that this plan does not name stands as the first plan has it.

Not tested:

- **Windows.** The tests ran on Windows in the CI of pull request #78 for `4fdbe940` and for `b306088e`, and passed.
  No test ran on a Windows machine outside that CI, and neither the count of the tables that a read by key follows nor the refusal of a weak table in the read of `tools.adhoc` through a view, both of which `b306088e` lacks, ran on one.
  The rule on the position of an Engine chunk reads names that the Engine writes itself, each with `/` on every platform, and a test holds each name to that opening.
- **`cargo dylint` and Rust 1.95.** Both ran in the CI of pull request #78 for `4fdbe940` and for `b306088e`, and passed.
  Neither ran on this machine, so neither ran on the count of the tables that a read by key follows or on the move to `cc55a351`.

Out of scope:

- [#64](https://github.com/cppalliance/runvil-harness/issues/64), a panic of `json.encode` and `tools.offer` in the Lua binding when the VM is close to its memory limit, and of the build of a VM with more tables than "Known" states.
- [#65](https://github.com/cppalliance/runvil-harness/issues/65), a decimal number of 16 digits that the rule on an address takes for an address on Windows.
- [#67](https://github.com/cppalliance/runvil-harness/issues/67), the empty schema of `tools.adhoc` when `parameters` is a table from `var` or `args`, which `cc55a351` ends on `master`: this change adds the refusal of a weak table to that read, as "Basis" tells.
- [#68](https://github.com/cppalliance/runvil-harness/issues/68), the two copies of every string key that one step of `next` makes.
- A replay of a recorded run.

## Git

- The commits go on `cc55a351`: this plan first, with the new wording of the first plan, then the code in eighteen commits, then the pages.
  The first fifteen of the code are the commits that pull request #78 had on `5d9c7bf8`, and the last three end the faults that its review found.
- A commit of the code makes one change of "Code", with its tests, or several that change the same code.
  In their order, with the numbers of "Code": the label of a failure (2), the position of an Engine chunk (11), the replacements (1, 12, 14, 17, 18, 19, and 20), the chunk of the `__concat` (15), the fields of a raised error table (9), the globals that an Engine chunk reads, the keyed members of `tasks.fanout` (4), the array of `tools.offer` (5), a key as a name (10), the quoted keys and names (8), the depth and the keys of `var` (13), a weak table in every reader (3), the unknown keys of `tools.adhoc` (6), a weak table in the `tasks` functions (16), the conversion to data (7), a read by key that looks into a weak table (21), the bytes of text of a record (22), and the comparisons by identity (23).
  The seven of the replacements change the same two chunks, their install, and the build of the VM that holds what the install makes.
  The commit on the globals has the raw read of the `var` global from 13, and from 16 the globals that the chunks take as locals and the chunk of `require`.
- Each of those commits was built and tested alone, as "Verification" tells.
  Where two commits change one statement, the earlier one holds a form of it that the later one replaces.
- Each message ends with `Plan: /plan/2026-11-23-1-deterministic-lua-follow-up.plan.md`.
- The change closes [#63](https://github.com/cppalliance/runvil-harness/issues/63), the conversion of a list with a gap, [#66](https://github.com/cppalliance/runvil-harness/issues/66), the position of a comparison function's failure, and [#69](https://github.com/cppalliance/runvil-harness/issues/69), the two orders of one table.
- The chapters of the guide are two commits in `runvil-docs`, on its branch `kb/lua-fixes`: one for the first fifteen commits of the code, and one for the last three.
