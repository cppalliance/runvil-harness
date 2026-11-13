---
name: Return Ends The Chain
overview: Any `return` at a block's top level ends the chain it runs in, and only a block that runs off its end lets the next block run. Every block is compiled with a trailer that tells the two apart, `require` follows the same rule, and the design pages and the guide are brought in line.
todos:
  - id: plan-copy
    content: "runvil-harness: on `master`, copy this plan to `plan/2026-11-13-1-return-ends-the-chain.plan.md` before any other change"
    status: completed
  - id: lua-crate
    content: "runvil-lua: compile each block plain then wrapped, add the marker and `LuaBlockResult::RanOff`, pass the marker from the guard, the `require` loader, and the test path; compile tests first, then the result and `require` tests"
    status: completed
  - id: runtime-crate
    content: "runvil-runtime: `settle_block` and `apply_settled` end the chain on any return, the H1 pass included; walk, H1, `call`, task, child-walk, and event tests; comments"
    status: completed
  - id: verify
    content: "runvil-harness: run every check in AGENTS.md"
    status: completed
  - id: design-docs
    content: "runvil-design: bring the decision pages and the language spec in line, one commit"
    status: completed
  - id: guide
    content: "runvil-docs: bring chapters 3, 4, and 8 and the smaller mentions in line, with example values from the built code, one commit"
    status: completed
  - id: commit
    content: "runvil-harness: mark the plan copy's todos done, record in it what the build found, and make the one commit on `master` with the `Plan:` trailer"
    status: completed
isProject: false
---

# A return always ends the chain

## The bug

`scalar_return` in [section.rs](../crates/runvil-internal/lua/src/section.rs) turns three different endings into one `None`: a block that runs off its end, a bare `return`, and `return nil`. `None` means "run the next block", so `return x` ends the chain or not depending on what `x` holds.

## The rule

- **A block that runs off its end** lets the section's next block run, as today.
- **A block that executes a `return`** ends the chain it runs in. That covers `return x` whatever `x` holds, `return nil`, a bare `return`, and `return f()` when `f` gives no value.
  - A string, number, or boolean is the chain's result as text, as today.
  - Nil or no value ends the chain with no text, the same way a chain that runs past its last section ends today: the run's result is `done`, a `call` gets `""`, and a task or fanout arm has the text `""`.
  - A table still fails with `cannot return a table as a result`.
- **A `return` inside a function** belongs to that function and changes nothing.
- **The H1 pass** follows the rule: any `return` in an H1 block ends the run. Today only a scalar does.
- **`require`** follows it too: any `return` in a library block ends the load. `require` gives that value, or `true` for nil or no value, which is standard Lua's rule. A library whose blocks all run off their ends still gives `true`.

Decisions I made where you left room:

- **A return with no value ends the chain like running past the last section.** No new kind of result is added (high confidence - `finish` already handles a chain that ends with no text).
- **`call` still gives `""` for a child that ends with no text.** After this fix `return call(...)` ends the chain either way, so a switch to nil is safe, but it is its own decision and I left it out (high confidence - the fix does not need it).
- **`tasks.reply(nil)` and an adhoc handler that returns nil still give `""`.** Those are function results, not block returns, and they keep `scalar_return`.

## How the Engine tells a return from running off the end

Lua gives back zero values for both a bare `return` and running off the end, so the difference is put in at compile time, in `LuaProgram::compile` in [program.rs](../crates/runvil-internal/lua/src/program.rs):

```rust
// compiled first, only to check the source; a failure here is the one reported
format!("{FRAME_GUARD}{source}")
// compiled second; this is the bytecode that runs
format!("{FRAME_GUARD}do {source}\nend return ...")
```

- **The marker.** The Engine calls every block with one argument, a private light userdata. A block that runs off its end reaches the trailer and returns it. Any `return` the author wrote happens before the trailer.
- **Why `do ... end`.** Lua wants `return` to be the last statement of its block, so nothing can follow a source that ends in `return`. Inside `do ... end` the author's `return` is still last.
- **Why two compiles.** The plain compile keeps every diagnostic as it is today, such as `unexpected symbol near <eof>`. It also refuses a source like `end do`, which would compile if only the wrapped form were tried. Both compiles share one compiler VM.
- **No line moves.** The opener sits on the block's first line beside the frame guard, and the trailer on a line of its own after the source, because a source can end in a line comment. The frame guard stays, so an error in `return f()` keeps its line.
- **One side effect.** `...` at a block's top level was empty and now holds the marker. A block that returns it acts as if it ran off its end. I would not guard against it (medium confidence - no prompt reads `...`, and the marker gives access to nothing).
- **Not run yet.** No Lua interpreter is installed here, so the two grammar facts above come from reading. The compile tests are written first, because the rest stands on them.

## What this changes for an author

- `return nil`, a bare `return`, and `return x` with a nil `x` used to move on to the next block. They now end the chain.
- An early exit such as `if not ready then return end` now ends the chain. To skip the rest of a block and keep going, put the rest inside the `if`.
- `return models.loop()` and a return of any other Engine call that gives no value, such as `store.write`, end the chain with no text.
- A called section, task, or arm that reaches such a return stops there. Today it falls through into the sections after it.
- No existing prompt changes behavior. Across the repositories in this workspace there are 32 prompt files, and none of their 41 top-level returns can give nil or no value. The Workshop's `chat.md` has no top-level return. One guide example changes, `block_returns` in chapter 3.

## The plan copy

- First thing, on `master` of `runvil-harness`, this plan is copied to `plan/2026-11-13-1-return-ends-the-chain.plan.md`, named as that directory's other plans are.
- The copy is part of the one commit. Before the commit its todos are marked done and it gains a section on what the build found.
- The commit message ends with `Plan: /plan/2026-11-13-1-return-ends-the-chain.plan.md`.

## Code: one commit in `runvil-harness`

The two crates change together: `step.rs` matches `LuaBlockResult` in full, so the workspace does not build with only one of them done.

`runvil-lua`:

- [program.rs](../crates/runvil-internal/lua/src/program.rs): the two compiles, the marker, and the docs of `FRAME_GUARD` and `compile`. `compile_internal` is unchanged, so the Engine's own chunks are not wrapped. A failure of the wrapped compile alone, which only Lua's nesting limit can cause, is reported the same way as any compile failure.
- [section.rs](../crates/runvil-internal/lua/src/section.rs): the result type below, and a new `block_result` that checks for the marker and otherwise uses `scalar_return`.

```rust
pub enum LuaBlockResult {
    /// The block ran off its end: the section's next block runs.
    RanOff,
    /// The block executed a `return`: its first value as text, or `None` for nil or no value.
    Returned(Option<String>),
    Jump(String, Option<NonZeroU32>),
}
```

- [vm/run.rs](../crates/runvil-internal/lua/src/vm/run.rs) and [__impl_coro.lua](../crates/runvil-internal/lua/src/__impl_coro.lua): `start_block_coro` resumes the guard with the block and the marker, the guard passes its extra arguments to the block with `raw_xpcall(block, guard_handler, ...)`, and `step_block_coro` uses `block_result`.
- [__impl_require.lua](../crates/runvil-internal/lua/src/__impl_require.lua) and [require.rs](../crates/runvil-internal/lua/src/require.rs): the loader gets the marker as a capture, and `run` becomes:

```lua
local value = blocks[index](ran_off)
if not rawequal(value, ran_off) then
  if value == nil then return true end
  return value
end
```

- [test_support.rs](../crates/runvil-internal/lua/src/test_support.rs): `run_loaded` passes the marker and uses `block_result`, so the test path follows the same rule.

`runvil-runtime`:

- [step.rs](../crates/runvil-internal/runtime/src/execute/scheduler/step.rs): `Settled::Returned` holds an `Option<String>`, and `RanOff` becomes `Settled::Advanced`. In `apply_settled` the H1 arm ends the run with the value or `done`, and the other arm becomes `self.finish(id, Ok(value))`. The H1 `var` read-back in `settle_block` runs for a return with no value too.
- [chain.rs](../crates/runvil-internal/runtime/src/execute/scheduler/chain.rs): no logic change. `finish` already turns `None` into `done` or `""`, reads `var` back, and reports the section as finished.
- Comments that say "scalar return" where any return is now meant, in `step.rs`, `chain.rs`, `h1.rs`, `chain_record.rs`, and `scheduler.rs`, and the doc of `AbandonReason::OwnerReturned` in [ids.rs](../crates/runvil-internal/types/src/ids.rs).

## Tests

`runvil-lua`:

- **The compile, first.** `end do` is refused with today's diagnostic. A block that is empty, holds only a comment, or ends in a line comment compiles and runs off. A block that ends in a label a `goto` reaches past a local compiles. An error on a block's last line maps to that line.
- **Each ending, by result.** One table over `start_block_coro`. `RanOff`: a block with no `return`, `if false then return end`, and a block that only calls a function holding a `return`. `Returned(None)`: `return`, `return nil`, `return nil, 'text'`, `do return end`, `if true then return end`, and `return f()` for an `f` that gives nothing. Scalars and `return {}` as today. The same for a block that suspends first.
- **`require`.** `a_block_without_a_value_lets_the_next_block_run` in [tests/require.rs](../crates/runvil-internal/lua/src/tests/require.rs) is replaced: a block that runs off lets the next one run, `return nil` and a bare `return` end the load with `true` and the later blocks do not run, and a second `require` gets the saved `true` without a rerun.
- **Updated.** `section_vm_accepts_only_scalar_top_level_returns` asserts on the result type. The helpers `run_scalar` and `returned` read `RanOff` as no value, so the other tests stay as they are.
- **Must pass untouched.** The four tests that pin exact compile diagnostics, and the tests that place an error on the line of a top-level `return f()`.

`runvil-runtime`, through `Scenario`, beside the existing ones in [exit_rules.rs](../crates/runvil-internal/runtime/src/execute/tests/exit_rules.rs) and [returns_and_prose.rs](../crates/runvil-internal/runtime/src/execute/tests/scheduler/h1/returns_and_prose.rs):

- **Main walk.** `return nil`, a bare `return`, an early exit in an `if`, and `return store.write(...)` each end the run with `done`, and a later block and a later section that would raise never run. A `return` inside a function does not end the chain.
- **H1.** A bare `return` ends the run with `done` and no section runs. The `var` read-back still happens.
- **`call`.** The child ends at the return, the sections after it do not run, and the caller gets `""` and goes on.
- **Tasks and arms.** `.ok` holds and `.text == ''`.
- **Child walk.** A return with no value in a level that a `jump` started ends the whole chain.
- **Events.** The section that returned reports that it finished.

## Documents: one commit each

They are written once the code passes its checks, and committed in their own repositories before the `runvil-harness` commit, so the plan copy can record them as done. The two can be written at the same time.

- **`runvil-design`:**
  - [decisions/prompt-language.md](https://github.com/cppalliance/runvil-design/blob/master/decisions/prompt-language.md): the section "A returned scalar ends the chain", the `require` section titled "returns its first non-nil block value", and the lines that say a block that returns nil lets the next block run. Open question 4, `done` against the empty string, stays open.
  - [decisions/tools-and-packs.md](https://github.com/cppalliance/runvil-design/blob/master/decisions/tools-and-packs.md): the line that says a handler's first rule is also the rule for a block's `return`.
  - [architect-runvil-prompting-language.md](https://github.com/cppalliance/runvil-design/blob/master/design/architect-runvil-prompting-language.md): "Returning nil continues" in the paragraph on Lua fences.
- **`runvil-docs`:**
  - [04-how-a-prompt-runs.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/04-how-a-prompt-runs.md): "How a block ends" gains a fourth ending, running off the end. "The scalar return rule" and "A return ends the chain" state the new rule. The table of results gains rows for `return nil` and a bare `return`, and its `return nil, 'text'` row ends the chain. "Returning nothing" becomes a section about a block with no `return`. "Returning from the H1 pass" covers a return with no value.
  - [03-blocks-and-prose.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/03-blocks-and-prose.md): "What `require` returns" and its `block_returns` example, whose result is no longer `ran then third`.
  - [08-jump-and-call.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/08-jump-and-call.md): "Returning from a child walk", and what `call` gives for a child that returns no value.
  - Smaller wording in chapters 2, 9, 12, 14, and 15: places that say a block "returns nothing" when it has no `return`, the handler rule in chapter 12, the `owner_returned` row, and the timed `tasks.join_any` example in chapter 15, whose task now ends if the wait times out.
  - Every changed example value comes from a run of the built code, because the guide has no tooling that checks its examples.

## Git

- The three commits stay local, each on `master` of its repository. Pushing them is yours to call.
- The Workshop pins `runvil-harness` by commit, so it gets the new rule when its pin moves. Its one prompt needs no change.

## Verification

Every command in `runvil-harness`'s `AGENTS.md`, before the commit: format, clippy, nextest, both doc builds, `cargo test -p build-xtask`, the wording check, `xtask api --check`, and dylint. Neither facade listing changes, because `LuaBlockResult` is not on a facade.

## What the build found

The design held. These points differ from the plan above, or add to it.

- **The two grammar facts hold.** The compile tests were written and run first. A source that ends in `return` compiles inside the `do` block, a source that ends in a line comment compiles with the trailer on its own line, and a label at the end of a block still takes a `goto` from before a local. `end do` is refused with the diagnostic of the source alone, `<eof> expected near 'end'`.
- **The loader compares with `~=`.** The plan showed `rawequal`. Hardening removes `rawequal` before `require` installs, so `__impl_require.lua` writes `value ~= ran_off`. The marker is a light userdata, which Lua compares by identity with no metamethod, whatever the other value is.
- **The marker is the address of a private static.** `ran_off_marker` in `program.rs` builds a light userdata from the address of a `static` byte, and `is_ran_off` compares pointers. No value a prompt can build equals it, and `json.null`, the other light userdata a prompt can hold, is the null pointer.
- **A source at Lua's nesting limit fails as a block.** The `do` block is one more level, so the deepest nesting that Lua compiles alone is one level too deep inside it. A test finds that depth by search and asserts that the failure has the `lua_compile` code and is placed in the prompt file.
- **A block the Engine did not compile as one has no trailer.** `start_block_coro` also takes a program from `compile_internal`, which one test passes it. Such a program returns nothing when it runs off its end, so it reads as a `return` with no value. Production passes only prompt blocks.
- **`return ...` reads as running off the end**, as the plan expected, and a test states it.
- **The old rule fails the new tests.** With `block_result` changed back to the old rule, 12 of the new tests failed, in both crates. Two kinds of new test pass under either rule, as they should: the `require` tests, because the loader reads the marker itself, and the test that an H1 `return` with no value still reads `var` back, because the pass reads `var` back at its fall-through too.
- **`tasks.reply` and adhoc handlers are unchanged.** They still turn nil into `""` through `scalar_return`, and their doc comments stand.
- **Tests.** `runvil-lua` has 17 more tests and `runvil-runtime` has 13 more. The whole workspace runs 3,604 tests, all passing.
- **The guide.** Chapters 1 and 5 changed as well, by one and two sentences, and chapter 9 needed no change. The timed `tasks.join_any` example of chapter 15 is unchanged: its documented run does not time out, and by a reading of the code a `Child` whose wait times out fails as `tasks_live` under either rule, because the grandchild is still live when `Child` ends. That case was not run.
- **The guide's books build.** `cargo xtask site --books-only`, run from `runvil` with `RUNVIL_DOCS` set, built both books and found no broken link, and the new anchor `returning-no-value` is in the built chapter 4. The 21 prompts behind the changed example values ran through `runvil-engine` in a scratch program outside this repository.
- **The design pages.** `tools-and-packs.md` now says where a handler's `nil` and a block's `return` of `nil` differ. The language spec's `call` entry still says nil for a child with no text, which the code gives as the empty string. That was left alone with the decision it belongs to.
- **Verification.** Every check listed passed.
- **Documents.** `runvil-design` commit `44834c6` and `runvil-docs` commit `3d03fe7` hold the document changes.
