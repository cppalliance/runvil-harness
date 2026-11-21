---
name: Host Builder Cleanup
overview: "Eleven changes, as seven code commits in runvil-harness and one in runvil-workshop: a test broker with two models, two stale records tidied, the timer moved from `Host` to `RunSetup`, the built-in Packs made by the runner, a loader that reports a library file and an entry point as it reports a directory's library, the two one-prompt finders made inside an `Arc`, and a `Host` that is built by `Host::builder`, lives only inside an `Arc`, loads its Plugins, and keeps one report for each of them. Both repos get a pull request from the fork, merged Harness first once every check is green. The last step writes a report on the new API as output, for a follow-up plan."
todos:
  - id: prep
    content: "Check that runvil-harness and runvil-workshop are clean; fast-forward runvil-harness master to cppalliance master and push the fork; run the tests once for the base count; Harness commit 0: copy the approved plan alone to runvil-harness/plan/2026-11-21-1-host-builder-and-run-timer.plan.md"
    status: completed
  - id: two-models
    content: "Harness commit 1 (item 9): `Listing::ListsAfter` in the scripted broker, the selection test over a two-model list, the new no-selection test; verify.ps1 -Tag h1; commit"
    status: completed
  - id: tidy-records
    content: "Item 10: close issue #29 with its comment; Harness commit 2 (text only): rewrite `## Left open` in plan/2026-11-20-1-remove-ui-and-fluent-setters.plan.md"
    status: completed
  - id: timer
    content: "Harness commit 3 (item 1): `RunSetup::new(recorder, broker, timer)`, `Host` loses its timer, `drive_run` reads the setup, every call site, tightened by the plan's rule, the two-clocks test, six page entries and the docs, bless the runvil listing (one line changed, one added, one removed); verify.ps1 -Tag h3; commit"
    status: completed
  - id: builtins
    content: "Harness commit 4 (item 2): `PackRegistry::new()` starts empty and refuses the built-in ids as `pack_id_taken`, `PackOrigin` removed, `packs_builtin.rs` in the runner, tests moved and the pin test added, three page entries, bless; verify.ps1 -Tag h4; commit"
    status: completed
  - id: plugin-file
    content: "Harness commit 5 (items 7, 12): in the loader, `insert_plugin_file`, `make_report`, and an `insert_plugin` that returns a `PluginReport`; the loader's unit tests adapt, four new tests, two page entries, bless the runvil listing (one changed line); verify.ps1 -Tag h5; commit"
    status: completed
  - id: finders
    content: "Harness commit 6 (item 11): `FindTextPrompt::new` and `FindFilePrompt::new` return the finder inside an `Arc`, the four `Host` sites that wrap one lose their `Arc::new`, one new test of a `Host` over a file finder, one page entry, bless the runvil listing (two changed lines); verify.ps1 -Tag h6; commit"
    status: completed
  - id: builder
    content: "Harness commit 7 (items 3, 4, 5, 6): the runner depends on the loader, `Host::builder`, `with_rust_plugin`, `build`, which logs each report, `Host::packs`, `Host::plugin_reports`, private fields, `Packs` out of the public API, every Host built through the builder, each call site tightened by the plan's rule, the example, four new tests, pages, Cargo.lock (one added line), bless; verify.ps1 -Tag h7; run the example; commit"
    status: completed
  - id: builder-plugins
    content: "Harness commit 8 (items 5, 6, 7, 12): the runner depends on `runvil-plugin-sys`, `with_resources`, `with_plugin_dir`, `with_plugin_file`, `with_plugin`, `insert_plugin` out of the facade, the test Plugin's suites on the builder with seven new tests, each call site tightened, pages, Cargo.lock (one added line), bless (four added lines, one removed); verify.ps1 -Tag h8; commit"
    status: completed
  - id: workshop-dry-run
    content: "Workshop dry run under a temporary [patch] on the local Harness: make the edits of the Workshop commit and tighten each call site, run clippy and nextest for workshop-agents and workshop-server, then restore the root Cargo.toml and Cargo.lock"
    status: completed
  - id: record
    content: "Harness commit 9: add 'What the build found' for both repos and the todo statuses to the repo copy of the plan, and commit it alone"
    status: completed
  - id: harness-pr
    content: Push Harness master to origin and open the pull request from vinniefalco:master
    status: completed
  - id: workshop-commit
    content: "Workshop commit: pin the three rev values to the Harness head, cargo fetch, run verify-workshop.ps1 -Tag w1, commit, push to origin, open the pull request"
    status: completed
  - id: merge
    content: "When every check on both pull requests is green and neither upstream master moved: git push upstream master in the Harness and confirm, then the same in Workshop, then confirm that each fork equals its upstream"
    status: pending
  - id: api-report
    content: "Write the API report as output, `host-builder-api-review.md`, for a reader who is new to the code: draft it while the checks run, finish it after both merges from the merged code, the listing, and the notes file; ask before overwriting a file with that prefix; write it in chunks; one cold read by a fresh subagent, then fix what it could not follow"
    status: pending
isProject: false
---

# Move the timer into RunSetup, build the Host with a builder, and let the runner make the built-in Packs

Eleven changes land as seven code commits in `runvil-harness` and one commit in `runvil-workshop`. Nine are from the design conversation, and two are the ones that you added while you reviewed this plan. When they are in, a Host application builds its `Host` in one chained statement, the `Host` is sealed inside an `Arc` from the moment it exists, and everything a run waits on arrives in its `RunSetup`. The classroom and the guide stay as they are. The plan ends with a report on the new API, written for a reader who is new to the code, which a follow-up plan starts from.

## For the agent that runs this plan

This section is for an agent that starts with this file and has seen none of the conversation behind it. What the run needs is in this file, in the two repos, and in the files named here. In this file, "I" is the agent that runs the plan, and "you" is the owner of the repos, who approved it.

### The short names this plan uses

- **The Harness** is the repo `runvil-harness`, and **Workshop** is the repo `runvil-workshop`, a Host application. **The guide** is `runvil-docs`, and **the classroom** is `runvil-classroom`.
- **The facade** is the crate `runvil` in `crates/runvil`, the one crate that a Host application depends on. It re-exports what the other crates define.
- **The runner** is `harness-runner` in `crates/harness-internal/runner`. It holds `Host`, `RunSetup`, `Harness`, `Packs`, and the stock prompt finders.
- **The loader** is `harness-plugin-loader` in `crates/harness-internal/plugin-loader`. It loads Plugins that follow the C contract.
- **The test Plugin** is `harness-test-plugin` in `crates/harness-internal/test-plugin`, a library Plugin that the end-to-end tests load.
- **The Gateway client** is `harness-gateway-client` in `crates/harness-gateway-client`. It holds `TokioTimer`.
- **The registry** is `PackRegistry`, in the crate `runvil-plugin` in `crates/runvil-plugin`, which a Plugin author builds against.
- **The listing** is `crates/runvil/public-api.txt`, the recorded public API of the facade, which a check compares with the code. To bless it is to rewrite it from the code. The Engine has one too, `crates/runvil-engine/public-api.txt`.
- **The pages** are the decision pages in `runvil-harness/decisions/`. Each entry is a heading with the rule and its reason.
- **The runner's suites** are the tests in `crates/harness-internal/runner/tests/it/`, **the facade suite** is `crates/runvil/tests/suite/`, and **the example** is `crates/runvil/examples/run-prompt.rs`.

### Read first

- [runvil-harness/AGENTS.md](runvil-harness/AGENTS.md) and [runvil-workshop/AGENTS.md](runvil-workshop/AGENTS.md): the terms, the rules, and the verification list of each repo. Where this plan and they differ, they are right.
- [.cursor/rules/commit.mdc](.cursor/rules/commit.mdc): how a commit message is written. It is written from the staged diff alone. It names the plan only when the diff is the plan file, as in commits 0, 2, and 9.
- [runvil-harness/plan/2026-11-20-3-stock-tokio-timer.plan.md](runvil-harness/plan/2026-11-20-3-stock-tokio-timer.plan.md): the last plan of this kind, with its record. The record starts at the heading `# What the build found`, and commit 9 of this plan follows its form.

### Where things are

- **The repos:** `runvil-harness` and `runvil-workshop` are checkouts in the workspace root. In each, `origin` is the fork under `vinniefalco` and `upstream` is the repo under `cppalliance`. The one branch is `master`. `runvil-docs` and `runvil-classroom` are beside them and stay untouched.
- **The scripts:** `cabinet/_scratch/remove-ui-build/` holds three PowerShell scripts from the earlier plans.
  - `verify.ps1 -Tag <tag>` runs the Harness's verification list in order. It prints one `exit=` line for each step, then the test summary, the line of the listing check, and what the custom lints warned about. The full output of each step is a log beside the script, named `<tag>-<step>.log`.
  - `verify-workshop.ps1 -Tag <tag>` does the same for Workshop. Its tests run in two groups, which is where Workshop's two counts come from.
  - `check-message.ps1 -Message <file> -Repo <path>` checks a commit message against the staged diff of that repo: the length of the subject, each backticked token, and each dash.
  - The folder is scratch. If it is gone, each list is under `## Verification` in the repo's `AGENTS.md`, and a script is that list with the output of each step sent to a log.
- **The notes of this run:** `cabinet/_scratch/host-builder-cleanup/notes.md`.
- **The conversation behind this plan:** the chat [Host builder plan](211698d2-851d-4214-ac0c-96a33d2e0a6a). Its transcript can be searched for a detail that this file leaves out.

### Rules of this work

The workspace rules apply. These come from the owner's requests in the conversation:

- **This file stays as it is during the run.** Results go into the notes, into the repo copy with commit 9, and into the chat.
- **Each code commit passes the whole verification list of its repo before it is committed,** and each is one commit on `master`.
- **Nothing reaches `upstream` before every check on both pull requests is green.** The Harness merges first.
- **Workshop is committed only with its pin on a Harness commit.** The `[patch]` section of the dry run stays out of every commit.
- **A question to the owner is a choice between concrete options.** A small point is settled with a stated confidence level.
- **Each report to the owner gives the context of every statement.** Each point names the thing, says what it does today, and then gives the finding, in plain conversational English. A conclusion without the facts behind it is what the owner asked twice to be spared.

### How this machine behaves

The machine runs Windows with PowerShell.

- **`gh` with `--jq`:** the expression goes in single quotes and holds no double quote, as in `gh api repos/cppalliance/runvil-harness/commits/master --jq '.sha'`.
- **`git` and `cargo` write progress to stderr,** which PowerShell shows as an error record. The exit code is what counts.
- **The API listing** is checked and blessed with the toolchain `nightly-2026-09-05`, while `CARGO_BUILD_WARNINGS` is unset.
- **After a manifest changes,** `cargo fetch` refreshes `Cargo.lock`. `cargo fmt --all` runs before each verification.
- **A push runs a pre-push hook** in each repo, so a push takes a while.
- **CI runs two commands beside the Harness's list:** `cargo doc -p runvil --no-deps` with `RUSTDOCFLAGS="-D warnings"`, and `cargo deny check`. Both run locally before the Harness is pushed.
- **An automatic reviewer, CodeRabbit, comments on each pull request.** On the last Harness pull request it found a count of code lines in a page entry, which `AGENTS.md` keeps out of an entry, and one more commit removed it.
- **The counts of tests are those of Windows.**

### Checks that refuse things you might write

`AGENTS.md` of each repo is the authority. These are the ones that earlier work ran into:

- **Clippy** runs with `all` and `pedantic` denied. CI denies warnings, which include `missing_docs`, `missing_debug_implementations`, and `unreachable_pub`.
- **Rustdoc** denies a broken link and a link to a private item.
- **The custom lint `io_try_without_context`** refuses a bare `?` that converts a `std::io::Error`.
- **The structural checks,** `cargo test -p build-xtask`:
  - limit a type to 25 methods and 12 fields
  - ban a compiled code block in a doc comment, so an example there uses a `text` fence
  - allow the facade only grouping `pub mod` blocks and `pub use` lines of one item each
  - ban `tokio` under `[dependencies]` of `runvil` and of each crate in `crates/harness-internal`
  - let `runvil-plugin` depend on `runvil-types` and `runvil-vfs` alone
- **The listing check** refuses a facade surface, or doc text of it, that names an internal crate. The listing records the `auto` line and the `derives` line of each type.
- **The wording check** reads `AGENTS.md` and each crate doc that has `## Invariants`. Host, Engine, Harness, and Plugin are written with a capital outside backticks, an Engine crate leaves the Host unnamed, and a comment avoids each word that starts with "activat".
- **A decision page entry** is free of file paths, internal names, counts of things in the code, test names, dates, and history. A change to what a Host application sees updates its entry in the same commit.
- **Rust:** `Arc::clone(&x)` fails to compile where an `Arc<dyn Trait>` is expected, and `x.clone()` works.

### The first moves

1. Read the three files under Read first.
2. Check the state that Where this starts describes: `git status -sb` in both checkouts, the two upstream heads with `gh api`, and the open pull requests with `gh pr list --repo cppalliance/runvil-harness` and the same for `runvil-workshop`.
3. Take the todos in their order, starting with `prep`.

## The eleven items and the commit each one lands in

The numbers are those of a list of changes from the design conversation. Each item is restated here in full, so the list itself is not needed. Item 8, the stock `TokioTimer`, is merged already, as pull request #59 of the Harness. Items 11 and 12 are the two changes that you asked for while you reviewed this plan: the prompt finders, and one way of reporting for every kind of Plugin.

- **Item 9, a test broker that lists two models:** Harness commit 1. It holds tests alone and needs nothing from the other items, so it goes first.
- **Item 10, two stale records:** issue #29 is closed after commit 1, and Harness commit 2 corrects the "Left open" list of an earlier plan record.
- **Item 1, the timer moves to `RunSetup`:** Harness commit 3.
- **Item 2, the runner makes the built-in Packs:** Harness commit 4.
- **Item 7, load one library file:** Harness commit 5 adds the loader's function, and commit 8 adds the builder method over it.
- **Item 11, the two one-prompt finders are made inside an `Arc`:** Harness commit 6. It comes right before the builder, so that the builder commit writes each `Host` site in its final form.
- **Items 3 and 4, one public Pack type and a `Host` that lives only inside an `Arc`:** Harness commit 7. They share a commit with the builder because the builder does the conversion that `Packs::new` does today, and a public `Packs` would then be a type that no public function accepts.
- **Item 5, the Host builder:** Harness commits 7 and 8. Commit 7 has the builder with Rust Plugins, and with it the runner's dependency on the loader crate, which is a new edge between two crates. Commit 8 adds the Plugins of the C contract.
- **Item 6, the Host keeps what loaded and what failed:** the `Host` keeps one `PluginReport` for each step that names a Plugin. Commit 7 adds `plugin_reports()` with the reports of Rust Plugins, and commit 8 adds the reports of the Plugins of the C contract.
- **Item 12, one way of reporting for every kind of Plugin:** Harness commit 5 makes the loader report an entry point as it reports a library. Commit 8 takes `insert_plugin` out of the facade, because `with_plugin` is then the way a Host application loads an entry point.

## How the work is delivered

1. The approved plan is copied into `runvil-harness/plan/2026-11-21-1-host-builder-and-run-timer.plan.md`, as a commit of its own.
2. Each testable step is one commit on `master`: seven in the Harness and one in Workshop. Each passes its repo's whole verification list before it is committed. The plan commit, the record fix, and the record commit hold text only.
3. Each repo is pushed to the fork (`origin`), and a pull request is opened from `vinniefalco:master`.
4. When every check on both pull requests is green, the merges go upstream in order: the Harness first, then Workshop, because Workshop pins a Harness commit that upstream `master` has to hold.
5. When both merges are confirmed, the report on the new API is finished and written as output.

## Where this starts

Read from the checkouts and from GitHub on 2026-11-21 at about 01:30 PST, and again at 06:50 PST with the same result.

- **`runvil-harness`:** local `master` and the fork are `02c925b1`. `master` of `cppalliance/runvil-harness` is `f40a9a2e`, four commits ahead. Those four are pull request #26, which adds tests in three files of the Engine's runtime and one plan file. So the first step fast-forwards the local branch to upstream and pushes the fork.
- **`runvil-workshop`:** local `master`, the fork, and `cppalliance/runvil-workshop` are all `4666b17f`, which pins the Harness at `f87bfbf5`.
- **Test counts:** the Harness ran 3,979 tests before upstream's four commits, so the first step runs the tests once on the fast-forwarded tree to get the base count. Workshop's two groups ran 706 and 353.
- **What a search found in the Harness:** a `Host` is written as a struct literal in 10 places, `Packs::new(` is called 29 times, and `RunSetup::new(` 22 times. The runner's suites pass a timer by hand in 38 `launch` calls and 23 `drive_run` calls. In Workshop the first three numbers are 4, 2, and 4, all in `workshop-server` and `workshop-agents`.
- **The structural checks allow the new dependencies of commits 7 and 8.** `build-xtask` has a fixture named `harness_container_siblings_may_depend_on_each_other`, and its ban on tokio reads only the names `tokio` and `tokio-util`. So `harness-runner` may list `harness-plugin-loader` and `runvil-plugin-sys`.
- **The guide needs nothing.** A search of `runvil-docs/src` found no line that names the registry, `Packs`, the fields of `Host`, or `RunSetup::new`. Its lines about a timer are about a prompt's timed wait.
- **Two open pull requests touch files that this plan changes.** Harness #60 edits `effect_loop-broker.rs` in the runner's suites and `broker.rs` in the facade suite, and each of those holds a `RunSetup::new` call. Workshop #32 edits `agents/tests.rs` and `tests-gateway.rs` of the server. The work starts from upstream `master` as it is on the day you say go. If one of them has merged by then, the compiler finds its call sites with the rest. If one merges while this work is open, I stop (see When to stop).

## The API after this plan

```rust
impl Host {
    pub fn builder(prompts: Arc<dyn PromptFinder>) -> HostBuilder;
    pub fn parse(&self, name: &str) -> Result<Arc<Prompt>, Error>;   // as today
    pub fn packs(&self) -> impl Iterator<Item = &PackInfo>;          // every Pack, the built-ins included
    pub fn plugin_reports(&self) -> &[PluginReport];                 // one for each step that names a Plugin, in the order written
}

impl HostBuilder {
    pub fn with_resources(self, resources: HostResources) -> HostBuilder;
    pub fn with_plugin_dir(self, directory: impl Into<PathBuf>) -> HostBuilder;
    pub fn with_plugin_file(self, path: impl Into<PathBuf>) -> HostBuilder;
    pub fn with_plugin(self, entry: PluginEntry) -> HostBuilder;
    pub fn with_rust_plugin(
        self,
        install: impl FnOnce(&mut PackRegistry) -> Result<(), Error> + Send + 'static,
    ) -> HostBuilder;
    pub fn build(self) -> Arc<Host>;
}

impl RunSetup {
    pub fn new(
        recorder: Arc<dyn RunRecorder>,
        broker: Arc<dyn InferenceBroker>,
        timer: Arc<dyn Timer>,
    ) -> RunSetup;
    // the six setters stay as they are
}

impl FindTextPrompt {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Arc<FindTextPrompt>;
}

impl FindFilePrompt {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Arc<FindFilePrompt>;
}
```

The example in the repository then reads:

```rust
let host = Host::builder(FindTextPrompt::new(GREET_NAME, GREET)).build();
let prompt = host.parse(GREET_NAME)?;
let setup = RunSetup::new(
    Arc::new(MemoryRecorder::new()),
    Arc::new(OfflineBroker),
    Arc::new(TokioTimer),
)
.args(r#"{"text":"desk"}"#);
let greeting = Harness::new(host, prompt, setup).run().await?.result?;
```

Workshop's startup then reads:

```rust
let mut builder = Host::builder(prompts).with_resources(resources);
if let Ok(runtime) = tokio::runtime::Handle::try_current() {
    builder = builder.with_rust_plugin(move |packs| plugin_mcp::install(packs, servers, runtime));
}
// A directory that is known to be missing is a state: a build that has
// not placed its Plugins has none.
if let Some(directory) = plugins.filter(|directory| directory.try_exists().unwrap_or(true)) {
    builder = builder.with_plugin_dir(directory);
}
let host = builder.build();
```

A `PromptSet` is filled through `&mut self` after `new()`, so its owner puts it into an `Arc` once it is filled, as in `Host::builder(Arc::new(set))`. A closure is wrapped by the code that writes it.

Every kind of Plugin is reported in one way. Each `with_` method that names a Plugin makes one `PluginReport` when `build()` runs, and the `Host` keeps them in the order written. A report holds the Plugins of the C contract that its step loaded and each failure of that step, as the report of `insert_plugin_dir` does today.

`build()` also logs each report through `tracing` as it makes it: each loaded Plugin at the info level, and each failure at the warn level. So a Host application with a `tracing` subscriber sees both with no code of its own, and Workshop's own logging of them goes.

The facade keeps one function of the loader, `insert_plugin_dir`, for code that fills a registry apart from a `Host`. `insert_plugin` leaves it, because `with_plugin` is how a Host application loads an entry point.

## The order of work

```mermaid
flowchart LR
    Prep[Fast-forward] --> C1[Two models]
    C1 --> C2[Tidy records]
    C2 --> C3[Timer in setup]
    C3 --> C4[Built-in Packs]
    C4 --> C5[Loader reports]
    C5 --> C6[Finders in Arc]
    C6 --> C7[Host builder]
    C7 --> C8[Builder Plugins]
    C8 --> Dry[Workshop dry run]
    Dry --> Rec[Record commit]
    Rec --> HPR[Harness PR]
    HPR --> WPR[Workshop PR]
    WPR --> Green[All green]
    Green --> MH[Merge Harness]
    MH --> MW[Merge Workshop]
    MW --> Report[API report]
```

Every commit goes on `master`. Each code commit passes its repo's whole verification list by itself.

## Tightening at each call site

You asked me to look around each call site as the plan runs and to tighten what can be tightened. A call site here is a function in which a commit of this plan changes how a `Host`, a `RunSetup`, or a registry is built, passed, or read. The look covers that function and a helper that only it calls.

**The rule.** The commit that changes a call site also leaves it in the shortest clear form that the API of that commit allows. Every test asserts what it asserts today, and a test keeps its name unless the name says something that stopped being true.

**What I know to look for,** from reading the sites:

- **A `RunSetup` in struct update syntax becomes the chain,** when a method exists for each field it sets. There are 18 such sites in the Harness: 12 in the runner's suites, 5 in the facade suite, and 1 in the test Plugin's suite. `RunSetup { vfs: vfs.clone(), ..setup(..) }` becomes `setup(..).vfs(vfs.clone())`. A site that sets the timer keeps struct update syntax, because `new` alone takes the timer, so that form stays exercised.
- **A helper that is named for a Host returns the `Host`.** In the runner's suites, eight helpers with `host` in their name return a `Packs` today: `files_host`, `starting_host`, `slow_host`, `slow_host_of`, `hold_host`, `surviving_hold_host`, `keeping_hold_host`, and `host_holding`. Several tests name a `Packs` value `host`, and so do Workshop's server tests.
- **Two helpers that build the same thing become one.** `no_packs` in `harness.rs` and `bare` in `prepare.rs` of the runner's suites both build the Packs of a Host with the built-ins alone. From commit 7 on they are one helper in `support.rs`, `bare_host()`, which returns that Host.
- **A parameter that nearly every caller fills the same way goes.** Seven of the eight `launch` calls of the facade suite pass `no_packs()`.
- **A hand-written finder becomes a stock one,** where the test's assertions stay as they are. `host_over` in the runner's suites holds an eight-line closure that finds no prompt, which is what an empty `PromptSet` does. `two_prompts` in `harness-execute.rs` serves two names from an eleven-line closure, which is what a `PromptSet` with two inserts does. A finder whose failure a test compares, such as the one in `host-tests.rs`, stays, because a stock finder words that failure differently.
- **A function that makes a finder returns it inside an `Arc`,** as the two stock finders do from commit 6 on. Workshop's `agent_finder` returns a bare closure today, and each of its two `Host` sites wraps it in `Arc::new`.
- **A value that is kept only to keep Plugins loaded goes,** because the `Host` holds them from commit 8 on: `_plugins` in Workshop's `Inner`, and the `PluginReport` that the test Plugin's suite returns beside each Host.
- **A wrapper that the new form makes pointless goes.** `run_agent_on` in Workshop's server tests takes a bare `Host` and wraps it in an `Arc`.
- **What the old form leaves behind goes:** imports, the constant of built-in names that the `plugin-mcp` tests filter out, the two fake timers, and each doc comment that describes the old shape.

**Beyond that list** I look for what the same eye finds at each site: a variable that is used once, a clone that the new ownership makes needless, a comment that repeats its code, and a second name for one thing.

**Where it stops.**

- At the function and at a helper that only it calls. A change to what a test asserts, or to the logic of other functions, is outside.
- At the API of this plan. The public API stays exactly as this plan lists it.
- At a doubt. A tightening that I am unsure of stays undone and goes on the list below.

**How it is reported.** Each commit message names its tightenings among its findings. The record of commit 9 lists, for each commit, every tightening beyond the list above, and everything that I saw and left, with the reason.

## Harness commit 1: a test broker that lists two models (item 9)

Every test broker in the Harness lists one model. So `a_prompt_reads_the_selected_model_through_a_roles_handle` passes even for a Harness that takes the first listed model and never reads the selection, which is what the automatic review of pull request #57 pointed out.

- [runvil-harness/crates/harness-internal/runner/tests/it/scripted.rs](runvil-harness/crates/harness-internal/runner/tests/it/scripted.rs): the enum `Listing` gains the variant `ListsAfter(&'static str)`. A broker with it lists the model named there first and the scripted model `m` second.
- [runvil-harness/crates/harness-internal/runner/tests/it/harness.rs](runvil-harness/crates/harness-internal/runner/tests/it/harness.rs): that test gives its broker `Listing::ListsAfter("first")`. Its setup selects `m`, as the `setup` helper of that file always does, so the prompt must return `m`. A Harness that ignored the selection would return `first`.
- A new test beside it, `a_run_with_no_selection_binds_the_first_listed_model`: the same broker, a setup with the selection left empty, and the prompt returns `first`. Today that rule is tested over an empty list alone.
- Expected count: the base count plus one. Both listings and the pages stay as they are, because the commit holds tests only.

## The two records (item 10)

- **Issue #29.** It reports that a handle built from a model id has a window of 8192 tokens, whatever the model. Commit `d5c2ebe9` removed that kind of handle. After commit 1, I close the issue with `gh issue close 29 --repo cppalliance/runvil-harness` and a comment that says: `models.get` takes one of the five model roles since `d5c2ebe9`, so a handle is built from a role alone, and the fixed window went with the code that built a handle from a model id.
- **Harness commit 2, text only.** In [runvil-harness/plan/2026-11-20-1-remove-ui-and-fluent-setters.plan.md](runvil-harness/plan/2026-11-20-1-remove-ui-and-fluent-setters.plan.md) the section `## Left open` lists four points, and by then each one is settled: the guide, Workshop, and that record reached `cppalliance` on 2026-11-20, the issue is closed, and commit 1 holds the test broker with two models. The four bullets become one paragraph that says so, under the same heading.

## Harness commit 3: the timer moves from `Host` to `RunSetup` (item 1)

### Why

The Harness uses the timer at two moments, and both are inside one run: `Harness::run` asks it for the run's start time, and the effect loop calls `sleep` for a wait with a timeout. `Host::parse` leaves it unread, which is why two unit test files of the runner each define a fake timer whose one job is to fill the field. After this commit the three services that a run waits on, the recorder, the broker, and the timer, all arrive in its `RunSetup`, and a `Host` holds the Packs and the prompt finder.

### Code

- [runvil-harness/crates/harness-internal/runner/src/setup.rs](runvil-harness/crates/harness-internal/runner/src/setup.rs): `RunSetup` gains the public field `timer: Arc<dyn Timer>` after `broker`, and `new` takes it as its third argument. `new` is the one place that sets it, which is the rule for the recorder and the broker too. The `Debug` impl leaves it out beside those two.
- [runvil-harness/crates/harness-internal/runner/src/host.rs](runvil-harness/crates/harness-internal/runner/src/host.rs): the field `timer` goes.
- [runvil-harness/crates/harness-internal/runner/src/harness.rs](runvil-harness/crates/harness-internal/runner/src/harness.rs): the start time is `setup.timer.now()`, and the effect loop gets the setup's timer.
- [runvil-harness/crates/harness-internal/runner/src/effect_loop.rs](runvil-harness/crates/harness-internal/runner/src/effect_loop.rs): `drive_run` and the private `drive` lose their `timer` parameter and read `setup.timer`. A second timer beside the setup's would let the two differ. `drive_run` is public inside the runner for its own suites, and the facade leaves it out.

### Call sites

The compiler finds each one. The counts are from a search.

- **`RunSetup::new(`, 22 calls:** each gains a timer. The four places that give a `Host` `Arc::new(TokioTimer)` today (the example, the facade suite, the test Plugin's suite, and the Gateway client's timer test) pass it to the `RunSetup::new` beside them.
- **`Host` literals, 10:** each loses its `timer:` line.
- **The runner's suites:** `launch` in `harness.rs` and `host_over` in `support.rs` lose their timer parameter, which changes 38 `launch` calls, and 23 `drive_run` calls lose their timer argument. Each run keeps the timer that it has today. A helper that builds a setup names the timer that most of its callers pass: `setup` in `harness.rs` uses `PendingTimer`, which 28 of the 38 `launch` calls pass, and `setup_over` in `prepare.rs` and `loop_setup` in `support.rs` use `Unused`. A call whose timer differs from its helper's puts its own timer into its setup with struct update syntax: `HeldTimer`, `PinnedClock`, `SleepTimer`, and a `PendingTimer` whose flag the test reads. The seven `launch` calls that pass `Unused` are in the `prepare` files. Their setups come from `setup_over`, or from a `RunSetup::new` of their own, which takes `Arc::new(Unused)` as its third argument.
- **Two fake timers are deleted:** `Instant` in `host-tests.rs` and `NoWait` in `prompt_finders-tests.rs`. Filling the field was their one job.

### Tests

- `a_pinned_timer_stamps_the_runs_start` in [runvil-harness/crates/harness-internal/runner/tests/it/prepare.rs](runvil-harness/crates/harness-internal/runner/tests/it/prepare.rs) passes its clock in the setup and asserts the same start time.
- New in the same file, `two_runs_on_one_host_each_start_at_the_clock_of_their_own_setup`: one `Host`, two setups with two pinned clocks, and each run's recorded start is the time of its own clock. Before this commit one `Host` had one clock.
- The run test in [runvil-harness/crates/harness-gateway-client/src/timer.rs](runvil-harness/crates/harness-gateway-client/src/timer.rs) passes `TokioTimer` to `RunSetup::new`, and its name ends in `of_its_run` where it ended in `of_its_host`.
- Expected count: one more than after commit 1.

### Pages and docs

In [runvil-harness/decisions/harness-and-host.md](runvil-harness/decisions/harness-and-host.md), six entries change:

- **The opening paragraph:** the `Host` struct is the installed Packs and a prompt finder, and the timer joins what a Host fills a `RunSetup` with.
- **"A Host builds one `Host` at startup and one `RunSetup` for each run":** the fields are `packs` and `prompts`. The rule in its second paragraph becomes: `Host` contains what a parse reads and what every run shares, and `RunSetup` contains what one run uses, which includes the services a run waits on: the recorder, the broker, and the timer.
- **"`RunSetup::new` takes the recorder and the broker, and every other field has a default":** the heading and the first sentence name the timer as the third required argument.
- **"Each optional field of `RunSetup` has a method that sets it and returns the setup":** its last sentence names the timer beside the recorder and the broker.
- **"The Host's `Timer` performs each sleep and stamps each run's start":** its second paragraph says that the timer is a required argument of `RunSetup::new`, so a Host that leaves it out fails to compile, and that it is in the setup because a run is what uses it.
- **"`TokioTimer` is the stock timer of a Host that runs on tokio":** a Host on tokio passes `Arc::new(TokioTimer)` to `RunSetup::new`.

The doc comments that say a `Host` holds the timer change with it: the crate docs of [runvil-harness/crates/harness-internal/runner/src/lib.rs](runvil-harness/crates/harness-internal/runner/src/lib.rs) and [runvil-harness/crates/runvil/src/lib.rs](runvil-harness/crates/runvil/src/lib.rs), the runner's `README.md`, the docs in `host.rs`, `harness.rs`, `setup.rs`, and the Gateway client's `timer.rs`, and the comments of the example.

### Listing

Bless with `cargo +nightly-2026-09-05 xtask api --bless` while `CARGO_BUILD_WARNINGS` is unset. The diff of `crates/runvil/public-api.txt` must be exactly this: the line of `RunSetup::new` changes, one line is added for the field `RunSetup::timer`, and the line of `Host::timer` is removed. The Engine's listing stays as it is in every commit of this plan.

## Harness commit 4: the runner makes the two built-in Packs (item 2)

### Why

Today `PackRegistry::new()` starts with `runvil/subagents` and `runvil/user`. That puts three things into `runvil-plugin`, the crate a Plugin author builds against: the list of built-ins, a Pack type for them, and the public enum `PackOrigin`, whose one job is to tell a built-in from a Plugin's Pack in `list()`. The runner already knows `runvil/user` by name, because it makes that Pack unavailable in a run with nobody there. After this commit the registry holds what Plugins inserted, and the runner adds the two built-ins when it builds a Host's Packs.

### Code

- [runvil-harness/crates/runvil-plugin/src/registry.rs](runvil-harness/crates/runvil-plugin/src/registry.rs): `PackRegistry::new()` returns an empty registry. A private list of the two built-in names stays, and `add` refuses a Pack whose Pack id a built-in holds. It uses the existing reason `pack_id_taken`, and the message names the built-in, as in ``Pack `acme/user` cannot take the Pack id user, which the built-in Pack `runvil/user` holds``. An insert of `runvil/user` itself gets the same refusal, where today its reason is `duplicate_pack`. `PackOrigin`, the `origin` of an entry, the `Builtin` Pack, and its factory are deleted. `list()` yields `&PackInfo`, and `Debug` shows each name and Pack id.
- New `runvil-harness/crates/harness-internal/runner/src/packs_builtin.rs`, a peer module of `packs.rs` as `performers_offline.rs` is of `performers.rs`: the two `PackInfo` values with their descriptions, the `Builtin` Pack, and its factory, moved as they are.
- [runvil-harness/crates/harness-internal/runner/src/packs.rs](runvil-harness/crates/harness-internal/runner/src/packs.rs): `Packs::new` adds each built-in after the registry's entries. The rule that `runvil/user` is unavailable in a run with nobody there stays where it is.
- [runvil-harness/crates/runvil-plugin/src/lib.rs](runvil-harness/crates/runvil-plugin/src/lib.rs) and [runvil-harness/crates/runvil/src/lib.rs](runvil-harness/crates/runvil/src/lib.rs): the two re-exports of `PackOrigin` go.

### What stays the same

Every Host still has both Packs, because `Packs::new` builds the Packs of every Host. So `uses: [user]`, `tools.packs.get('user')`, and the refusal `this run has no user` behave as before. The 14 runner tests about the built-ins, and the Workshop tests that declare `user` or `subagents`, stay as they are and show it.

### Tests

- [runvil-harness/crates/runvil-plugin/src/registry-tests.rs](runvil-harness/crates/runvil-plugin/src/registry-tests.rs) uses the private factory of the built-ins as its stand-in at 16 places, so it gets a small stand-in of its own. Its tests change like this:
  - `a_new_registry_holds_the_builtins_and_lists_every_pack_in_fqn_order` becomes `a_new_registry_is_empty_and_lists_every_pack_in_fqn_order`.
  - `add_refuses_a_pack_taking_a_builtins_pack_id` covers four names, `acme/subagents`, `acme/user`, `runvil/subagents`, and `runvil/user`, each refused as `pack_id_taken` with a message that names the built-in.
  - `add_refuses_an_fqn_the_registry_holds_already_the_builtins_included` keeps its case of a Plugin's Pack inserted twice, under a name without the built-ins.
  - The tests of `into_entries`, of `Debug`, of the refusal messages, and of `insert_unavailable` lose their built-in entries.
- `no_builtin_takes_a_reserved_pack_id` and `builtins_offer_no_tools_or_prelude` move to the bottom of `packs_builtin.rs`. One new test joins them, `the_registry_refuses_the_name_and_the_pack_id_of_each_builtin`. It fails when the runner gains a built-in that the registry would let a Plugin take.
- The tests that read a registry's listing change with it. `pack-tests.rs` and `host-tests.rs` of the loader and `lifecycle.rs` of the test Plugin read `list()` without the origin, and `lifecycle.rs` loses the filter that took the built-ins out. `support.rs` of the `plugin-mcp` tests loses its constant of built-in names and the filter over it. `plugin-tests.rs` of the loader asserts that a registry is as it was after a Plugin with no Pack loads, and it compares two empty listings now.
- Expected count: one more than after commit 3.

### Pages and docs

- [runvil-harness/decisions/tools-and-packs.md](runvil-harness/decisions/tools-and-packs.md), three entries. "A registry holds one Pack for each Pack id" says that every Host has the built-in Packs `subagents` and `user`, which the Harness makes itself, and that the registry refuses both ids with the same reason. "`user` marks a prompt that needs a person" and "`subagents` reserves a name" each say that the Pack is a Pack of every Host, where they said that it is built into every registry.
- The crate doc of `runvil-plugin`, the docs of `PackRegistry`, the bullet on `Packs` under `## Invariants` of the runner, and the doc of `mod plugin` in the facade.

### Listing

The diff must be exactly this: the five lines of `PackOrigin` are removed (its enum line, its two variants, its `auto` line, and its `derives` line), and the line of `PackRegistry::list` changes.

## Harness commit 5: the loader reports a library file and an entry point as it reports a directory's library (items 7, 12)

### Why

The loader's public functions load every library of a directory, or an entry point that is compiled into the Host. The builder needs a third one, for a single library file, and it needs every kind reported in one way.

Today the two existing functions part ways when the registry refuses a Pack. For a library, `insert_plugin_dir` keeps the Plugin in its `PluginReport` and records each refusal as a failure. For an entry point, `insert_plugin` returns the first refusal as an error, drops its handle on the Plugin, and discards the other refusals, while the Packs that went in stay. So a linked Plugin can be loaded and serving runs while its caller has only an error in hand. You asked for the same reporting, so `insert_plugin` returns a `PluginReport`.

This reverses one line of the design note of 2026-10-20, [runvil-harness/vibe/2026-10/2026-10-20-1-library-plugin-loading.md](runvil-harness/vibe/2026-10/2026-10-20-1-library-plugin-loading.md), which rejected a report from `insert_plugin` "because it loads exactly one Plugin, so a `Result` says exactly what happened". That held while a Plugin inserted no Packs. Since Plugins insert Packs there is a third outcome, loaded with a Pack refused, and a `Result` has to return that one as an error.

### Code

All of it is in [runvil-harness/crates/harness-internal/plugin-loader/src/plugin.rs](runvil-harness/crates/harness-internal/plugin-loader/src/plugin.rs), and [runvil-harness/crates/harness-internal/plugin-loader/src/lib.rs](runvil-harness/crates/harness-internal/plugin-loader/src/lib.rs) exports the two new functions from the loader crate.

- **New:** `pub fn insert_plugin_file(packs: &mut PackRegistry, path: &Path, resources: &HostResources) -> PluginReport`. It treats the file as `insert_plugin_dir` treats each file that it finds: a library that loads is the report's one Plugin, a library that fails to load is one failure with its path, and a Pack that the registry refuses is a failure while its Plugin stays in the report.
- **Changed:** `pub fn insert_plugin(packs: &mut PackRegistry, entry: PluginEntry, resources: &HostResources) -> PluginReport`, where it returns `Result<Arc<Plugin>, Error>` today. A Plugin that loads is the report's one Plugin. A load that fails is one failure. Each Pack that the registry refuses is one failure, and the Plugin stays in the report. The `Path` fact stays a library's alone, because an entry point has no file.
- **New:** `pub fn make_report(outcome: Result<(), Error>) -> PluginReport`. It makes the report of a step that loads no Plugin of the contract: empty for `Ok`, and holding the one failure for `Err`. The runner calls it from commit 7 on, for a Rust Plugin and for a directory that cannot be read, because the fields of `PluginReport` are private to the loader.
- **One private function records a loaded Plugin for all three kinds.** It inserts the Plugin's Packs, adds each refusal to the report as a failure, with the library's path when there is one, and adds the Plugin. The body of the loop in `insert_plugin_dir` becomes a second private function over it, which `insert_plugin_file` shares.
- `insert_plugin_file` makes `path` absolute first with `std::path::absolute`, as the directory function does for each file, because the loader opens a library on Windows by an absolute path. When that call fails, the report holds one failure with the reason `open` and the path as it was given. The function returns a report and holds no `?`, so the custom lint `io_try_without_context` has nothing to refuse.
- **The facade:** `insert_plugin_file` and `make_report` stay out of it, because the builder is the one caller that a Host application needs. `insert_plugin` stays re-exported in this commit, with its new return type, and leaves the facade in commit 8, when `with_plugin` exists.
- **The doc of `PluginReport`** says that it is what one call that loads Plugins loaded and what failed. Today it names `insert_plugin_dir` alone.

### Tests

- **The loader's unit tests** reach `insert_plugin` through three helpers in `fakes.rs`, `insert`, `loaded`, and `failure`, and through eight direct calls. The helpers read the report and return what they return today, so the tests behind them keep their assertions. Each direct call reads the report's one Plugin.
- **One test pins today's behaviour:** `a_pack_the_registry_refuses_fails_the_insert_and_names_the_pack` in `pack-tests.rs`. It becomes `a_pack_the_registry_refuses_is_a_failure_and_its_plugin_stays_in_the_report`, with the same assertions on the failure and one more on the report's Plugin.
- **One new unit test** in `plugin-tests.rs`, `a_report_made_from_an_outcome_holds_no_plugin_and_the_failure_of_the_outcome`.
- **Three new tests** in [runvil-harness/crates/harness-internal/test-plugin/tests/it/insert.rs](runvil-harness/crates/harness-internal/test-plugin/tests/it/insert.rs), over the copy of the fixture library that the suite already makes: a file loads and reports its name, its version, and its absolute path as its source; a file that is no library is one failure with the reason `open` and its path; and a file whose Pack the registry holds already is in the report, with the refusal as a failure that names the Pack.
- Expected count: four more than after commit 4.

### Pages and docs

- [runvil-harness/decisions/harness-and-host.md](runvil-harness/decisions/harness-and-host.md), "A Host depends on one crate, `runvil`": `insert_plugin` returns a `PluginReport`, as `insert_plugin_dir` does.
- [runvil-harness/decisions/tools-and-packs.md](runvil-harness/decisions/tools-and-packs.md), "A library Plugin and a C entry point compiled into the Host receive the same calls": a Pack that the registry refuses is a failure in the report of either function, and its Plugin stays in the report.

### Listing

The diff of `crates/runvil/public-api.txt` must be exactly one changed line, the return type of `insert_plugin`.

## Harness commit 6: the two one-prompt finders are made inside an `Arc` (item 11)

### Why

A `Host` holds its finder as `Arc<dyn PromptFinder>`, so every caller that gives it a stock finder writes `Arc::new(FindTextPrompt::new(..))` today. `FindTextPrompt` and `FindFilePrompt` are complete when `new` returns and stay as they are from then on, so `new` can hand the finder back in the form that a `Host` takes. From commit 7 on the line reads `Host::builder(FindTextPrompt::new("greet.md", GREET))`.

You chose this in the review over two other ways that I showed: an `Arc` from `PromptSet::new()` as well, with inserts that lock inside, and a `Host::builder` that takes any finder by value.

### Code

- [runvil-harness/crates/harness-internal/runner/src/prompt_finders.rs](runvil-harness/crates/harness-internal/runner/src/prompt_finders.rs): `FindTextPrompt::new` returns `Arc<FindTextPrompt>`, and `FindFilePrompt::new` returns `Arc<FindFilePrompt>`. The doc of each says that the finder comes back inside an `Arc`, which is how a `Host` holds it. The fields of both types are private, so `new` is the one way to get either, and each exists only inside an `Arc`.
- **`PromptSet` stays as it is.** Its owner fills it through `insert`, `insert_file`, and `insert_dir`, which take `&mut self`, and then puts it into an `Arc`. Its doc describes that use already.
- **Clippy's rule about `new`** accepts a return type that contains the type itself. So `Arc<FindTextPrompt>` passes, where a `Host::new` that returns a `HostBuilder` fails.

### Call sites

- **Four `Host` literals** hold `prompts: Arc::new(FindTextPrompt::new(..))`: the example, `support.rs` of the facade suite, `run.rs` of the test Plugin's suite, and the timer test of the Gateway client. Each becomes `prompts: FindTextPrompt::new(..)`. Rust converts an `Arc<FindTextPrompt>` to an `Arc<dyn PromptFinder>` at a field of a struct literal, as it does at a function argument.
- **The six unit tests of the two finders** call `finder.find(..)`, which goes through the `Arc`, so they stay as they are.

### Tests

- New in [runvil-harness/crates/harness-internal/runner/src/prompt_finders-tests.rs](runvil-harness/crates/harness-internal/runner/src/prompt_finders-tests.rs), `a_host_parses_the_file_of_a_file_prompt_given_as_new_returns_it`: it writes a prompt file, gives `FindFilePrompt::new(..)` to a `Host` as it is, and parses the prompt. Today the tests give a `Host` a text finder, a set, and closures, and they call the file finder by hand. So this test is the one place where what `FindFilePrompt::new` returns meets a `Host`. It builds its `Host` through the helper `host_over` of that file, whose parameter becomes `Arc<dyn PromptFinder>`, so the helper takes a file finder as it takes a set and the count of `Host` literals stays at 10.
- The text finder is covered by the four `Host` sites above: they compile only when `new` returns the `Arc`, and the example and the facade suite run through them.
- Expected count: one more than after commit 5.

### Pages and docs

[runvil-harness/decisions/harness-and-host.md](runvil-harness/decisions/harness-and-host.md), the entry "A prompt that lists other prompts needs a finder that serves each listed name": its paragraph on the stock finders gains two sentences. `FindTextPrompt::new` and `FindFilePrompt::new` return the finder inside an `Arc`, because that is how a `Host` holds its finder, so a Host passes the result on as it is. A `PromptSet` is filled first, and the Host then puts it into an `Arc` and may keep a second handle for `rescan`.

### Listing

The diff of `crates/runvil/public-api.txt` must be exactly two changed lines, those of `FindFilePrompt::new` and `FindTextPrompt::new`, each with `alloc::sync::Arc` around its return type.

## Harness commit 7: `Host::builder`, a `Host` that lives only inside an `Arc`, and `Packs` out of the public API (items 3, 4, 5, 6)

### The new edge

[runvil-harness/crates/harness-internal/runner/Cargo.toml](runvil-harness/crates/harness-internal/runner/Cargo.toml): `[dependencies]` gains `harness-plugin-loader.workspace = true`, with a comment that says what the runner takes from it: the `PluginReport` that a `Host` keeps for each step, and the function that makes one. Commit 8 adds the functions that load a Plugin to that comment. `Cargo.lock` is refreshed with `cargo fetch`, and its diff must be exactly one added line in the dependency list of `harness-runner`. The edge is in this commit because the `Host` keeps the loader's `PluginReport` from the first commit of the builder. The loader depends on `libloading`, `runvil-plugin`, `runvil-plugin-sys`, and `serde_json`, so the two crates form no cycle.

### What changes

```rust
pub struct Host { /* private: its Packs, its finder, and the reports of its Plugins */ }

impl Host {
    pub fn builder(prompts: Arc<dyn PromptFinder>) -> HostBuilder;
    pub fn parse(&self, name: &str) -> Result<Arc<Prompt>, Error>;   // as today
    pub fn packs(&self) -> impl Iterator<Item = &PackInfo>;
    pub fn plugin_reports(&self) -> &[PluginReport];
}

impl HostBuilder {
    pub fn with_rust_plugin(
        self,
        install: impl FnOnce(&mut PackRegistry) -> Result<(), Error> + Send + 'static,
    ) -> HostBuilder;
    pub fn build(self) -> Arc<Host>;
}
```

- **The builder is a description, and `build()` does the work.** `with_rust_plugin` stores its function. `build()` makes an empty `PackRegistry`, calls each stored function with it in the order written, turns the registry into the Host's Packs, which adds the two built-ins, and returns the `Host` inside an `Arc`.
- **Each `with_rust_plugin` step makes one `PluginReport`,** and `Host::plugin_reports()` returns the reports in the order written. The report of a Rust Plugin holds no Plugin, because a Rust `install` leaves no `Plugin` object behind. It is empty when the function returns `Ok`, and it holds the function's error as its one failure otherwise. `build()` makes it with the loader's `make_report`.
- **A function that returns an error leaves the `Host` built.** The Packs that the function inserted before it failed stay.
- **`build()` logs each report as it makes it,** through one private function in `host.rs`. It logs each Plugin of the report at the info level, with its name, its version, and its source. It logs each failure at the warn level, with the failure's `Path` fact as the field `path` when it has one, and the error. In this commit a report holds at most the one failure of a Rust Plugin. No test reads these lines, as no test reads the lines that the Harness logs today.
- **`Host::packs()` lists every Pack of the Host** in the order of their names, the built-ins and a Pack inserted as unavailable among them.
- **The fields of `Host` are private.** So `build()` is the one way to get a `Host`, and every `Host` is inside an `Arc`. The set of Packs is fixed from that moment, because a private field is out of reach even for the one holder of the `Arc`.
- **`Packs` and `Packs::new` become `pub(crate)`,** and both re-exports go. The runner keeps the type for its own work.
- **The name is `builder`** because clippy's `new_ret_no_self` refuses a `new` that returns another type than its own.
- **The function is `Send + 'static`** because the builder keeps it until `build()`. `Send` lets an application move its builder to a blocking thread, which matters from commit 8 on, when `build()` opens libraries.

All of it is in [runvil-harness/crates/harness-internal/runner/src/host.rs](runvil-harness/crates/harness-internal/runner/src/host.rs). Each chained method has `#[must_use]`, and `HostBuilder` gets a hand-written `fmt::Debug`.

### Call sites

- **The 10 `Host` literals become builder chains:** three in the runner's unit tests, three in the runner's suites, and one each in the facade suite, the example, the test Plugin's suite, and the Gateway client's timer test.
- **`Packs::new(`, 29 calls:** the 6 in the unit tests of `packs.rs` stay, because those tests are inside the crate and test `Packs` itself. The 3 in the unit tests that build a `Host` go with their literals, and so do the 20 outside the crate.
  - In the runner's unit tests, `host` in `host-tests.rs` names its three Packs in one `with_rust_plugin` function, and `host_over` in `prompt_finders-tests.rs` is `Host::builder(prompts).build()`. Both return the `Arc<Host>`.
  - In the runner's suites, 17 helpers name `Packs` in their signature, as a parameter or as what they return. Each names the `Host` instead: a helper that built a `Packs` ends with `host_over(packs)`, and `launch`, `report_of`, `prepare_and_drive`, and `drive_over` take the `Host`.
  - Those suites hold three `Host` literals. `host_finding(packs, prompts)` moves from `harness-execute.rs` to `support.rs`, takes the filled `PackRegistry` and a finder, and is the one helper that hands a registry to the builder: one `with_rust_plugin` function puts that registry in the place of the builder's empty one. `host_over(packs)` calls it with an empty `PromptSet`. The third literal, in `harness.rs`, has a finder of its own and the built-ins alone, so it becomes `Host::builder(finder).build()`. Each insert keeps its `unwrap()`, so a refused Pack still fails its test at once.
  - In the facade suite, which shows the API as a Host application uses it, `launch(source, setup)` builds a Host with the built-ins alone, and `no_packs` goes. The one test with a Pack of its own calls a second helper, `launch_with(install, source, setup)`, with an `install` function that inserts its Pack.
  - `host_offering` in the test Plugin's suite fills its registry with `insert_plugin_dir`, as today, hands it over the same way, and keeps returning the `PluginReport` until commit 8.
- **The example** reads as shown under The API after this plan.

### Tests

In [runvil-harness/crates/harness-internal/runner/src/host-tests.rs](runvil-harness/crates/harness-internal/runner/src/host-tests.rs), whose nine parse tests move to the builder, four new tests:

- `a_host_built_with_no_plugin_has_the_two_builtin_packs_and_an_empty_list_of_reports`: `packs()` yields `runvil/subagents` and `runvil/user`.
- `rust_plugins_run_in_the_order_written_and_the_first_keeps_a_pack_id`: two functions insert `acme/web` and `runvil/web`. The Host has `acme/web`. Its first report is empty, and its second holds one failure with the reason `pack_id_taken`.
- `a_rust_plugin_that_fails_is_the_failure_of_its_report_and_its_earlier_packs_stay`.
- `a_pack_inserted_as_unavailable_is_among_the_hosts_packs`.

Expected count: four more than after commit 6. `cargo run -p runvil --example run-prompt` must print `Hello, desk.`.

### Pages and docs

In [runvil-harness/decisions/harness-and-host.md](runvil-harness/decisions/harness-and-host.md):

- **The entry "A Host builds one `Host` at startup and one `RunSetup` for each run"** gets the heading "A Host builds one `Host` at startup, through `Host::builder`, and one `RunSetup` for each run" and this text:

```markdown
`Host::builder(prompts)` takes the Host's prompt finder and returns a builder. A `with_` method of the builder names one Plugin, and `build()` installs the Plugins in the order written, keeps one report for each, and returns the `Host` inside an `Arc`. The fields of `Host` are private, so `build()` is the one way to get a `Host`. `RunSetup` is a struct of public fields that the Host fills for each run.

A `Host` lives inside an `Arc` because every run shares it: `Harness::new` takes the `Arc`, and `Host::parse` only reads. Its Packs are fixed when `build()` returns, so the `uses:` check of a parse and the Packs of a later run agree.

`Host` contains what a parse reads and what every run shares. `RunSetup` contains what one run uses, which includes the services a run waits on: the recorder, the broker, and the timer.
```

- **The opening paragraph, "A prompt names roles and Pack ids, and the Host binds both", and "A Host depends on one crate, `runvil`":** the Host's Plugins supply the Packs, `HostBuilder` joins the items at the crate root, and a Rust Plugin is named on the builder.
- **"The Host hands a Rust Plugin what its Packs need as the arguments of `install`":** the Host names the Plugin with `with_rust_plugin`, as a function that is lent the registry, and `build()` calls it.

In [runvil-harness/decisions/tools-and-packs.md](runvil-harness/decisions/tools-and-packs.md):

- **"A declared Pack can be refused at install, at parse, and at the run's first step":** a Rust Plugin whose function returns an error is the one failure of its report, which the `Host` keeps.
- **"A Rust Plugin is a function named `install`, and its arguments are its own":** how the Host passes the registry to it.

The crate docs of the facade and of the runner, the runner's `README.md`, and the module doc of `packs.rs` describe the builder where they described the struct literal and `Packs`. Every doc link to `Packs` goes, because the rustdoc lint for links to private items is set to deny.

### Listing

The diff must be exactly this:

- **Removed:** the two field lines of `Host`, and the four lines of `Packs` (its struct line, `Packs::new`, its `auto` line, and its `derives` line).
- **Changed:** the struct line of `Host`, which shows private fields.
- **Added:** `Host::builder`, `Host::packs`, `Host::plugin_reports`, and five lines for `HostBuilder` (its struct line, its `auto` line, its `derives` line, `with_rust_plugin`, and `build`).
- The `auto` line of `runvil::Host` must still hold `Send` and `Sync`, because Workshop moves its `Arc<Host>` to a blocking thread to parse.

## Harness commit 8: the builder loads the Plugins of the C contract, and the `Host` keeps their reports (items 5, 6, 7, 12)

### The second new edge

[runvil-harness/crates/harness-internal/runner/Cargo.toml](runvil-harness/crates/harness-internal/runner/Cargo.toml): `[dependencies]` gains `runvil-plugin-sys.workspace = true`, with a comment that says what the runner takes from it: the type of an entry point. `Cargo.lock` is refreshed with `cargo fetch`, and its diff must be exactly one added line in the dependency list of `harness-runner`.

### What changes

```rust
impl HostBuilder {
    pub fn with_resources(self, resources: HostResources) -> HostBuilder;
    pub fn with_plugin_dir(self, directory: impl Into<PathBuf>) -> HostBuilder;
    pub fn with_plugin_file(self, path: impl Into<PathBuf>) -> HostBuilder;
    pub fn with_plugin(self, entry: PluginEntry) -> HostBuilder;
}
```

Inside `build()`, each step that names a Plugin becomes one report:

```rust
let report = match step {
    Step::Rust(install) => make_report(install(&mut packs)),
    Step::Dir(directory) => insert_plugin_dir(&mut packs, &directory, &resources)
        .unwrap_or_else(|cause| make_report(Err(make_directory_error(&directory, &cause)))),
    Step::File(path) => insert_plugin_file(&mut packs, &path, &resources),
    Step::Entry(entry) => insert_plugin(&mut packs, entry, &resources),
};
reports.push(report);
```

- **Each method stores one step, and `build()` runs the steps in the order written,** the Rust Plugins among them. So the first Plugin that inserts a Pack id keeps it, whatever kind of Plugin it is.
- **Each step that names a Plugin makes exactly one `PluginReport`,** and `host.plugin_reports()` returns them in the order written. So the third report belongs to the third such step. `with_resources` names no Plugin and makes no report.
- **Every kind is reported in one way.** A Plugin of the contract that loaded is in the report of its step, whether it came from a directory, from a file, or from an entry point. A library that did not load, an entry point that refused, and a Pack that the registry refused are each one failure of that report. After a failure, `build()` goes on with the next step.
- **`with_resources` applies to every Plugin of the contract that the builder loads,** wherever in the chain it stands, because the loads happen in `build()`. A second call replaces the first, and a builder without one offers an empty `HostResources`.
- **A path is read when `build()` runs.** A relative directory or file resolves against the working directory of that moment.
- **Each step is one call of the loader.** So a Plugin name is unique within one directory, as today, and two steps may each load a Plugin of the same name. The registry then refuses the Packs of the second one, and each refusal is a failure.
- **A directory that cannot be read is a new failure,** and the one failure of its step's report: a `PluginLoadFailed` error with the `Reason` fact `directory`, the directory as its `Path` fact, and the system's reason as its cause. `insert_plugin_dir` returns a `std::io::Error` for it. The runner makes the error itself, with `Error::new`, the prose of the code, and `with_cause`, because the loader's function that makes the other failures is private to the loader.
- **The `Host` holds each Plugin it loaded, through the reports,** so a Plugin that inserted no Pack stays loaded for as long as the `Host` lives. Today the application keeps the `PluginReport` for that.
- **`build()` logs what every step loaded and what failed,** through the function of commit 7. You chose this in the review. A Host application with a `tracing` subscriber then sees a load that failed, even when it never reads the reports.
- **`insert_plugin` leaves the facade.** The re-export in [runvil-harness/crates/runvil/src/lib.rs](runvil-harness/crates/runvil/src/lib.rs) goes, because `with_plugin` is now how a Host application loads an entry point. The function stays in the loader crate, where `build()` and the loader's unit tests call it. `insert_plugin_dir` stays in the facade for code that fills a registry apart from a `Host`.
- **`build()` runs foreign code,** and its doc says so in the words that the doc of `insert_plugin_dir` uses: a Host names only libraries and entry points that it trusts.

### Tests

The suites of the test Plugin have the fixture library, so the tests are there.

- [runvil-harness/crates/harness-internal/test-plugin/tests/it/run.rs](runvil-harness/crates/harness-internal/test-plugin/tests/it/run.rs): `host_offering` becomes one chain with `with_plugin_dir` and then `with_resources`. The test that reads the resources then also shows that `with_resources` may stand after the directory. The helper returns the `Arc<Host>` alone, the assertions on the report read `host.plugin_reports()`, and the unload counts are read after the `Host` is dropped.
- A new file `host.rs` in that suite, seven tests:
  - `a_host_over_a_plugin_directory_has_its_plugin_and_its_pack`.
  - `a_plugin_directory_that_cannot_be_read_is_one_failure_and_the_host_is_built`.
  - `a_library_named_by_its_file_loads_as_one_found_in_a_directory_does`.
  - `an_entry_point_given_to_the_builder_loads_as_a_linked_plugin`. It opens a copy of the fixture library with `libloading`, which the suite already uses to read the fixture's counters, takes the symbol `runvil_plugin` from it, and keeps the library open until the `Host` is dropped. The parameter of `with_plugin` fixes the type of the symbol, so the test names no type of the C contract, the suite's dependencies stay as they are, and the diff of `Cargo.lock` stays at one line.
  - `a_library_whose_pack_id_an_earlier_rust_plugin_took_is_a_failure_and_stays_loaded_with_the_host`.
  - `an_entry_point_whose_pack_id_an_earlier_rust_plugin_took_is_reported_as_a_library_is`. Two Hosts name the same Rust Plugin first, and then the fixture, one as a file and one as an entry point. Each report of the fixture holds the Plugin and one failure with the reason `pack`. The two reports differ in the Plugin's source and in the `Path` fact alone. This is the test of what you asked for.
  - `the_host_keeps_one_report_for_each_step_in_the_order_written`: a Rust Plugin, a directory, and a file give three reports in that order.
- Expected count: seven more than after commit 7.

### Pages and docs

- [runvil-harness/decisions/harness-and-host.md](runvil-harness/decisions/harness-and-host.md): "A Host depends on one crate, `runvil`" says that a Host names its Plugins on the builder, and that the facade keeps one function of the loader, `insert_plugin_dir`, for code that fills a registry apart from a `Host`, such as a tool that lists a Plugin directory. "`Host::parse` parses the prompt before a run exists" says that the reason of a failed load is in the Host's `plugin_reports()`. "The Host fills one `HostResources`" says that the Host passes it to the builder with `with_resources`. A new entry follows it, "`build()` loads every Plugin in the order written, and the `Host` keeps one report for each step", with the rules of the list above and the reason for each.
- [runvil-harness/decisions/tools-and-packs.md](runvil-harness/decisions/tools-and-packs.md): "Every tool call is lent the Host's resources as JSON text" names `with_resources`. "A library Plugin and a C entry point compiled into the Host receive the same calls" names the three builder methods as the ways a Host loads each kind. It says that a loaded Plugin stays loaded while a `PluginReport`, one of its Packs, or a run's instance of one of its Packs holds it, and that a `Host` keeps the report of each of its steps.
- **The item "One Plugin API"** is in the planned work at the end of both pages. Each says that a Host inserts a Plugin of the contract with `insert_plugin`, and each says `with_plugin` after this commit.
- [runvil-harness/decisions/testing-and-debugging.md](runvil-harness/decisions/testing-and-debugging.md), "The Harness reports operational problems through `tracing`, and Workshop discards those lines": a Plugin that failed to load joins the example of that entry, and the entry says that `build()` logs each loaded Plugin too.

### Listing

The diff must be exactly four added lines, the four builder methods, and one removed line, `insert_plugin`.

## Workshop: one commit that pins the Harness and moves to the builder

Workshop builds against the new Harness only with every change below, so they are one commit.

### Production code, all in [runvil-workshop/crates/workshop/server/src/agents.rs](runvil-workshop/crates/workshop/server/src/agents.rs)

- **`host_packs` and `with_plugins`** return `(Packs, Option<PluginReport>)` today, and the caller writes the `Host` literal. `host_packs` becomes `server_host(prompts, registry, mcp, plugins)`, and `with_plugins` gains the finder as its first parameter. Each returns the built `Arc<Host>`, and the body of `with_plugins` is the chain shown under The API after this plan.
- **`agent_finder`** returns a bare closure today, and `AgentSessions::new` wraps it in `Arc::new`. It returns `Arc<dyn PromptFinder>`, as the two stock finders of the Harness return an `Arc` from commit 6 on. So `AgentSessions::new` passes `agent_finder(agents_path.clone())` as it is.
- **`load_plugin_dir` goes.** It loads and logs today. The builder loads, and `build()` logs each loaded Plugin and each failure through `tracing`. The line "the mcp Plugin inserted no Pack" in `with_plugins` goes for the same reason. Workshop has no `tracing` subscriber, so its process discards these lines today and discards the Harness's lines after the change. `runvil-workshop --plugins` stays the way a person sees what a Plugin directory loads.
- **A Plugin directory that does not exist** stays a state, as today: `with_plugins` leaves a directory out of the builder when `try_exists` reports that it is missing. Every other directory is named, so one that cannot be read is the failure of its step's report, which `build()` logs.
- **`holds`** asks the built `Host`: `host.packs().any(|info| info.fqn == pack)`. The warning for a shipped Pack that is missing runs after `build()`.
- **`Inner` loses `_plugins`.** Its doc says that the field keeps a Plugin that inserted no Pack loaded, and the `Host` does that now.
- **`setup_for`** becomes one chain: `RunSetup::new(recorder, broker, Arc::new(TokioTimer)).vfs(language_guide::run_vfs()).has_user(true)`.
- **`list_plugin_dir` stays as it is.** It fills a registry apart from any `Host` through `insert_plugin_dir`, and `runvil-workshop --plugins` prints what it returns.
- **The docs:** the module doc, the doc of `with_plugins`, and the doc of the `host` field say that the timer is part of each run's setup and that every Host has the built-ins.

### Tests

- [runvil-workshop/crates/workshop/server/src/agents/tests.rs](runvil-workshop/crates/workshop/server/src/agents/tests.rs): `run_prompt`, `run_prompt_over`, and `run_agent_on` take the built `Arc<Host>`, and the first two lose their `source` parameter. The closure that gives one source under every name moves out of `run_prompt_over` into a helper, `serving(source)`, which returns it inside an `Arc`. The eleven tests that get their Packs from `host_packs` or `with_plugins` pass `serving(source)` to that call, and those that read the returned report read `host.plugin_reports()`. The four that call `load_plugin_dir` build a `Host` and assert on `host.plugin_reports()`. The test of `list_plugin_dir` stays as it is. `the_finder_refuses_an_unknown_agent_as_not_found_and_an_unreadable_one_as_io` calls `finder.find(name)` where it called the closure, and the test that builds a `Host` over `agent_finder` passes what the function returns as it is.
- [runvil-workshop/crates/workshop/server/src/agents/language_guide/tests.rs](runvil-workshop/crates/workshop/server/src/agents/language_guide/tests.rs): its four tests change as the eleven do.
- [runvil-workshop/crates/workshop/server/src/agents/bindings-tests.rs](runvil-workshop/crates/workshop/server/src/agents/bindings-tests.rs) and [runvil-workshop/crates/workshop/agents/tests/it/support.rs](runvil-workshop/crates/workshop/agents/tests/it/support.rs): `RunSetup::new` gains `Arc::new(TokioTimer)`, and the fixture `Host` is one builder chain. In `support.rs` that chain starts with `Host::builder(FindTextPrompt::new(AGENT, source))`.
- [runvil-workshop/crates/workshop/server/tests/it/std_real_directory.rs](runvil-workshop/crates/workshop/server/tests/it/std_real_directory.rs) stays as it is. It fills a registry with `insert_plugin_dir` and takes a factory from `into_entries`, and both stay public.
- Expected counts: 706 and 353, the same as before. A different count gets a line in the record.

### The pin

[runvil-workshop/Cargo.toml](runvil-workshop/Cargo.toml), lines 28 to 30: the three `rev` values become the Harness head. Then `cargo fetch`. `Cargo.lock` must change its 14 source lines for `runvil-harness`, gain two lines in the dependency list of `harness-runner`, and stay as it is everywhere else.

### Dry run, before the Harness is pushed

- Add this temporary section at the end of Workshop's root `Cargo.toml`. It makes Workshop build against the local Harness checkout, and it stays out of every commit:

```toml
[patch."https://github.com/cppalliance/runvil-harness"]
runvil = { path = "../runvil-harness/crates/runvil" }
harness-gateway-client = { path = "../runvil-harness/crates/harness-gateway-client" }
plugin-mcp = { path = "../runvil-harness/crates/plugin-mcp" }
```

- If cargo warns that a patch was not used, a path or a version is off. Both workspaces are at version `0.3.0` today.
- Make the edits above.
- Run clippy with warnings denied and nextest for `-p workshop-agents -p workshop-server`, without `--locked`, because the patch changes the lock file.
- Restore the root `Cargo.toml` and `Cargo.lock` with `git restore`. The edits stay in the working tree for the real commit.

### The real commit, after the Harness pull request is open

- Set the pin, run `cargo fetch`, then `verify-workshop.ps1 -Tag w1`, the whole list of Workshop's `AGENTS.md`.
- Commit, `git push origin master`, and open the pull request. Its body names the Harness pull request and says that it merges second.

## Verification and commit messages

- **Harness:** `cabinet/_scratch/remove-ui-build/verify.ps1 -Tag <tag>` runs the whole list of the Harness's `AGENTS.md`. Every line must read `exit=0`, and the test summary must show the expected count.
- **Workshop:** `verify-workshop.ps1 -Tag <tag>` beside it does the same for Workshop, structural checks included.
- **Commit messages:** each one is written from the staged diff by `.cursor/rules/commit.mdc`, saved in the scratch folder of this run, and checked with `check-message.ps1 -Message <file> -Repo <path>`.
- **Three rules for the new docs,** because checks enforce them. A crate doc with `## Invariants` writes Host and Plugin with a capital and puts each method name in backticks. An example in a doc comment uses a `text` fence, because the structural checks ban a compiled code block there. The docs of the builder leave the names of the internal crates out, because the listing check refuses them in the facade's surface.
- **While a verification runs,** which takes about ten minutes, I draft the commit message from the staged diff. During the verification of commit 8, I also make the Workshop edits, whose first build is the dry run.

## Record

After each commit I append what it found to a scratch notes file, `notes.md` in the scratch folder `host-builder-cleanup`: the test count, the diff of each listing, each tightening beyond the known list, and each thing that I saw and left, with the reason. The record is written from that file, so it holds what was seen at the time, even when the agent's context has been summarized in between.

Harness commit 9 adds a section "What the build found" to the repo copy of the plan and sets the todo statuses. It covers the seven Harness code commits and the Workshop dry run. Its form is that of the last record, in `plan/2026-11-20-3-stock-tokio-timer.plan.md`: one heading for each commit with its id, and under it the points that differ from the plan or add to it. If a review leads to one more commit, a section "After the pull requests opened" is added with it, as that record did. The approved Cursor plan file stays as it is. The pull request numbers, the CI results, and the merges come after this commit, so I report them in chat.

## Publishing, in order

1. Harness: run `cargo doc -p runvil --no-deps` with `RUSTDOCFLAGS="-D warnings"` and `cargo deny check`, which CI runs beside the list. Then `git push origin master`, and `gh pr create --repo cppalliance/runvil-harness --base master --head vinniefalco:master`. Its body has the sections of the last one, which `gh pr view 59 --repo cppalliance/runvil-harness` shows: a first paragraph on what a Host can now do, then Commits, Why, What a Host will notice, Call sites, Tests, Checks, Companion changes, and Follow-up.
2. Workshop: the real commit above, then `git push origin master` and `gh pr create --repo cppalliance/runvil-workshop --base master --head vinniefalco:master`. Its body has the sections of `gh pr view 31 --repo cppalliance/runvil-workshop`: a first paragraph, the merge order in bold, then The commit, What changes, Call sites, Tests, and Checks.
3. Wait until every check on both pull requests has passed: the CI workflow with `ci-green` on both, and Workshop's Bundle workflow. `gh pr checks <number> --repo cppalliance/<repo>` shows them. While they run I read the automatic reviews of CodeRabbit, and a finding that is right gets a fix as a new commit on the fork. The merges wait for the checks of the last commit on each pull request, so a fix starts the wait again.
4. Merge the Harness: `git fetch upstream`, confirm that `upstream/master` is still the commit this work started from, then `git push upstream master`, and confirm the new head with `gh`.
5. Merge Workshop the same way.
6. Confirm that each fork equals its upstream, and report the CI run that each push starts on upstream `master`.

Your approval of this plan is the go-ahead for both merges. When step 3 is done, steps 4 to 6 follow directly, and Workshop merges as soon as the Harness merge is confirmed. Each merge is a fast-forward push, so upstream `master` ends at the exact commit that its pull request tested.

## The API report (output)

You asked for a report that a follow-up plan can start from. The finished report is **output**, named `host-builder-api-review.md`, and it is the last step of this plan.

**Who it is for.** A reader who is new to this code and has seen none of the conversation behind this plan. So the report explains each term where it first uses it, shows code for each claim, and names the file that holds each thing it describes.

**When it is written.** I draft it while the checks of the two pull requests run, and I finish it after both merges, so that it names the merged commits and the two pull requests. If a fix during the checks changes the API, the draft changes with it.

**What it is written from.** The code as merged, the API listing, the decision pages, the repo copy of this plan, and the scratch notes of the build. Each number in the report comes from a search that I run at that time. The section "What the review found" below holds the findings of the review of 2026-11-21, so that the report stands on this file and on the code.

### What it holds, in this order

1. **Summary.** One page: what changed, the strongest points, the limits that matter most, and what to do first.
2. **The words.** Engine, Harness, Host application, the `Host` type, prompt, Pack, Plugin and its three kinds (a library, a linked entry point, a Rust Plugin), the C contract, the Pack registry, finder, recorder, broker, timer, Gateway, Workshop, and the facade crate `runvil`. Each gets one or two sentences and the crate that holds it.
3. **How a prompt runs.** The steps from building a `Host` to reading a `RunReport`, with the repository's example program, a note on each of its lines, and one diagram.
4. **Before and after.** The smallest Host and Workshop's startup, each in both forms, and the list of what left the public API and what joined it, with the reason for each.
5. **The API in full.** Each public item of `Host`, `HostBuilder`, `PluginReport`, `RunSetup`, and the stock finders, with the rule that it follows: the order of the steps, which Plugin keeps a Pack id, what a report holds, the facts of a failure, and what `build()` logs.
6. **Strengths.** Each with its evidence from the code or from the build.
7. **Limits and rough spots.** For each one: what it is, a short piece of code that shows it, whom it affects, and whether a later change can fix it without breaking a Host application.
8. **Advice for the follow-up plan.** The candidates in the order I would take them. For each one: the problem, the options with a sketch of the code, what each option touches with counts from a search, its risk, and a recommendation with a confidence level.
9. **Decisions already made, and why.** The choices of this plan and of its review, so that the follow-up plan reopens one only on purpose.
10. **What the build found.** What surprised me, and the real numbers.
11. **Where to look.** A map from each topic to its file in the two repos.

### What the review found

Each point is checked against the merged code before it goes into the report.

**Strengths:**

- A `Host` is built in one statement, where the struct literal takes five lines, and it exists only inside an `Arc` with its Packs fixed.
- A `Host` that only parses needs no timer, and each run can have its own clock.
- One `PluginReport` serves a directory, a file, an entry point, and a Rust Plugin. The `Host` keeps each report and, through it, each loaded Plugin.
- `Packs`, `PackOrigin`, and `insert_plugin` leave the public API, and a single library file can be loaded by its path.

**Rough spots:**

1. `RunSetup::new` takes three values that the caller wraps in `Arc::new`, right below a `Host` line that has none. `FindTextPrompt::new` returns an `Arc`, and `MemoryRecorder::new` returns a value.
2. A finder has three spellings. A one-prompt finder goes in as it is, and a `PromptSet` or a closure needs `Arc::new`. `Arc::clone(&prompts)` fails to compile at the builder, where `prompts.clone()` works.
3. A conditional step breaks the chain into `let mut builder` and an assignment.
4. A failed load is quiet for a Host application without a `tracing` subscriber, and Workshop is one.
5. A report is found by its position, which shifts when a step is conditional. A report has no label, and the report of a Rust Plugin never lists a Plugin.
6. A Rust Plugin needs a wrapper closure with owned values, and the bare name `with_plugin` belongs to the rarest case.
7. A directory has two public ways in, `with_plugin_dir` and `insert_plugin_dir`, and they report a directory that cannot be read in two ways.

**What the builder prevents,** and the free functions allowed:

1. Different resources for different Plugins. It matters because resources lend the Gateway's address and key to every tool call.
2. Stopping at the first failure, or choosing a load from the outcome of the one before.
3. Borrowed or non-`Send` arguments for the function of a Rust Plugin.
4. Loading an entry point without building a `Host`.
5. Several Hosts from one builder. Each `Host` loads its own copy of each library.

**More candidates for the follow-up:** a `RunSetup::new` without `Arc::new`, a `tracing` subscriber or a visible list of reports in Workshop, the move of `plugin-mcp` onto the C contract, launching a prompt by name from the `Host`, and private fields for `RunSetup`.

### How it is written

- **The folder is checked first.** If a file with the prefix `host-builder-api-review` exists, I ask you whether to overwrite it or to number a new one.
- **It is written in chunks,** one group of sections at a time.
- **Its form follows the rules for output:** no YAML front matter, the date, the time, and the model in italics on the last line, lines without hard wraps, and plain conversational English. It names files by their path in the two repos, pull requests by their links, and this plan by its repo copy.
- **Each recommendation has a confidence level** and a one-phrase reason.
- **One cold read tests it.** A fresh subagent that has seen none of the work reads the report and lists each term, step, or claim that it could not follow. I fix each one. Its list is scratch.

### Done means

The report stands alone. A reader who starts from it can find each thing in the code, and can turn each candidate of its advice into a step of a plan.

## When to stop or change course

- **A check fails on a pull request.** I fix it with a new commit on top of the fork's head. If the new Harness commit changes code, Workshop moves its pin to it.
- **Upstream `master` moved in either repo,** for example because #60 or #32 merged. I stop and tell you, because a rebase rewrites commits that the fork already holds.
- **A check refuses the runner's dependency on the loader,** which commit 7 adds. Commits 1 to 6 stand, and I stop and tell you before I change the approach.
- **The `auto` line of `runvil::Host` loses `Send` or `Sync`.** I stop, because Workshop needs both.
- **A listing diff or a `Cargo.lock` diff holds a line that this plan did not name.** I find the reason before I go on, and the record says what it was.

## What each step needs

- **The first step** needs two clean trees and the upstream head.
- **Commit 1** is independent of the other commits. The issue close and commit 2 need commit 1, because the record names its test.
- **Commit 3** is independent of commits 1 and 2.
- **Commit 4** is independent of commit 3. It comes before commit 7 so that the built-ins are added in one place when the builder arrives.
- **Commit 5** is independent of commits 3 and 4.
- **Commit 6** is independent of commits 3 to 5. It comes before commit 7 so that the builder commit writes each `Host` site in its final form.
- **Commit 7** needs commit 3, because the builder takes the finder alone. It needs commit 5, because the report of a Rust Plugin comes from the loader's `make_report`. It comes after commit 4 by choice: both rewrite the same doc lines about the built-ins and `Packs`, and this order writes each line once.
- **Commit 8** needs commit 5 for the loader's functions and commit 7 for the builder.
- **The Workshop dry run** needs commits 3 to 8 in the local Harness checkout.
- **Commit 9** needs the results of the seven code commits and of the dry run.
- **The Workshop commit** needs the id of the Harness head, an open Harness pull request so that the id can be fetched, and the edits that the dry run left in the working tree.
- **The merges** need every check green on both pull requests and two upstream branches that have stayed where they were. Workshop's merge needs the Harness merge.
- **The API report** needs the merged code of both repos, the notes file, and the review findings that this plan holds. Its draft needs only the two open pull requests.

The commits run one after the other. Each verification is a full build on one machine, and each commit changes files that the next one reads. Three things run beside a verification: the issue close, the draft of the next commit message, and, during the verification of commit 8, the Workshop edits. The draft of the API report is written while the checks of the two pull requests run.

## Choices I settled

- **One pull request for each repo, the Harness first** (confidence: medium - the last three plans went this way, and Workshop needs every Harness change at once to build).
- **The API report is the last step, drafted while the checks run and finished after the merges** (confidence: medium - it then describes the code that is on `master` and names the merged commits, and the wait for the checks is otherwise idle).
- **The report is named `host-builder-api-review.md`** (confidence: medium - the naming rule for output asks for a two-word slug from the plan and then the subject).
- **One cold read by a fresh subagent tests the report** (confidence: medium - it is the direct test of your requirement that a reader who is new to the code can follow it).
- **Both merges follow the green checks directly, and Workshop merges as soon as the Harness merge is confirmed** (confidence: medium - you asked for the same when the last plan was published, and the CI run that the Harness push starts on upstream `master` builds the tree that passed on its pull request).
- **The timer is the third argument of `RunSetup::new`, and `new` alone sets it** (confidence: high - a setter would be a second way to supply a required argument, which is the rule for the recorder and the broker).
- **`drive_run` reads the timer from the setup** (confidence: high - a timer parameter beside `setup.timer` would let the two differ).
- **The runner's suites name the timer in their setups, and `launch` loses its timer parameter** (confidence: medium - 61 call sites change, but a helper that overwrote the setup's timer would leave an unused value in every setup).
- **A built-in's Pack id is refused with the existing reason `pack_id_taken`** (confidence: medium - the registry keeps one rule for a taken id, at the cost that an insert of `runvil/user` itself changes its reason).
- **The list of built-in names is private in `runvil-plugin`, and a runner test pins that the registry refuses each built-in** (confidence: medium - a shared public list would add an item to the Engine's surface).
- **`Host::builder`, where your sketch had `Host::new`** (confidence: high - clippy refuses a `new` that returns another type).
- **The builder stores each step and `build()` runs them** (confidence: medium - `with_resources` then applies wherever it stands, and `build()` is the one call that runs foreign code, at the cost of the `'static` bound).
- **The function of `with_rust_plugin` is `Send`** (confidence: medium - opening libraries is blocking work, so an application can move its builder to a blocking thread).
- **`Host::packs()` is a new method that the design conversation did not name** (confidence: medium - Workshop's warning for a missing shipped Pack reads the registry today, and after the change the built `Host` is what there is to ask).
- **`insert_plugin_file` stays out of the facade** (confidence: medium - the repo's rule is that a new facility needs a benefit beyond tidiness, and the builder method is the one caller that a Host application needs).
- **`insert_plugin_file` returns a `PluginReport`** (confidence: medium - a file that is named by itself then behaves exactly as a file found in a directory, and the two share one code path).
- **The facade keeps `insert_plugin_dir`** (confidence: medium - Workshop's `--plugins` listing and one of its tests fill a registry apart from a `Host`).
- **A directory that cannot be read is a failure in the report of its step, and Workshop leaves out a directory that is known to be missing** (confidence: medium - a mistyped directory should show up in a Host that names it, and Workshop treats a missing one as a state, as it does today).
- **The runner makes the `directory` failure itself** (confidence: medium - it is about six lines, and the other way is a new public function in the loader for one caller).
- **The `Host` keeps one `PluginReport` for each step, and `build()` returns the `Host` alone.** You chose this in the review, over a `build()` that returns the reports beside the `Host` and over two flat lists.
- **`insert_plugin` returns a `PluginReport` and leaves the facade.** You chose this in the review. It reverses one line of the design note of 2026-10-20, whose reason held while a Plugin inserted no Packs.
- **A Rust Plugin has a report too** (confidence: medium - every step that names a Plugin then has exactly one report, at the cost of one small function in the loader, because the fields of `PluginReport` are private to it).
- **`insert_plugin` changes its return type in commit 5 and leaves the facade in commit 8** (confidence: medium - every commit then leaves the facade able to load an entry point, at the cost that its line in the listing changes once and is removed once).
- **The runner's dependency on the loader is in commit 7** (confidence: medium - the `Host` keeps the loader's `PluginReport` from the first commit of the builder, so the edge cannot wait for commit 8).
- **A Plugin name is unique within one directory and not across the builder's steps** (confidence: medium - each step is one call of the loader, and the registry already refuses the Packs of a second copy).
- **Workshop's function is `server_host`** (confidence: medium - it returns the server's Host, and `host_over` is a helper of the Harness's suites that means something else).
- **`build()` logs each loaded Plugin and each failure through `tracing`, and Workshop's own logging of them goes.** You chose this in the review. Workshop has no `tracing` subscriber, so its process discards these lines before and after the change.
- **A loaded Plugin is logged at the info level and a failure at the warn level** (confidence: medium - these are the levels of Workshop's lines today, and the Harness logs a Pack that was inserted as unavailable at the warn level).
- **Items 3 and 4 share commit 7 with the first half of the builder** (confidence: medium - a public `Packs` that no public function accepts would be unused surface for one commit).
- **The builder stays two commits** (confidence: medium - commit 7 moves every Host to the builder with Rust Plugins alone, and commit 8 adds the Plugins of the C contract with their tests, so each is reviewed alone).
- **The change to the finders is a commit of its own, right before the builder** (confidence: medium - it changes two return types and two listing lines by itself, and this order lets the builder commit write each `Host` site in its final form).
- **That commit gets one new test, of a `Host` over a file finder** (confidence: medium - the text finder meets a `Host` in four places already, and the file finder meets one in this test alone).
- **Workshop's `agent_finder` returns the `Arc` too** (confidence: medium - it is the same idea applied to Workshop's own finder, at the cost that one test calls `find` where it called the closure).
- **In the runner's suites the helpers return the `Host`, and one helper hands a filled registry to the builder by assignment** (confidence: medium - each helper keeps its body and its `unwrap()`, at the cost of one line that replaces the builder's registry; a loop over `into_entries` is the longer way to say the same).
- **In the facade suite `launch` builds a Host with the built-ins alone** (confidence: medium - seven of its eight calls ask for that today, and the suite shows the API as a Host application uses it).
- **A tightening goes into the commit that changes its call site** (confidence: medium - the plan `2026-11-20-3-stock-tokio-timer` did the same, and a separate tidy commit would touch each line twice).
- **The look stops at the function and at a helper that only it calls** (confidence: medium - a wider sweep would turn an API change into a rewrite of the suites).
- **Workshop is one commit** (confidence: medium - the pin moves the whole API at once).
- **The classroom stays as it is** (confidence: medium - each student program pins an older Harness commit, and the last two plans made the same choice).
- **The plan commit, the record fix, and the record commit skip the verification list** (confidence: medium - they change files under `plan/` alone, which the checks leave unread).

## Not in this plan

- **The follow-up plan itself.** The API report is what it starts from.
- **The classroom and the guide.** The Plugin tutorial in the classroom quotes Workshop's `with_plugins` at a commit that it names, so it stays true to that commit.
- **The move of `plugin-mcp` onto the C contract.** `with_rust_plugin` stays until then, and after it for a Host that inserts a Pack written in Rust.
- **Taking `insert_plugin_dir` out of the facade.** Workshop's `--plugins` listing and one of its tests call it.
- **An `Arc` from `PromptSet::new()`.** You decided against it in the review. A set is filled after `new()` through `&mut self`, and five tests of the Harness are the only code that gives a set to a `Host`.
- **The `Arc::new` around the three arguments of `RunSetup::new`.** The recorder, the broker, and the timer arrive as `Arc`s that the caller makes, as in `Arc::new(MemoryRecorder::new())` and `Arc::new(TokioTimer)`. Your request named the prompt finders, so these read as they do today. The review of the API named this the bump to fix next, as a plan of its own: more than 80 places make a `MemoryRecorder`, and most keep a handle on it or wrap it.
- **Different resources for different Plugins.** Today each load call takes its own `HostResources`, and the builder has one for the whole `Host`. So a Host cannot lend the Gateway to one directory and keep it from another. A later method can add this and leave `with_resources` as it is.
- **A `build()` that stops at its first failure,** or that chooses a load from the outcome of the one before. A Rust Plugin step can read the registry as the earlier steps filled it.
- **One report shape for `insert_plugin_dir`,** which returns an `io::Error` for a directory that cannot be read, where `with_plugin_dir` makes a failure of it.
- **The name `with_plugin`.** The review offered `with_plugin_entry`, and you kept your name.
- **Private fields for `RunSetup`,** and the struct update sites that the changes leave alone.
- **The second review comment on pull request #57,** a test of two renders in one run.
- **Launching a prompt by name from the Host,** which the design conversation set aside.
- **Pull request #27.** Its conflict with `master` grows, which is expected.

## Risks

- **Other people's pull requests.** #60 and #32 touch files that this plan changes. Whichever side merges second has to be adapted.
- **The report is written at the end of a long run,** when the agent's context has been summarized. It is written from the merged code, the notes file, and the review findings that this plan holds, so it stands on those and not on what the context still holds.
- **The test migration is large.** Commit 3 changes 61 calls in the runner's suites, and commit 7 changes 17 helper signatures. The compiler finds each site, and a test that passes for a new reason is the thing to look for.
- **The dependency from the runner to the loader is new.** I read the structural checks, and its first build is the verification of commit 7, which is also the largest commit.
- **The loader's unit tests reach `insert_plugin` in about twenty places.** Three helpers and eight direct calls change shape in commit 5. The tests behind the helpers keep their assertions, and one test changes what it asserts, which is the behaviour that this plan changes.
- **Clippy's pedantic group is on,** and the builder is uncompiled. A lint may ask for a small change of shape.
- **A `new` that returns an `Arc` is unusual.** Clippy's rule about `new` accepts a return type that contains the type itself, by its documentation. The first clippy run of commit 6 shows whether another lint objects.
- **The test of `with_plugin`** opens a copy of the fixture library with `libloading` and takes the entry point from it, which needs an `unsafe` block in a crate that is allowed to have one. Its first run is in commit 8. If it fails to work, the record says how `with_plugin` was tested.
- **Tightening widens each diff.** Commits 3 and 7 change many test sites already, and each tightening is one more edit that has to leave its test's assertions as they are. A doubt leaves the site as it is, and the record lists it.
- **`HostBuilder` is `Send` and lacks `Sync`,** because it holds boxed functions. The listing will show that.
- **The docs have three automatic checks** (the wording scan, the doctest ban, and the surface check on doc text), and commits 7 and 8 write the most new doc text of the plan.
- **Workshop's tests change their shape,** because a finder now goes in before the Packs. I read each helper, and the dry run is their first run.
- **The count of tests after each commit** is the base count plus the new tests that this plan names. A test that I merge or split changes it, and the record gives the real numbers.

# What the build found

The plan held in both repos. This record was written after the seven Harness code commits and the Workshop dry run, and before anything was pushed. These points differ from the plan above, or add to it.

## Before the first commit

- **Upstream `master` had moved.** It was at `f40a9a2e`, four commits ahead of the fork, which held pull request #26. Local `master` was fast-forwarded and the fork was pushed, as the plan's first step says.
- **The base count is 3,979 tests,** all passing, with 25 skipped. Those four commits changed tests and added none.
- **A whole run of the verification list takes about two minutes** on a warm tree, where the plan guessed ten.

## Harness commit 0, `0bf31ef5`: the plan

- **The copy is the approved text, with every todo `pending`.** The Cursor plan file showed the first todo as `in_progress` when the copy was taken, so that one line of the copy was set back.

## Harness commit 1, `72b95a37`: a test broker that lists two models

- **Both tests pass over the two-model list.** The selection test returns `m`, though the broker lists `first` before it, and the new test with no selection returns `first`. The count is 3,980.
- **Two helpers go beyond the plan.** `listed_model(id, description)` in `scripted.rs` makes the `ModelInfo` of one id, so the second model repeats no argument list. `broker_listing_two()` in `harness.rs` makes the broker of both tests, so the name `first` is written in one place.

## Item 10 and Harness commit 2, `fbf7060d`: the two records

- **Issue #29 is closed as completed** with the comment that this plan gave, and one more clause: a loop on such a handle can no longer be written.
- **`## Left open` of the earlier record is one paragraph.** It names docs pull request #9 and Workshop pull request #29, which settled two of its points.
- **Seen and left:** two other sentences of that record still describe the gap as open. They are the history of that day, and this plan asked for the one section.

## Harness commit 3, `0cee3750`: the timer moves from `Host` to `RunSetup`

- **The listing diff is the three lines that this plan named,** and the count is 3,981. The commit changed 46 files.
- **The 18 struct update sites became chains, as counted.** Struct update stays at four places, each of which sets the timer.
- **`begin` in `prepare.rs` takes the setup alone.** Its one caller passed `host.timer.now()`, so a `started_at` parameter would have been `setup.timer.now()` at that one call.
- **Two helpers hold the struct update that sets a timer:** `timed_loop_setup(recorder, timer)` in the suites' `support.rs`, for the six `drive_run` calls with a timer of their own, and `pinned_setup(recorder, instant)` in `prepare.rs`, for the pinned-clock test and the new two-clocks test. This plan said struct update at each call.
- **`host_over` of the runner's suites took its finder from an empty `PromptSet` in this commit,** where this plan listed that for commit 7, because this commit rewrote the function.
- **Other tightenings:** `a_pinned_timer_stamps_the_runs_start` calls `report_of`, and one timer whose flag nothing read became `PendingTimer::default()` inline.
- **Seen and left:** each of the eight `RunSetup::new` calls of the facade suite names `Arc::new(TokioTimer)` itself. A helper would be shorter, but that suite shows the API as a Host application writes it.

## Harness commit 4, `555ecd38`: the runner makes the two built-in Packs

- **The count is 3,983, two more than after commit 3, where this plan expected one more.** The old test of a new registry asserted that each built-in has a description. That assertion needed a home in the runner, and it is the test `each_builtin_has_a_description`.
- **`PackRegistry` derives `Default`.** With an empty start the hand-written impl had nothing left to do, and the listing shows the same `derives` line.
- **`builtin_id(&PackInfo)` in `packs_builtin.rs` gives a built-in's Pack id,** because the registry's own function for that is private to `runvil-plugin`.
- **Two tests have new names,** because the old names said something that stopped being true: `add_refuses_an_fqn_the_registry_holds_already` and `the_registrys_debug_output_shows_each_fqn_and_pack_id`.
- **Two loader tests got simpler.** One lost its filter for the fake Pack, because the registry now holds that one Pack alone, and one asserts in a single line that the registry is empty after a load.
- **Seen and left:** `USER` in `packs.rs` and the names in `packs_builtin.rs` both spell `runvil/user`. The rule about a run with no user stays in `packs.rs`, as this plan says, and it uses its own constant.

## Harness commit 5, `54d1a1eb`: the loader reports every kind of Plugin in one way

- **The first verification failed the listing check.** The doc of `PluginReport::failures` linked `make_report`. `PluginReport` is on the facade's surface and `make_report` is outside it, and the check refuses doc text of the surface that links an item of an internal crate that the facade leaves out. The sentence now names no item. The second run passed, with 3,987 tests and a listing diff of one changed line.
- **`insert_plugin` and `insert_plugin_file` have `#[must_use]` with a reason.** A `Result` forced its caller to look, and a report leaves that to the caller, and a Plugin that inserted no Pack unloads when its report drops.
- **One helper reads a report as the old `Result`.** `insert_into(packs, entry, resources)` in the loader's `fakes.rs` returns the Plugin or the first failure, so the unit tests behind it kept each `.expect(..)`.
- **A path that cannot be made absolute has a test.** The second new test of `insert.rs` also names an empty path, which is one failure with the reason `open` and the path as given.
- **Two copies of the fixture load by their files into one registry,** because each file is a call of its own, and the Pack of the second one is refused. That is the rule "a Plugin name is unique within one directory" from its other side.
- **Seen and left:** the `duplicate` failure says "was already loaded from this directory". A single file can never reach it, because its report starts empty.

## Harness commit 6, `e2d8e1f0`: the two one-prompt finders come back inside an `Arc`

- **Clippy accepts a `new` that returns `Arc<Self>`.** `new_ret_no_self` and the pedantic group said nothing, which settles that risk. The listing diff is the two changed lines, and the count is 3,988.
- **Seen and left:** one test of `prompt_finders-tests.rs` wrote a `Host` literal that equals a call of `host_over`. This commit left it, and commit 7 moved it.

## Harness commit 7, `0da07fd6`: `Host::builder`, private fields, and `Packs` out of the public API

- **The listing diff is exactly what this plan named,** `Cargo.lock` gained its one line, the count is 3,992, and the example printed `Hello, desk.`. The `auto` line of `Host` is unchanged, and `HostBuilder` is `Send` and lacks `Sync`.
- **One field of `Host` is `pub(crate)`, and the other two are private.** `prepare.rs` of the runner reads the Host's Packs to make a run's instances, and it is a sibling module of `host.rs`. The public API shows private fields either way, and no code outside `host.rs` can write a `Host` literal, because the finder and the reports are private.
- **The structural checks accept the edge from the runner to the loader.** Both crates are inside `crates/harness-internal`, and a crate of that container may depend on the crates it holds.
- **The log lines are `Plugin loaded`, at info, and `Plugin failed`, at warn.** The warn line has the field `path` when the failure names one.
- **Two more sites became `Host::builder(finder).build()`.** `harness-execute.rs` called `host_finding` twice with the built-ins alone. So `host_finding` has two callers: `host_over`, and the one test with both a filled registry and a finder of its own.
- **`bare_host()` goes around `host_finding`,** so a Host with the built-ins alone has an empty list of reports, as an application's has.
- **Nine helpers returned a `Packs`, where this plan counted eight.** `building` in `prepare.rs` is the ninth, and it returns the `Host` too.
- **The desk test of the facade suite got a Rust Plugin as an application writes one.** `desk_packs() -> Packs` became `install_desk(packs: &mut PackRegistry) -> Result<(), Error>`, which returns the result of `insert`. A refused Pack still fails the test at once, at the parse, which names the Pack as not installed.
- **Docs outside this plan's list:** the type doc of `PackRegistry`, which named `Packs::new`, and one doc comment in the loader's `pack.rs`.
- **Seen and left:** the suites write `registry(&[])` for an empty registry that the next lines fill by hand, where `PackRegistry::new()` says the same. The form is the same length, needs one more import in each file, and changes nothing about how the Host is built.
- **Seen and left:** `host_finding` hands a filled registry to the builder by assignment inside a Rust Plugin. That is a device of the tests. It also shows that a Rust Plugin can replace the registry, which drops the Packs of every Plugin before it.

## Harness commit 8, `3286ca81`: the builder loads the Plugins of the C contract

- **The listing diff is four added lines and one removed line,** `Cargo.lock` gained its one line, and the count is 3,999.
- **The test of an entry point works as this plan hoped.** It opens a copy of the fixture with `libloading` and takes `runvil_plugin` from it, and the parameter of `with_plugin` fixes the type of the symbol. The suite's dependencies are unchanged, which settles that risk.
- **Clippy's `type_complexity` refused the boxed function written out inside the enum of steps,** so the type alias of commit 7 stays.
- **The directory failure says `the Plugin directory could not be read:` and then the system's reason.** Its `Path` fact is the directory as the builder was given it, where the `Path` fact of a library is an absolute path.
- **`report_of_run` became `report` at 25 places of `run.rs`.** The longer name existed only because `report` was the Plugin report that each test held beside its Host.
- **One assertion changed its form.** The first test of `run.rs` dropped its report and then asserted that the Host's Packs keep the Plugin loaded. A report can no longer be dropped apart from its Host, so the test asserts, before the Host is dropped, that the Host keeps the Plugin loaded.
- **Three helpers moved to the suite's `support.rs`,** `fact`, `library_source`, and `library_path`, because the new `host.rs` needs the same three as `insert.rs`.
- **Docs outside this plan's list:** the type doc of `HostResources` and the doc of `PluginSource::Linked` named `insert_plugin` as what a Host calls, and a comment in `build-xtask` named it as the reason one outside crate is allowed in the facade's listing. Each names the builder method now. One sentence of `testing-and-debugging.md` named `insert_plugin`, which is an internal name from this commit on.
- **One entry differs from the text this plan gave.** "A Host builds one `Host` at startup, through `Host::builder`" says that a `with_` method names one Plugin or sets the resources that the Host offers its Plugins.
- **Seen and left:** no test builds a `Host` over a relative Plugin path, because the path would depend on the working directory of the test process.

## The Workshop dry run

- **Workshop built against the local Harness checkout at `3286ca81`** through a temporary `[patch]` section, which was taken out again with `git restore` on `Cargo.toml` and `Cargo.lock`. The lock file changed as this plan expects of the real pin: its 14 source lines for `runvil-harness`, and two added lines under `harness-runner`.
- **Clippy with warnings denied passed for `workshop-agents` and `workshop-server`.** Their tests passed too: 277 run, with 2 skipped, the same count as before the change.
- **The production code changed as this plan wrote it.** `server_host` and `with_plugins` return the built `Host`, `agent_finder` returns its finder inside an `Arc`, `load_plugin_dir` and the field `_plugins` are gone, `holds` asks the `Host`, and `setup_for` is one chain. `with_plugins` is the chain shown under The API after this plan.
- **Three test helpers go beyond this plan.** `directory_report(host)` is the last report of a Host that names a Plugin directory, `loaded(host)` gives the names of the Plugins in it, and `assert_no_failures(host)` asserts on it. They replace six copies of the same lines.
- **The report of the Plugin directory is "the last report".** Workshop's Host has one step on a thread with no runtime and two on a runtime, so a test cannot name that report by its position from the start.
- **One assertion changed its form.** A test with no Plugin directory asserted that no report came back. Its Host now has the report of the mcp Plugin, so the test asserts that this is the one report and that it holds no Plugin.
- **`std_real_directory.rs` compiled unchanged,** as this plan said.
- **Seen and left:** `with_host_state` in `bindings.rs` keeps struct update syntax. It sets the selected model from an `Option`, and the method of `RunSetup` takes a value.
- **A mistake of the run, caught and undone.** An edit script was given one pair of strings as a flat list and replaced a single letter throughout `language_guide/tests.rs`. `git status` showed the damage, `git restore` brought the file back, and its five edits were made again one at a time. The script touched no other file wrongly.
- **The edits wait in Workshop's working tree** for the commit that pins the Harness.

## For the API report

These points were seen at the call sites, and the report takes them up.

- **A build cannot fail.** A test that wants a refused insert to stop it writes `unwrap()` inside its function. A Host application that must not start with a failed Plugin reads `plugin_reports()` itself.
- **The reports have no label.** A report is known by its position alone, and the position of a step moves when a step before it is added under a condition, as Workshop's mcp Plugin is.
- **`Host::packs()` yields the `PackInfo` alone.** A Pack that was inserted as unavailable looks like any other, so an application cannot list which Packs no run can use, or why.
- **A directory that does not exist is a failure for the builder.** Workshop treats it as a state and filters it out before it names the directory.
- **`Arc::new` is still written around a `PromptSet`, a closure, a recorder, a broker, and a timer.**

## Still to come when this record was written

- The push of the Harness to the fork, and its pull request.
- The Workshop commit on the real pin, with Workshop's whole verification list, and its pull request.
- The two merges, the Harness first, once every check on both pull requests is green.
- The API report.

Each of those is reported in its pull request and in the chat, because a record of them would be one more commit after the merge.

## After the pull requests opened

The automatic reviews of the two pull requests led to one more commit in each repo, so this note was added with them, before either merge.

- **The pull requests are [#61](https://github.com/cppalliance/runvil-harness/pull/61) on the Harness and [#33](https://github.com/cppalliance/runvil-workshop/pull/33) on Workshop.**
- **`cargo fetch` got `6efe4ad7` from `cppalliance/runvil-harness` while only the open pull request held it,** so the planned order stood: both pull requests were open before either merge.
- **Workshop commit `71593d7a` passed Workshop's whole verification list on the real pin.** Its two test groups ran 706 and 353 tests, all passing, the same counts as before. `Cargo.lock` changed its 14 source lines for `runvil-harness` and gained two lines under `harness-runner`, as this plan expected.
- **The automatic review of the Harness found a count in an entry of `tools-and-packs.md`.** The entry said that a run includes the two built-in Packs, and `AGENTS.md` keeps counts of things in the code out of an entry. Commit `e9c07ba5` removes that count and four more of the same sort that this work wrote: both ids of the built-ins, two kinds of Plugin, one of three ways to name a Plugin of the C contract, and each of the two one-prompt finders. So those five sentences differ from the text this plan gave.
- **The same review said that the links of this plan copy start at `plan/` and so lead nowhere.** That is true, and the links stay as they are. The copy is the approved text, its links are written from the folder that holds both repositories side by side, and the ten earlier plan copies with links have the same form.
- **The automatic review of Workshop asked for one test,** of a Plugin directory that exists and cannot be read. That is the one case of the directory whose outcome changed with this work. Workshop commit `a2d67686` adds it: a regular file in the place of the directory gives a built `Host` whose one report holds one failure with the reason `directory` and the configured path. Workshop's second test group runs 354 tests with it.
- **One count in the text of the Workshop pull request was corrected.** It said that `with_plugins` and `load_plugin_dir` were 60 lines. A count by script gave 58, and 28 for `with_plugins` after the change.
- **Workshop keeps its pin at `6efe4ad7`.** That commit holds all of the code, and `master` of `cppalliance/runvil-harness` holds it after the merge. The commits above it change text only.
