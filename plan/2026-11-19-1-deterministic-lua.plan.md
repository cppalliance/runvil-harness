---
name: Deterministic Lua
overview: "`math.random` draws from the run's seed and the section entry's id, `table.sort` is a stable merge sort, `tostring` and `string.format` print a table, function, thread, or userdata by its name, a failure made from a raised value names the value's type, `pairs` and `next` walk a table in the order of its keys, and a check that refuses a table for one of several entries names the entry whose key comes first. The length of a table with a gap in its list still differs between runs. Closes issue #22."
todos:
  - id: plan-copy
    content: "runvil-harness: this plan goes to `plan/2026-11-19-1-deterministic-lua.plan.md` in the first commit of the branch `kb/lua-fixes`, before any other change"
    status: completed
  - id: random
    content: "runvil-types, runvil-runtime, runvil-lua: derive a `LuaRandom` from the run's seed, seed each section VM's generator from it and the section entry's id, and replace `math.randomseed`"
    status: completed
  - id: sort
    content: "runvil-lua: replace `table.sort` with a stable merge sort in `__impl_stdlib.lua`, with stock Lua's failures on the author's line"
    status: completed
  - id: tostring
    content: "runvil-lua: replace `tostring` and `string.format` so neither prints an address, refuse `%p`, cut a `__name` at its first zero byte, print a Rust callback's failure as its message where a `__close` handler gets it, and move the `table.concat` wrapper into the same chunk"
    status: completed
  - id: failure-text
    content: "runvil-lua: one text rule, at every place an `mlua` failure becomes text, takes an address of a pointer's width at most out of a raised value's rendering"
    status: completed
  - id: walks
    content: "runvil-lua: `pairs` and `next` walk a table in an order of its keys alone, with no Lua reference held in Rust: the iterator of `pairs` is a Lua function in `__impl_iteration.lua`, `next` returns the least key after the one it was given, and both refuse a table, function, userdata, or thread key and a weak table"
    status: completed
  - id: fanout
    content: "runvil-lua: `tasks.fanout` ends its list at the first gap, reads the keyed part with no Lua reference to each member, refuses the keyed member whose key comes first, and refuses a weak collection"
    status: completed
  - id: offer
    content: "runvil-lua: `tools.offer` ends its array at the first nil that the read meets, and refuses the key outside the array that comes first"
    status: completed
  - id: models-use
    content: "runvil-lua: `models.use` reads one option at a time and refuses the bad option whose key comes first"
    status: completed
  - id: prelude
    content: "runvil-lua: the install of a Pack prelude reads the globals by name, refuses the bad global whose name comes first, and refuses a weak environment"
    status: completed
  - id: labels
    content: "runvil-lua: a read of a view copies no label, and `json.encode` and the Markdown renderer count a label's bytes before they copy it"
    status: completed
  - id: adhoc
    content: "runvil-lua: the short form of the parameters of `tools.adhoc` takes string names only, reads a parameter's type and description raw, refuses the bad parameter whose key comes first, and converts within one `WalkBudget`, with each view counted as six values"
    status: completed
  - id: pages
    content: "decisions: `prompt-language.md`, `tasks-and-fanout.md`, `tools-and-packs.md`, and `engine.md` state the new rules and their reasons"
    status: completed
  - id: verify
    content: "runvil-harness: run every check in AGENTS.md on the last commit, and the tests of `runvil-lua` on each commit"
    status: completed
isProject: false
---


# Seeded randomness, a stable sort, and table walks by key in a section's Lua

Basis: written after the build, on `runvil-harness` `bdcadfbb` with the change in the working tree.
Every number below was measured on that tree, on macOS on Apple silicon, in a debug build where no build is named.
The Lua in the build is 5.5.0, which the lock file pins through `lua-src` 550.1.1.

## The problem

[Issue #22](https://github.com/cppalliance/runvil-harness/issues/22) reports that `math.random` and `math.randomseed` in a section VM are seeded from the clock and a memory address.
The Engine requires that a run with the same seed, start time, and answers produces the same effects, events, and ids.
Stock Lua 5.5 breaks that rule in more places than the issue names:

- `math.randomseed` with no argument, and the `math` library when it loads, read the clock and an address.
- `table.sort` takes a pivot from the clock and an address after an unbalanced partition, so elements that compare equal end in an order that differs between runs.
- `tostring`, `%s` and `%p` in `string.format`, and the text of an uncaught error print the address of a table, function, userdata, or thread.
- A walk over keys that are tables, functions, userdata, or threads follows a hash of their addresses.

The work on those and your review found more, most of them on `master` too:

- **`next` never ends.** In a table that holds the float key `0x1p63` and the integer key `math.maxinteger`, `next` returns the float key again and again.
  It found the key it was given by `mlua`'s equality of values, which rounds the integer to a float.
- **A refusal names a different entry in each run.** `tools.offer`, `tools.adhoc`, `tasks.fanout`, and a Pack prelude each stop at the first bad entry that Lua's own walk meets.
  In twelve processes one `tools.adhoc` call gave three texts.
- **A loop of `pairs` walks panics.** `mlua` keeps one slot for each Lua value that Rust holds, on a stack of about a million, and panics when it cannot get a slot.
  The iterator of `pairs` was a Rust closure that held its table and each string key until Lua collected it.
  Lua collects by the growth of its own heap, to which such an iterator adds little, so up to 458,000 iterators were alive at once.
  A table of 2000 string keys walked 1000 times failed its block with `C stack overflow`, on `master` too.
- **`models.use` panics on a huge options table.** It collected every entry before it checked one, and 1,200,000 entries panicked in 12 of 12 processes.
- **Two keys of `tools.adhoc` parameters name one parameter.** The keys `1` and `'1'` both read as the string `1`, so the type of that parameter, and a second entry in `required`, followed Lua's own walk.
  One table gave three schemas in twelve processes.
- **A walk of a weak table gives a different result in each VM.** The walk reads the table in Lua's own order and holds the entry it has just read while it allocates.
  A collection at that moment clears every entry that nothing else uses, except that one.
  With 100,000 such entries and little memory left, each of six VMs walked one key, and each a different one.
- **`tasks.fanout` and a Pack prelude panic with little memory left.** Both held a Lua reference to each keyed member or global, and `mlua` panics when the stack of those references cannot grow.
  That stack grows by half its size or more at a time, so 50,000 keyed members panicked with 512 KiB or less left to the VM.
- **The short form of `tools.adhoc` parameters is built without a budget.** A description that 128 parameters share is copied 128 times, so a table with one description of 1 MiB costs 128 MiB outside the VM's memory, on `master` too.
  A check that reads every entry to find the first refusal builds the schema of every good entry on the way, so a table to refuse costs the same.
- **A read of a view copies its label.** A view whose data only a function can read carries a label, and each read copied it.
  2,048 parameters behind one view with a label of 4 MiB made 8 GiB of copies, and `json.encode` in debug mode made 256 MiB of text from 256 views with one label of 1 MiB.
- **The short form follows views without a count.** A parameter's value behind 10,000 views, used by 1,000 parameters, made one Rust call of 10,000,000 steps, which no cancel interrupts.
- **A table with a gap walks in two ways.** `pairs`, `next`, and `tasks.fanout` split a table at `#t`, and `tools.offer` measured its array by it.
  For a table with a gap in its indices, `#t` is any index before a gap, and when the indices sit in the hash part Lua picks one with the seed of the state (`hash_search` in `ltable.c`), which `mlua` draws anew for each state.
  `{ [1] = 1, [2] = 2, [3] = 3, [5] = 5, [false] = false }` gave `#t` 3 or 5, and `pairs` gave `1,2,3,false,5` or `1,2,3,5,false`, over 32 VMs on an earlier build of this change, and on `master` as your review measured.
  `tasks.fanout` over such a table gave three members and two pairs, or a list that read back as three members, and `tools.offer` refused it in two texts.

## The rule

- **`math.random`.** Each section VM's generator is seeded from the run's seed and the section entry's id.
  `math.randomseed` with integers seeds as stock Lua does, and with no argument it seeds from the generator's own next two draws.
- **`table.sort`.** A stable merge sort in Lua.
  It reads every element once, compares, and writes every element once, so a sort whose comparison fails leaves the list as it was.
- **`tostring` and `string.format`.** A value that stock Lua prints with an address prints as its name alone: its type name or the `__name` of its metatable.
  A `__name` counts up to its first zero byte, as stock Lua prints it.
  `%p` raises an error.
  A failure that an Engine function raised prints as its message, the text `pcall` gives for it, where a `__close` handler gets it before `pcall` does.
- **Failure text.** A failure text that ends in `: <address>`, before a traceback or at its end, loses the address.
  An address has at most as many hex digits as a pointer, 16 on a 64-bit target, and a longer number stays.
  With a plain name in front and nothing else, it reads `error object is a <name> value`.
  With anything else in front, `(address)` takes the place of the address.
- **`pairs` and `next`.** Both raise an error for a table that has a table, function, userdata, or thread as a key, and `next` raises for such a key to go on from.
  A string key that is not valid UTF-8 sorts by its bytes, and an infinite number key by its value.
  The refusal names the type of such a key, and of several types the first by name.
- **The order of a walk.** `pairs` and `next` visit the integer keys from 1 up by value, then the other keys in the shared order: booleans, then the other numbers, then strings.
  The place of a key is the key's own, and `#t` has no part in it.
  A table without a gap walks as before.
- **`next`.** The answer is the least key after the one given, and the position of a key is exact, so an integer and a float that differ are two keys.
  A float with the value of an integer is that integer's key, as in stock Lua.
  One step reads every key of the table and keeps one candidate.
- **`pairs`.** `pairs` writes the table's own keys to a Lua list, sorts their places in Rust as plain data, and moves each key of the list to its slot.
  It hands the list to an iterator that is a Lua function, so all that a walk keeps is in Lua's heap, and no string key is copied there.
- **A refusal for one of several entries.** The check reads every entry and keeps the refusal of the entry whose key comes first: booleans, then numbers, then strings by their bytes, then any other key by the name of its type.
  This holds for the options of `models.use`, the short form of the parameters of `tools.adhoc`, the keys outside the array of `tools.offer`, the keyed part of a `tasks.fanout` collection, and the globals of a Pack prelude.
- **A weak table.** `pairs` and `next` refuse a table whose metatable has a `__mode` string with `k` or `v` in it, at each call.
  The table still reads and writes by key.
  `tasks.fanout` refuses such a table as its collection, and the prelude install refuses a prelude that made its environment one.
- **Each entry stays in Lua's heap.** `tasks.fanout` writes each keyed member's pair table to its list of members as Lua's own walk meets it, and sorts the list as `pairs` sorts its keys.
  The prelude install keeps the names of the globals and reads each value by its name.
  Neither has a limit on the count.
- **The list of a collection.** `tasks.fanout` takes as the list the indices from 1 up to the first that holds nothing, and every other entry, an index past a gap included, as a keyed member.
- **The array of an offer.** `tools.offer` ends its array at the first nil that the read meets, and refuses a raw key outside it before it refuses a `#t` past it.
  A table with a gap is then refused for the same key whatever `#t` gave, when the read meets a nil at the first gap.
  A table with no `__len` whose `__index` gives a value at the gap is still measured by `#t`, as under "Left alone".
- **One budget for the short form.** One conversion may read as many parameters as one eighth of the VM's memory limit in bytes, with each view on the way to a parameter's value counted as six, and as many bytes of names, types, and descriptions as that limit.
  Each string is counted before it is read, a table past the budget is refused with `tools.adhoc: parameters are too large to convert`, and no string of a schema is copied after the first refusal.
  A refused entry copies its own name, and a type it does not know, into its message, and both are counted first.
  What an entry costs does not depend on the entries read before it, so a table is too large in every order of Lua's own walk or in none.
- **A view's label is copied only where it is printed.** A view whose data only a function can read carries a label, a string of any length that many views can share.
  The read of a view hands out the label as the view's own string, with no copy.
  `json.encode` and the Markdown renderer print it, in an error or in a placeholder of debug mode, and count its bytes before they copy it.
  `tools.adhoc` and the conversion of a view to a plain table do not print it, and do not copy it.
- **Parameter names.** A key of the short form that is not a string is refused.
  A parameter's type and description are strings read raw from its value, or from the data of a sealed view, so no metamethod runs in the order of Lua's own walk.

## What this changes for an author

- A set of objects, `seen[object] = true`, still works as a lookup, and a walk over it raises an error.
  A block that must walk the objects keeps them in a list as well, or gives the table a `__pairs`.
- Two tables print the same text, so code that must tell them apart compares them with `==`.
- A waiting call such as `tools.call` works inside a `table.sort` comparison, because the sort is no C function.
- `table.sort` takes a table only, where stock Lua also takes a value with the `__index`, `__newindex`, and `__len` metamethods.
- `table.sort` always writes every element, so a sorted read-only proxy fails where stock Lua writes nothing.
- `table.sort` of a table whose `__len` cannot be called fails with `attempt to call a boolean value (metamethod 'len')` on the line of the call, where stock Lua names no metamethod and no line.
- `%s` with a width pads the name alone, where stock Lua pads the name with its address, which is longer than most widths.
- An error string that ends in `: ` and a lowercase hex number with `0x` in front and at most 16 digits loses the number.
  `error('size: 0x1f', 0)` reads `error object is a size value`, and `error('bad byte: 0x1f')` ends in `bad byte: (address)`.
  On Windows the number is uppercase hex digits at the pointer's width with no `0x`, and those two strings stay.
- `tools.adhoc` with `parameters = { [1] = 'string' }` raises an error, where it made a parameter named `1`.
- `pairs` and `next` over a weak table raise an error, where they walked what the collector had left.
- `tasks.fanout` over a weak table raises an error, where `master` spawns an arm for each member it holds.
- Of several bad options of `models.use` with keys that are not strings, the error names the type of the first key in the shared order, where it named the first type by name.
- `tools.adhoc` with a parameter whose description is a number raises an error, where the number became the description's text.
- `pairs` and `next` over a table with a gap in its indices visit every integer key from 1 up before a boolean key, where the keys past `#t` came after it, and `#t` differed between two runs.
- A loop of `next` that clears a key it has passed goes on from that key's place, where a cleared integer key with no list before it sent the walk back to the first boolean.
- `tasks.fanout` over `{ 'a', nil, 'c' }` spawns an arm for `a` and one for the pair of the key 3, where it spawned by `#t`.
- `tools.adhoc` with a short form whose text is larger than the VM's memory limit raises an error, where it built the schema.
  A table of 128 parameters with one description of 1 MiB is refused in 1.4 ms.
- A walk by `pairs` costs more time, in a release build, the best of three runs: 365 ms against 228 ms for 40,000 walks of 20 string keys, 345 ms against 251 ms for 400 walks of 2000 string keys, and 132 ms against 89 ms for 400 walks of a list of 2000.
  That is at most 0.17 microseconds more for each key.
  A loop of `next` over string keys costs less, 2.13 s against 3.34 s for three loops over 2000, and over a list a little more, 0.66 s against 0.62 s for three loops over 2000.

## Settled with you

- Replace the functions with deterministic ones; do not remove them.
- `pairs` and `next` refuse a reference key, although that breaks the walk over a set of objects.
- The failure text is fixed by one rule on the text, with the false positive on a string of the same shape accepted.
- Both of those go in this pull request, not a later one.
- Comments are written in this pull request wherever they help.

## Decisions I made where you left room

- **The seed is a separate step.** `SectionVm::new` seeds the generator with a fixed value, and `SectionVm::seed_random` gives it the run's.
  A caller that forgot the second step would give every entry the same draws, which the runtime test `math_random_draws_from_the_runs_seed_and_the_section_entrys_id` guards.
- **`math.randomseed()` reseeds; it is not refused.** Code written for stock Lua calls it, and the draws after it still follow from the run's seed.
- **Keys that are not UTF-8 or not finite get a place.** They have a value to sort by, and refusing them would break tables that `json.encode` already orders.
- **The text rule is exact on the address and takes any name.** It takes `0x` and lowercase hex of at most the pointer's width, and on Windows uppercase hex at the pointer's width.
  A `__name` is any string, so the rule looks before every traceback heading in the text and at its end.
  The address stays only on a platform that prints another shape.
- **`%s` asks for an argument's text when the stock function reaches it.** A stand-in with a `__tostring` takes the place of each table, function, userdata, or thread argument.
  A look at every argument before the call prints an address when a `__tostring` changes a later argument's metatable.
- **The option check of `tasks.spawn` and the waits walks with `pairs`, as on `master`.** A table, function, userdata, or thread key in the options is therefore refused in the words of `pairs`, on the line of the call.
- **The iterator is Lua, not a Rust closure with fewer references.** A closure that held the keys' data and one slot for its table still panicked: 4,000,000 walks beside a 40 MB list failed at 278,528 slots, because the slot stack cannot grow near the memory limit.
- **The list holds the table's own keys.** A string key made anew from its bytes costs the key's length again from the VM's memory.
  A table with one key of 40 MiB then fails both walks with `not enough memory`, where `master` walks it.
  The keys of the hash part now go to the list as Lua's own walk meets them, and `arrange` moves each to its slot along the cycles of the sort's permutation, with two keys in Rust at a time.
- **`next` looks for the least key and sorts nothing.** A sorted list for each step would be made in Lua's heap at each step.
  The least key after the given one is the same answer, and a step needs one candidate.
- **The refusal is kept, the entries are not.** A sort of the entries before the check holds a reference to each, which is the panic above.
  `FirstRefusal` holds one refusal and one key's position.
- **Two keys with no place and of one type tie.** The refusal met first stays, so each check words the refusal of such a key by its type alone.
- **A number is refused as a parameter's name, not made a string.** Making it a string keeps the collision of `1` and `'1'`.
- **A weak table is refused, not walked by a fixed rule.** A collection can run at any allocation of the walk, also when the collector is stopped, since Lua still collects when an allocation fails.
  So no walk can read a weak table as it was when the walk began.
- **The weak-table check is made at each call, not in `setmetatable`.** Lua reads `__mode` each time it collects, so a table is weak from the moment its metatable gets the field, which can be after `setmetatable`.
- **A walk of `pairs` that began before the table became weak goes on.** Its list of keys is made by then, and each step reads the table by key, as an author's `t[key]` does.
- **No limit on keyed members or prelude globals.** A limit of 100,000 for each bounds the references held and does not stop the panic with little memory left.
  The code holds no reference to bound.
- **A walk that holds no reference refuses a weak table.** Such a walk writes to Lua's heap as it reads, and the collector can run at each write and take the entries not read yet.
  `tasks.fanout` over a weak table of 20,000 members that nothing else used gave 14,479 members, a different set in each VM, where the code that held every member gave them all.
  A prelude that made its environment weak had 1,132 of its 2,000 globals installed.
  `is_weak` in `order.rs` is the one test of a weak table, and each of the four walks words its own refusal.
- **The place of a key is the key's own, not a place in a list that ends at the first gap.** With a list that ends at the first gap, a loop of `next` that clears the key 1 moves every later index behind the booleans, and the walk then ends before them; that order was reasoned and not built.
  An order of the keys alone has no such case, and `next` needs no length.
  Its cost is that an integer key past a gap now comes before a boolean and before a number below 1, where it came after them.
- **The list of `tasks.fanout` does end at the first gap.** A member of the list is the arm's item as itself, and a keyed member is a pair, so the collection needs one end of the list, and the first gap is the one end that every run agrees on.
  `listed` in `order.rs` counts it with one read for each item.
- **`#t` itself is left as Lua has it.** It is an operator, so the Engine cannot put its own function in its place, and `mlua` takes no seed for a new state.
  The page `prompt-language.md` states it as a limit.
- **The budget of the short form is that of `json.encode`, with a view at six values.** It is sized from the VM's memory limit, so only a schema larger than the table's own heap is refused.
  A view is a table of its own, 48 bytes of the Lua heap on a 64-bit machine, which is six values of 8 bytes, so views that are passed once each and fit the heap stay inside the budget.
  With a view at one value, as `json.encode` counts it, a value behind 10,000 views under 1,000 parameters is refused only after 8,388,608 views: 16.3 s in a debug build and 1.7 s in a release build.
  At six it is refused after 1,398,101 views, in 3.1 s in a debug build and 0.46 s in a release build, and that is the longest one conversion can read views at the default limit.
  The text of one schema is at most 64 MiB at the default limit.
- **The label stays a Lua string.** With a copy at each read, 2,048 parameters behind one view with a label of 4 MiB take 2.53 s in a debug build before the refusal, and with the label left in Lua 7.1 ms.
  Counting the label in the read of a view is the other way, and it refuses a table for a text that the caller throws away.
  In debug mode `json.encode` puts the label in a placeholder for each such view, so the label counts against the bytes of the value, once for each placeholder, as a repeated string does.
- **A refusal waits, a spent budget does not.** A refusal is kept to the end so that the one with the least key wins.
  A spent budget ends the read at once, which is the same in every order because the sum of the costs is.
- **`pairs` and `next` install inside `harden`.** The iterator chunk captures `rawget`, which `harden` removes.

## Code

All in `runvil-harness`, on the branch `kb/lua-fixes`: this plan in the first commit, one commit for each change below in this order, and the pages in the last.
Neither `public-api.txt` listing changes.

1. **`math.random`.**
   - `runvil-types`, `seed.rs`: `LuaRandom`, derived from the `RunSeed` with SHA-256 under `runvil.lua-random.v1`, and `entry_seed`, which gives the two integers for one section entry.
   - `runvil-runtime`, `execute/context.rs` and `execute/section_context.rs`: the run holds one `LuaRandom`, and `limited_vm` seeds each VM it builds.
   - `runvil-lua`, new `__impl_stdlib.lua` and `stdlib.rs`: the replacement of `math.randomseed`, and `from_here`, which moves a failure that Lua raised in the chunk to the author's line and passes every other failure on as it is.
   - `runvil-lua`, `vm.rs` and `hardening.rs`: `SectionVm::seed_random`, and `harden` ends with `stdlib::install`.
2. **`table.sort`.** `__impl_stdlib.lua`: the merge sort, with its reads, writes, and default comparison under a raw `pcall`.
3. **`tostring` and `string.format`.** `__impl_stdlib.lua` and `stdlib.rs`: the replacements of `tostring`, `string.format`, and `table.concat`, whose inline chunk leaves `hardening.rs`.
   The Rust side is `unaddressed`, which reads the `__name` and `__tostring` of a userdata's metatable, as Lua code cannot, and cuts the name at its first zero byte.
   `string.format` hands the stock function a stand-in for each argument that can print with an address.
   `error_value.rs`: `failure_message` is the text a Rust callback's failure prints as.
4. **The text of a failure.** `error.rs`: `without_address` is the text rule, `lua_text` applies it to an `mlua` failure's text, and `root_text` applies it to what Lua raised.
   `make_lua_error`, `program.rs`, `error_value.rs`, and `prelude.rs` take their text from these.
5. **`pairs` and `next`.**
   - `order.rs`: `position` places any boolean, number, or string key, and `compare_places`, `FirstRefusal`, `is_weak`, `listed`, and `sort_list`, which sorts a Lua list by the places of its items with two items in Rust at a time.
     `json/keys.rs` uses `position` and `compare_places` in place of its own copies.
   - `iteration.rs` and new `__impl_iteration.lua`: `WalkPlace` is the place of a key, `walk_list` makes the list of a walk and names the first type of a key with no place, `refuse_weak` refuses a weak table, `deterministic_next` finds the least key after `previous_place`, `integer_of` reads a float with an integer's value, and `pairs` calls the Lua factory of the iterator.
   - `hardening.rs` and `vm.rs`: `harden` installs the walk right after the `_G` guard.
6. **`tasks.fanout`.** `collection.rs` and `__impl_fanout.lua`: `keyed_member`, the least refusal, the list counted by `listed`, and the keyed members sorted by `sort_list`.
7. **`tools.offer`.** `tools/offer.rs`: `array` ends at the first nil and hands the keys outside it to `FirstRefusal`.
8. **`models.use`.** `models.rs`: `set_option` reads one option, so `option_check_order` is gone.
9. **A Pack prelude.** `prelude.rs`: `defined_globals` returns the names of the globals, and the install reads each value by its name.
10. **A view's label.** `proxy.rs`, `json/encode.rs`, and `markdown/walk.rs`: `Unsealed::Unreadable` holds the label as a Lua string, and `label_text` and the renderer's refusal count its bytes and then copy it.
11. **`tools.adhoc`.** `tools/decode.rs`, `proxy.rs`, and `walk_budget.rs`: `adhoc_params_schema` takes the VM and spends one `WalkBudget`, `adhoc_param` refuses a key that is not a string, `type_and_description` reads a parameter's value raw, `AdhocParam` holds a parameter's own strings until its schema is made, `view_data_within` counts the views of a read, and `charge_view` is the cost of one.

## Documents

- `decisions/prompt-language.md`: three new entries, on `math.random`, `table.sort`, and `tostring` with `string.format`.
  The entry on `pairs` and `next` states the order of the keys, the refusal of a key with no place and of a weak table, and why the order does not use the length.
  Two more entries are new: the length of a table with a gap in its list differs between runs, and an error for one entry of a table names the entry whose key comes first.
  The entry on a call that waits names `string.gsub` where it named `table.sort`.
- `decisions/tasks-and-fanout.md`: the list of a collection ends at the first gap, a weak collection is refused, and the determinism entry names the new rules and puts the length of a table with a gap outside them.
- `decisions/tools-and-packs.md`: the short form of the parameters of `tools.adhoc` takes string names only, runs no metamethod, and converts within the limits of its budget.
- `decisions/engine.md`: the seed entry names the `math.random` seed.
- The guide in `runvil-docs` is not changed here.

## Tests

- `seed.rs`: a known answer for the derived seed.
- `run_inputs.rs`: draws repeat with the same seed and differ by seed and by section entry.
- `tests/sandbox.rs`:
  - the draws of a VM come only from its seed;
  - the sort is stable, orders by `<` or by the comparison, and fails on the author's line in stock Lua's words;
  - `tostring` and `string.format` print no address;
  - 42 cases give the same answer in a section VM and in a stock `Lua::new()`, four of them with a `__tostring` that changes what a later `%s` argument prints as, one with a `__name` that holds a zero byte, and one with a `__tostring` of the string metatable that returns a table;
  - a raised value with no text fails the block without its address, including on the paths where `mlua` renders the value, under a `__name` that is empty or holds a `: `, and in a `__close` handler that prints the failure with `tostring`, `%s`, and `table.concat`.
- `error-tests.rs`: the texts the rule changes and leaves, the Windows shape, a name that holds a traceback heading, an address of the pointer's width, and a number one digit longer among them.
- `tests/args.rs`: two tests print a failure that a raw `pcall` caught, and now expect its message, which starts with the prompt's file and line.
- `args_surface-chains.rs`: one case, a `tasks.spawn` whose options hold two table keys.
- `iteration-tests.rs`: the refusals of `pairs` and `next` with their exact text, the type named for a table with a function key and a table key, and the place of infinite and non-UTF-8 keys.
  The two tests of a walk over cleared table keys are removed with the behavior.
- `iteration-tests.rs`: `pairs` and `next` refuse a table with the `__mode` `k`, `v`, or `kv`, also when the field is set after `setmetatable`, and the table reads by key; `next` ends over `math.maxinteger` and `0x1p63`; `next` takes a float with an integer's value; 150 walks of 2000 string keys beside a string that fills the heap to 4 MiB under the limit; a key of 40 MiB is walked by `next` and by `pairs`; a loop of `next` and a loop of `pairs` give the same order over 300 keys of every kind; the chunk name resolves.
- `models-tests.rs`: an options table of 1,200,000 entries is refused with an error.
- `tools/tests.rs`, `tools/tests-offering.rs`, `collection.rs`, `prelude-tests.rs`: of several bad entries, the refusal is the same over 8 to 16 fresh states, each with its own hash seed.
- `tools/tests.rs`: a parameter with an `__index` that counts its calls is refused, and the count stays 0.
- `collection.rs`, `prelude-tests.rs`: 50,000 keyed members with 256 KiB left end with `not enough memory`; the names of 50,000 globals are read with 64 KiB left.
- `collection.rs`, `prelude-tests.rs`: a weak collection and a prelude with a weak environment are refused, each with its exact text.
  Both inputs gave a result on a build without the two refusals, as probes.
- `iteration-tests.rs`, `collection.rs`, `tools/tests-offering.rs`: the table with a gap above gives one walk, one list of members, and one refusal over 16 to 32 fresh VMs.
  Each gave two results on an earlier build of this change, as probes.
- `iteration-tests.rs`: a loop of `next` that clears the key it is at, or one before it, meets every key once; this takes the place of the test of the replay.
  The order of the test of mixed keys, of the infinite numbers, and of `math.maxinteger` is the new one.
- `tools/tests.rs`: with a memory limit of 64 KiB, eight parameters that share a description of 16 KiB, and 100 parameters behind the same 20 views, are too large in 8 states, with an entry to refuse beside them and without.
  Inputs of the same two kinds gave a schema on a build without the budget, as probes.
- `tests/json/limits.rs`: with a memory limit of 4 MiB, 32 views that share a label of 512 KiB are too large for `json.encode` in debug mode.
  Without the count the test ends with `not enough memory`.
  The time of the `tools.adhoc` input above was measured by a probe and has no test, because a test of a time is not a yes or no.

Of the tests of the walk and of the refusals, seven fail on the first build of this pull request, whose `pairs` and `next` refused a key with no place and held a Lua reference to each string key.
Four more fail on an earlier build of the walk: the weak-table test, the two tests with little memory left, which panic there, and the test of a parameter with an `__index`.
The input of the 40 MiB test failed on an earlier build too, as a probe.
The changed test of the option order of `models.use` and the new test of `next` with `2.0` for the key `2` pass on that build and guard the rewrite.
The test of the same order of the two walks was not run against earlier code.

Probes that are not tests, each run once on the final code: 1000 walks of 2000 string keys, 100,000 walks of 20, three walks of 600,000 with a `break`, 40 walks of 60,000 with a `break`, 4,000,000 walks and 4,000,000 bare `pairs` calls beside a 40 MB list, and a walk of a weak table of 20,000 entries, which is now refused.
All ended without a panic.
With no reference held to each entry: `tasks.fanout` over 50,000 keyed members, `pairs` over 50,000 string keys, and 30 steps of `next` over them, each with 0 to 3000 KiB left to the VM in ten steps, ended with a result or with `not enough memory`, and 300,000 keyed members came out in key order.
The weak table of the problem above is refused in three VMs.
A table of 600,000 short string keys is walked, where a walk that holds a reference to each key panics at that size.

## Verification

On the last commit of the branch:

- `cargo nextest run --locked --workspace --all-features`: 4080 run, 4079 passed.
  The one failure, `harness-test-plugin::it lifecycle::the_fixture_is_loaded_invoked_unloaded_once_and_closed`, fails on `master` too.
- `cargo clippy --workspace --all-targets --all-features` with `CARGO_BUILD_WARNINGS=deny`: clean.
- `cargo fmt --all --check`: clean.
- `cargo doc --workspace --no-deps --all-features` and `cargo doc -p runvil-engine --no-deps`, both with `RUSTDOCFLAGS="-D warnings"`: clean.
- `cargo +nightly-2026-09-05 xtask api --check`: 0 violations for each facade, and each listing matches its `public-api.txt`.
- `cargo test -p build-xtask`: 204 passed.
- `node --test tools/docs-claims.mjs`: 3 passed.

On each commit before it: `cargo clippy --workspace --all-targets --all-features` with `CARGO_BUILD_WARNINGS=deny`, `cargo fmt --all --check`, and the tests of `runvil-lua`, and of `runvil-runtime` where the commit changes it.

Not run: `cargo dylint`, which is not installed on this machine, a build on Rust 1.95, and any test on Windows.

## What the build found

- **A C function that Lua tail-calls keeps the caller's frame.** `return base_tostring(v)` therefore put the chunk's position on the failure, which is why every stock function is called from a chunk function under `pcall`.
- **Stock Lua's sort puts no position on a failure of the comparison.** The replacement puts the call's line on it.
- **A replacement that the author tail-calls loses the author's frame.** `return string.format(...)` places a failure on the caller's line, as the Engine's other Lua replacements do.
- **A method call counts its arguments differently.** `('%d'):format('x')` reports argument `#2`, where stock Lua reports `#1`.
- **`mlua` renders a raised value before the Engine sees it.** When Rust runs the author's Lua and it raises a table, `mlua` keeps only the text `table: 0x...`, and it offers no hook.
  That is why the failure text is fixed by a rule on the text.
  A first version that looked at the raised value in `coro.rs` covered one path only, and it is gone.
- **A probe of 4,531 calls** passed values whose metamethods raise a table to every Engine global: 25 failures held an address before the text rule, and none after.
  46 of the calls stopped at a waiting call that the test driver cannot answer.

Costs, in a release build, over stock Lua:

| Call                                    | Stock | Replacement                     |
| --------------------------------------- | ----- | ------------------------------- |
| `tostring` of a number or a table       | 1×    | 1.7× to 1.9×, about 0.1 µs more |
| `tostring` of a value with `__tostring` | 1×    | 13×, about 0.43 µs more         |
| `string.format('%d:%s', ...)`           | 1×    | 5.9×, about 0.9 µs more         |
| `table.sort` of 100,000 integers        | 25 ms | 91 ms                           |

The sort holds 18 more bytes for each element while it runs.
Under a 64 MiB limit it sorts 2,000,000 integers and fails on 3,000,000 with `not enough memory`.

Of the walks:

- **A closure with fewer references still panics.** A Rust iterator that held the keys' data and one slot for its table failed at 278,528 slots in 4,000,000 walks beside a 40 MB list.
  That is why the iterator is a Lua function and a walk keeps nothing in Rust.
- **`mlua`'s slot stack grows by half its size or more at a time.** A count of references that fits the stack can still panic with little memory left, so a limit on the count is no fix.
- **A walk that holds no reference lets the collector run between two reads.** That is why each such walk refuses a weak table.
- **`#t` of a table with a gap follows the hash seed of the state.** `mlua` draws the seed anew for each state and takes none from its caller, so the Engine cannot fix the length and keeps it out of the order of a walk.
- **The lines of `ltable.c` named above were read in Lua 5.5.0,** the release in the build.

### The review

This part is later than the rest of this file.
It records the rounds of your review of the branch and the changes that followed each.

**The first round.** It reproduced five faults in the first build.
Two of them were in code that issue #22 does not need, and that code is cut.

- **Cut: the shared-order walk of four checks.** The first build made `tools.offer`, `tasks.fanout`, `tools.adhoc`, and the check of a Pack prelude's globals read a table's entries in sorted order, so that of several bad entries the same one was refused in every process.
  The walk held a reference to every key and value, and a prelude that set 1,200,000 globals ran `mlua` out of reference slots and panicked.
  The four checks walk as on `master` again.
  Cost: of several bad entries in one call, the one that the refusal names can differ between processes, as on `master`.
  In twelve processes one `tools.offer` call gave five texts, one `tools.adhoc` call five, and one `tasks.fanout` call three.
  The fix is a check that holds one refusal and no entry, which `FirstRefusal` became after this round.
- **Cut: the Rust key reader of the task option check.** It left two keys of one type in address order and printed them with `tostring`.
  The check walks with the Engine's `pairs` again, which refuses such a table before the check reads a key.
- **Fixed: `string.format` printed an address** when a `__tostring` cleared the metatable of a later `%s` argument, and printed a name where stock Lua runs a `__tostring` that an earlier one installed.
  The stand-in removes the early look at the arguments.
- **Fixed: `pairs` and `next` named the first bad key that Lua's own walk met,** a function key in 13 of 32 processes and a table key in 19.
  After the fix, 24 processes named the function key.
- **Fixed: the text rule left the address** after a `__name` that is empty or holds a `: ` or a line break.
  A `__name` that holds a traceback heading hid the address behind it, which the rule now finds too.

**The second round.** It found the walk by `next` that never ends over `math.maxinteger` and `0x1p63`, which is on `master` too.
`next` now takes the least key after the one it was given, by the key's exact position.

**The third round.** It found two more faults.

- **Fixed: a `__name` that holds a zero byte failed `%s` with a width.** `string.format('%10s', value)` raised `string contains zeros`, where stock Lua prints the name up to the zero byte.
  `unaddressed` now cuts the name there.
- **Fixed: two failures named the Engine's chunk.** `tostring` of a string called the stock function outside a protected call, and `table.sort` read the length of its list outside one.
  A `__tostring` of the string metatable that returns a table, and a `__len` that is `false`, then failed with `__impl_stdlib.lua` and a line in front.
  Both now run under `pcall`, and so does the look of `string.format` at its arguments, which the round did not name.
  The three changed tests fail on the code before the fix.

**The round on the first build of the walk without references.** That walk made each string key anew from its bytes, and a table with one key of 40 MiB failed both walks.
The list of a walk now holds the table's own keys.

**The fourth round.** In the replacements it found one fault and one unbounded rule.

- **Fixed: `tostring` of a failure in a `__close` handler printed an address.** Lua calls the handler with the failure as `mlua` raised it, a userdata, before the `pcall` around the block returns, so the `pcall` replacement comes too late.
  `mlua` prints that userdata as its own text for the failure, which holds `table: 0x...` when a metamethod raised a table inside an Engine function.
  `unaddressed` now gives such a userdata the message of the error table that `pcall` makes of it, and `tostring`, `%s`, and `table.concat` all print through it.
  `getmetatable` of the userdata gives `false`, so its own `__tostring` is out of an author's reach.
- **Fixed: the text rule took a hex number of any length for an address.** A failure text that ended in `: 0x` and 4,096 hex digits lost them all.
  The rule now takes at most the digits of a pointer, so the most a text loses is one number of 16 digits.

A `__tostring` nested in `string.format` nested in a `__tostring`, and so on, reaches 97 levels, and `tostring` 98, where stock Lua reaches 197.
Each replacement runs its stock function under `pcall`, which takes one of Lua's 200 C calls for each level.
The fourth round counted 99 levels and 190, and a probe after it 98 and 198: the replacement reaches half of stock Lua's depth by each count.
Past it the call fails with `C stack overflow`, the same failure in every run.
The cost stays: without the `pcall`, a failure that Lua raises in the chunk names the Engine's file, which the third round refused.

In the walks the fourth round found that a walk of a weak table gives a different result in each VM, and that `tasks.fanout` and a Pack prelude panic with little memory left.
`pairs` and `next` refuse a weak table, and the limit of 100,000 keyed members and globals went with the references it bounded.

**The fifth round.** With no reference held, `tasks.fanout` over a weak table gave 14,479 of 20,000 members, and a prelude with a weak environment had 1,132 of its 2,000 globals installed.
Both now refuse such a table.

**The sixth round.** It found three faults.
A refused short form of `tools.adhoc` parameters cost 128 MiB outside the VM's memory, and a value behind 10,000 views under 1,000 parameters took 23 s by the round's measure, so the short form now converts within one budget.
A table with a gap in its indices walked in two ways, so the order of a walk, the list of a collection, and the array of an offer no longer use `#t`.

**The seventh round.** It found two faults.
A read of a view copied its label, which the budget did not count.
The label now stays a Lua string, and a reader that prints it counts its bytes first.
The sandbox test expected the Linux and macOS shape of an address on every platform.
The test now builds its string that looks like an address in the shape of the platform that runs it.

**The eighth round.** `tools.offer` still measures by `#t` a table with no `__len` whose `__index` gives a value at a gap.
That is stated in the code, in the page, and under "Left alone", and is not fixed.

## Left alone

Known:

- **The text rule changes a string that only looks like a rendering.** Every failure text that ends in `: 0x` and 1 to 16 lowercase hex digits, before the traceback or at the end, loses the number, whoever wrote it: the author, a Pack, or Lua.
  The loss is that one number, at most 16 digits, at each such place.
  On Windows the shape is `: ` and uppercase hex digits at the pointer's width.
  The text in front stays, except that a plain name alone becomes `error object is a <name> value`.
- **`mlua` panics for any new reference near the memory limit.** The slot stack is a Lua stack, it grows by half its size or more at a time, and it cannot grow when the VM has less memory left than that.
  A check that holds up to 100,000 references, such as the JSON encoder's key collection or the array of `tools.offer`, can still meet that in a VM that is almost out of memory, as on `master`.
  The array part of a `tasks.fanout` collection and every other Engine function hold a few references at a time, and a few can still be one too many.
  The cause is in `mlua`, and a fix is a change of its own.
- **`#t` of a table with a gap, and what is measured by it.** `#t`, `table.insert`, `table.remove`, `table.concat`, `table.unpack`, and `table.sort` give such a table a length that differs between two runs, as on `master`.
  So does `mlua`'s conversion of a table to data, which the arguments of `tools.call`, the `item` of a spawn, a message's value, and a global in prose go through: `{ [1] = 1, [2] = 2, [3] = 3, [5] = 5 }` became `[1,2,3]` or `[1,2,3,null,5]` over 32 states.
  A fix of the conversion needs a rule for a list with a gap in every one of those functions, and is a change of its own.
  The lists that the Engine itself measures by `#t`, in `var`, in a batch answer, and in the calls of a model reply, are made without a gap by the Engine's own code; that was read from their comments and not tried.
- **An offer whose `__index` fills a gap.** `tools.offer` reads its array through `__index` up to `#t`, and with no `__len` that `#t` is Lua's own.
  `{ [1] = a, [2] = a, [3] = a, [5] = a }` with an `__index` of `{ [4] = a }` was accepted in 30 of 64 fresh VMs and refused for the key 5 in 34.
  With a second gap, a key 7 beside those, it was refused for the key 5 in 34 and for the key 7 in 30.
  With a `__len` of 5 it was accepted in all 64, and with an `__index` that gives nothing at the gap it was refused for the key 5 in all 64.
  The code of `master` reads the same way and was not run.
  A fix needs a rule for where an array that is read through `__index` ends, and is a change of its own.
- **The time of one `tools.adhoc` conversion.** It is one Rust call with no cancel inside it, of up to 1,398,101 views at the default limit: 0.46 s in a release build and 3.1 s in a debug build on this machine.
  `json.encode` and the conversions that read views count a view as one value, so one call of theirs can pass 8,388,608, as on `master`.
- **The size of one refusal of `tools.adhoc`.** The message quotes the parameter's name and a type it does not know, with each control byte written as up to six characters, as on `master`.
  Both are counted once, so the copies of one conversion are at most seven times the VM's memory limit, 448 MiB at the default limit, and one message is at most six times the limit, 384 MiB.
  That bound is derived from the code and was not measured.
- **The schema's own overhead.** The budget counts the text of a schema and not the JSON objects around it, as `json.encode` does not.
  The count of parameters is bounded by the VM's memory and by the budget of values, and the bytes of one empty property were not measured.
- **A weak table that another Engine function reads.** `json.encode`, `models.use`, `tools.adhoc`, `tools.offer`, and each conversion of a table to data, a member of a fanout collection included, read a table in Rust.
  The option read of `models.use` and the short form of `tools.adhoc` make nothing in Lua's heap between two reads.
  `json.encode`, `tools.offer`, and the conversions were not read for it, and a weak table passed to any of them was not tried.
- **A weak table read by key.** What it holds follows the collector.
  One probe gave the same counts of live entries at 40 points in six VMs, which proves nothing about another block.
- **`models.use` on `master`.** The panic was measured on this branch, whose code for it is that of `master`, and not on `master` itself.

Not tested:

- **Windows.** The address shape that the text rule takes there was written from memory.
  The sandbox test raises real values, so it fails on a platform whose shape the rule does not know.
- **Paths after a wait.** A callback that raises a table after a waiting call, such as one in `models.loop`, goes through the same text functions, and no test runs it.
- **The rest of Lua's library.** About 60 calls of Engine globals with a table or a function as an argument printed no address.
  The library was not read in full for another clock or address.

Out of scope:

- The guide in `runvil-docs`.
- A replay of a recorded run.

## Git

- The branch `kb/lua-fixes` starts at `bdcadfbb`: this plan first, then one commit for each change under "Code", then the pages.
- Each message ends with `Plan: /plan/2026-11-19-1-deterministic-lua.plan.md`.
- The branch is on your fork and is the head of [pull request #56](https://github.com/cppalliance/runvil-harness/pull/56).
