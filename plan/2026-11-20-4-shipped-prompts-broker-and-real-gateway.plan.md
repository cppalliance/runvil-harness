---
name: Check the shipped prompts, what a round hands the broker, and a run through a real Gateway
overview: A prompt reaches a model in three steps. Its file is parsed and run, each model round is handed to the broker its Host supplies, and the broker sends the round to a Gateway. Tests check each step with stand-ins and leave three gaps, which this plan closes with eight tests. Six run in a normal test run, and each closes a gap that was shown by breaking the code or a shipped prompt and seeing every test still pass. Two run the shipped `hello.md` through a real Gateway and are ignored unless asked for. It changes no production code and no prompt.
todos:
  - id: plan-copy
    content: "Commit this plan alone on the branch qa-plan-tests"
    status: completed
  - id: shipped-prompts
    content: "Add the three helpers and the five tests to crates/runvil-engine/tests/suite/shipped.rs, and read the research prompt through the new helper"
    status: completed
  - id: round-hand-off
    content: "Make the recording broker in crates/harness-internal/runner/tests/it/effect_loop-broker.rs keep each round's options and tool names, and add the test"
    status: completed
  - id: live
    content: "Add crates/harness-gateway-client/tests/live.rs with the two ignored tests, which share one run of hello.md"
    status: completed
  - id: breaks
    content: "Apply each of the eight breaks, confirm the test named for it now fails, and revert it"
    status: completed
  - id: runs
    content: "Run the two live tests against real Gateways, in the cases the Testing Plan lists"
    status: completed
  - id: verify
    content: "Run every verification command and vibelight"
    status: completed
  - id: pr
    content: "Push the branch qa-plan-tests to the fork and open one PR against cppalliance/runvil-harness"
    status: in_progress
---

# Check the shipped prompts, what a round hands the broker, and a run through a real Gateway

<product-contract>

## Product Requirements

A prompt reaches a model in three steps: its file is parsed and run, each model round is handed to the broker its Host supplies, and the broker sends the round to a Gateway. Tests check each step with stand-ins, and they leave three gaps. Two were shown by breaking the code or a shipped prompt and seeing every test still pass. The third is that nothing runs a prompt through the Harness against a real Gateway. The code and the prompts are correct today, so this is missing protection, not a bug.

- Problem and users:
  - The shipped prompts. The repository ships five sample prompts in `prompts/`. A test runs one of them, `research-person.md`. No test reads `echo.md`, `greet.md`, `hello.md`, or `analyst-example.md`, so each can be broken and every test still passes. People read and copy the samples to learn the language, and the samples are edited by hand when the language changes. Two commits of 2026-11-13 edited them that way, `3d87c936b` (a prompt's arguments became one JSON object) and `7147e709f` (every heading became a section), and nothing checked the result.
  - What a round hands the broker. A prompt sets how a model round is made: its role can turn thinking off, `models.use` can set a temperature and a token cap, and a section can offer tools. The Engine puts all of that in the round. `perform`, in `crates/harness-internal/runner/src/effect_loop.rs`, hands the broker the round's options, which carry the name of the model the run bound, and the schemas of its tools. Engine tests check what the round holds, and the Gateway client's tests check what it sends for the options it is given. Every test broker ignores the options and the tool schemas it is handed. So the Harness can ask the broker for a model the run never bound, drop the options, or drop the tool schemas, and every test passes.
  - A real Gateway. Workshop is one Host. Any program that runs a prompt through the Harness is another, and such a Host reaches its models through `GatewayBroker` in `crates/harness-gateway-client`. The broker is tested against stand-in servers, and the Harness against brokers written for its tests. The Gateway's own tests of its cloud route send to stand-in servers too. So nothing runs a prompt through the Harness and `GatewayBroker` against a real Gateway, on a model the Gateway runs on its own machine or on a cloud provider's model.
  - The users are the people who copy the samples, the prompt authors whose settings must reach the model, the people who write a Host other than Workshop, the people who point a Gateway at a cloud provider, and the engineers who change the language, the Harness, or the Gateway client and rely on the tests.
- Goals:
  - Each of the eight breaks listed in the Functional Specification makes at least one test fail.
  - Every prompt in `prompts/` is run by a test in one file, `crates/runvil-engine/tests/suite/shipped.rs`.
  - Two live tests run the shipped `prompts/hello.md` through the Harness and `GatewayBroker` against a real Gateway, and check that each run ends with text. The first runs on the model the Gateway lists first, or on a model the person names. The second runs on a local model and then on a cloud model of the same Gateway. It tells the two apart itself, so it cannot pass on two local models.
  - A normal test run skips the two live tests. A person asks for them by name and names the Gateway in the environment.
- Non-goals:
  - Any change to production code, or to a prompt in `prompts/`.
  - A test over HTTP of what a round hands the broker. The hand-off is one call, and a broker that keeps what it is given pins it.
  - A test of the Engine's side or of the Gateway client's side of the hand-off. Both are covered today.
  - A test of which model a run binds. Two tests upstream check that, as the Decision Record sets out.
  - Starting or stopping a Gateway, fetching a model, or holding a provider's key. The person who asks for a live test has a Gateway running.
  - A run of the live tests in CI. No workflow has a Gateway with a chat model.
  - A check of what a real model says. Its words vary.
- Success criteria:
  - With each break applied in turn, the test named for it fails, and it passes again once the break is reverted.
  - A normal run of the workspace has exactly six more tests than before, and skips two more.
  - With a Gateway running that serves a chat model, the first live test passes with no model named and with that model named. With no Gateway at the named address, it fails and the failure names the address.
  - With a Gateway that serves a local chat model and a cloud chat model, the second live test passes. With a Gateway that serves no cloud model, it fails before any run and says which kind is missing.
  - Every verification command in the Testing Plan passes.
- Constraints:
  - The existing tests of the two files that gain tests keep every assertion they make today: two in `shipped.rs`, and two in `crates/harness-internal/runner/tests/it/effect_loop-broker.rs`.
  - The tests use the test support their files already use, and add no dependency to any crate.
- Open questions: None

## Functional Specification

Six tests run in a normal test run, on scripted models and on a broker that keeps what it is handed. Two tests are live: each builds a Host as a small command-line program would, and runs against the Gateway the environment names. Nothing a user of Runvil sees changes.

- Actors and workflows:
  - A developer or CI runs the test suite, which runs the six tests and skips the two live ones.
  - For the live tests, a person starts a Gateway that serves a chat model, sets the variables, and runs `cargo nextest run -p harness-gateway-client --test live --run-ignored ignored-only`. That runs both. `-E 'test(through_a_real_gateway)'` asks for the first alone, and `-E 'test(local_model_and_on_a_cloud_model)'` for the second.
  - The second live test needs a Gateway that serves a local chat model and a cloud chat model at once.
- Inputs and outputs:
  - The shipped prompts. Each of five tests reads a prompt's file from `prompts/`, so it runs the text a reader of the repository sees, and runs it on the Engine with scripted model replies.
    - `echo.md`, with the arguments `{"text":"ping"}`: the run returns `ping`, and it makes no model round.
    - `greet.md`, with the arguments `{"name":"Ada"}` and one scripted reply: the run makes one round under the `default` role. The round's only message is `Repeat exactly, with no extra words: Hello, Ada!`. The run returns the scripted reply.
    - `hello.md`, with one scripted reply: the run makes one round under the `default` role. The round's only message is `Say "Hello, world!"`. The run returns the scripted reply.
    - `analyst-example.md`, with the arguments `{"text":"the input"}`, one scripted reply, an 8,192-token model for `default`, and a 40,000-token model for `strong`: the run makes one round under the `strong` role. The round's only message is the prompt's instruction, a blank line, and `the input`. The run returns the scripted reply.
    - `analyst-example.md`, with a 39,999-token model filling every role: the run is refused on its first step with `requirements_unmet`. It reports no event and issues no effect. The message names the role, the 40,000 tokens it requires, and the 39,999 the model provides.
  - What a round hands the broker. One test runs a prompt of one model round through the effect loop, with a broker that keeps what each round hands it.
    - The run's one model is named `m`. The prompt declares its `default` role with `thinking: false`. Its one section selects `default` with a temperature of 0.25 and a token cap of 64, defines an adhoc tool named `grab`, offers it, and makes one round.
    - The broker is handed one round. Its options name the model `m`, turn thinking off, and hold the temperature 0.25 and the token cap 64. Its tools are the one schema named `grab`.
  - The live tests. Both read `RUNVIL_E2E_GATEWAY`, the Gateway's API root, such as `http://127.0.0.1:8189/v1`, and `RUNVIL_E2E_KEY`, the Gateway's key. Their Host has no Plugin, so its only Packs are the built-ins, and it finds one prompt, the text of `prompts/hello.md`.
    - The first test: `RUNVIL_E2E_MODEL`, when set, is the model the Host selects. Without it the run takes the first model the Gateway lists. The run ends with text that is not empty.
    - The second test asks the Gateway for its settings at `/admin/config`, with the key. The local model is the first `[[local_model]]` entry of kind `chat`. The cloud model is the first `[[model]]` entry of kind `chat` whose first endpoint has a `base_url` that is not a loopback address. The test runs `hello.md` with the local model selected, and then with the cloud model selected. Each run ends with text that is not empty.
- States and validation:
  - A missing `RUNVIL_E2E_GATEWAY` or `RUNVIL_E2E_KEY` fails a live test with a message that names the variable.
  - Settings with no local chat model, or with no cloud chat model, fail the second live test before any run. The message says which is missing.
- Errors and recovery:
  - A prompt file that cannot be read fails its test with a message that names the path.
  - A live run that ends with an error fails its test and prints the error whole. In the second live test the message also says whether it was the local or the cloud run, and names the model.
- Security and privacy behavior:
  - The Gateway's key comes from the environment and is held in the client's `SecretString`, which is redacted wherever it is formatted.
  - The provider's key belongs to the Gateway and never reaches the tests. They send only the Gateway's key.
  - The cloud run sends the text of `hello.md` to the provider, and the provider charges for it.
- Acceptance criteria:
  - The shipped prompts, five breaks of the prompt files:
    - Break 1: in `echo.md`, change `return args.text` to `return args.txt`. The echo test fails.
    - Break 2: in `greet.md`, change `"Hello, "` to `"Hi, "`. The greet test fails.
    - Break 3: in `hello.md`, change `return models.infer(prose)` to `models.infer(prose)`. The hello test fails.
    - Break 4: in `analyst-example.md`, change the Lua line `models.use("strong")` to `models.use("fast")`. The analyst run test fails.
    - Break 5: in `analyst-example.md`, change `min_context: 40000` to `min_context: 4000`. The analyst refusal test fails.
    - With all five applied, the five new tests of `shipped.rs` are the only tests in the workspace that fail.
  - What a round hands the broker, three breaks of `perform`:
    - Break 6: change `binding.completion_options()` to `binding.completion_options().with_model("other")`. The test fails.
    - Break 7: change `binding.completion_options()` to `CompletionOptions::new(binding.completion_options().model())`, which keeps only the model's name. The test fails.
    - Break 8: pass `Vec::new()` to `broker.chat` in place of `tools`. The test fails.
    - With any one of the three applied, the new test is the only test in the workspace that fails.
  - The live tests: the runs listed under Success criteria give the results stated there.

</product-contract>
<implementation-contract>

## Technical Design

The change is test code in two existing files and one new test file. No module, interface, public API, or stored format changes.

- Architecture:
  - The tests of the shipped prompts run on the Engine with scripted model replies, as the two existing tests of `research-person.md` do.
  - The test of what a round hands the broker drives a run through `drive_run`, as the two existing tests of its file do.
  - The live tests are integration tests of `harness-gateway-client`, the crate that holds the broker. That crate already depends on `runvil`, the one crate a Host depends on, so the tests build their Host from public items only.
- Modules and interfaces: None
- File and public API changes:
  - `crates/runvil-engine/tests/suite/shipped.rs` gains three helpers and five tests. One helper reads a shipped prompt by its file name. One lists a run's model rounds as the role and the messages of each. One checks that a run made exactly one round and returned a given reply. The existing helper that runs `research-person.md` reads its file through the first. The refusal test calls `refusal` in `crates/runvil-engine/tests/suite/prepare.rs`, which already checks the error code, the absence of events, and the absence of effects.
  - In `crates/harness-internal/runner/tests/it/effect_loop-broker.rs`, the recording broker keeps each round's options and the names of its tools, beside the round and last message it keeps today. The file gains one prompt, one helper that builds a run over a given prompt, and one test. The existing helper builds its run through the new one.
  - `crates/harness-gateway-client/tests/live.rs` is new. A `Gateway` type holds the API root and the key the environment names. Its `hello` method is the run of `hello.md` that both tests make. Its `a_local_and_a_cloud_model` method reads the settings and returns the two names in a `LocalAndCloud` value. Each of the two tests is marked `#[ignore]` with the reason.
  - `plan/2026-11-20-4-shipped-prompts-broker-and-real-gateway.plan.md` is this plan.
  - No public API changes, so `crates/runvil-engine/public-api.txt` and `crates/runvil/public-api.txt` stay as they are.
- Data, persistence, failure, security, and privacy constraints: None

</implementation-contract>
<verification-contract>

## Testing Plan

The six tests of a normal run are checked by their breaks: each break must fail the test named for it. The two live tests are checked by running them against real Gateways, and against none. The repository's full verification list runs once at the end.

- Unit: None
- Integration and end-to-end:
  - `cargo nextest run --locked -p runvil-engine -E 'test(shipped)'` runs the seven tests of `shipped.rs`: two existing and five new.
  - `cargo nextest run --locked -p harness-runner -E 'test(effect_loop::broker)'` runs the three tests of `effect_loop-broker.rs`: two existing and one new.
  - Run the first live test with a Gateway that serves a small local chat model, once with no model named and once with the model named.
  - Run the second live test with a Gateway that serves a small local chat model and one model of a cloud provider.
- Regression, security, and performance:
  - Before the tests are written, apply breaks 1 to 5 together, and then each of breaks 6 to 8, and see the whole workspace pass under each.
  - Apply each of breaks 1 to 5 in turn, run the seven tests of `shipped.rs`, see the named test fail, and revert the break. Then apply all five at once and run the whole workspace.
  - Apply each of breaks 6 to 8 in turn, run the whole workspace, see the new test fail and every other test pass, and revert the break.
  - Run the first live test with the Gateway stopped, and see it fail with the address in the message.
  - Run the second live test with a Gateway that serves no cloud model, and see it fail with the message that names the missing kind.
  - Run the crate's tests without asking for ignored tests, and see both live tests skipped.
- Exit criteria:
  - `cargo nextest run --locked --workspace --all-features`
  - `cargo clippy --workspace --all-targets --all-features`, with `CARGO_BUILD_WARNINGS=deny`
  - `cargo fmt --all --check`
  - `cargo doc --workspace --no-deps --all-features` and `cargo doc -p runvil-engine --no-deps`, both with `RUSTDOCFLAGS="-D warnings"`
  - `cargo +<nightly> xtask api --check`, with the nightly named in `crates/build-xtask/src/api/toolchain.rs`
  - `cargo dylint --all -- --locked --workspace --all-targets --all-features`
  - `cargo test -p build-xtask`
  - `node --test tools/docs-claims.mjs`
  - `vibelight`, run in the repository root

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Each gap of a normal run's tests is shown before its tests are kept, by a break that passes every existing test. The user's rule, given on 2026-11-18: "If existing tests already cover these behaviors, we shouldn't be raising a PR that introduces duplicate coverage."
  - The tests of the shipped prompts run on the Engine with a scripted model, not through the Harness. The existing tests of the fifth shipped prompt are written that way, in the same file, and the Engine's test support already records each model round. The Harness's test brokers do not record what a round sent.
  - Those tests read the prompt files from `prompts/`. A copy of a prompt's text inside a test would pass while the shipped file broke.
  - Each message of a shipped prompt is compared whole, not searched for a phrase. A sample is short, and any edit to its prose is a deliberate change that should update its test.
  - The refusal test uses a 39,999-token model, one token short. With the passing run on a 40,000-token model, the pair pins the sample's `min_context` to exactly 40,000.
  - What a round hands the broker gets one new test function, in the file whose subject is what the broker receives. Its two existing tests run a prompt that sets no option and offers no tool, so neither can show the hand-off.
  - The existing recording broker gains the two things it ignored, in place of a second broker. One broker then records everything a round hands over.
  - The model's name is checked beside the other options, in the same test. It travels in the same options value, and break 6 changes it in the same line as break 7.
  - The tool of that test is an adhoc tool, defined in the prompt's Lua. It needs no Pack, so the test needs no tool performer.
  - The live tests are ignored by default and read the Gateway's address from the environment. The user's direction, given on 2026-11-20: "All these tests should be runable locally. We aren't dealing with running tests in CI yet."
  - The live tests send the shipped `hello.md`. The tests they serve, in the QA plan of attack, the list of tests this work automates, name that prompt or leave the prompt open: "Send `hello.md` from a command-line Host through a real Gateway. The run ends with text." and "Run one prompt on a local model, then on a cloud provider. Both runs end with text."
  - The Gateway's key is a required setting with no default. The broker always sends a key, and a real Gateway answers a wrong key with status 401 even from a loopback caller it would trust with no key at all. A default key would fail that way, which was seen on the first run of the first live test.
  - The live tests assert only that the text is not empty. What a real model writes for "Say Hello, world!" is not fixed.
  - The second live test finds its two models in the Gateway's settings. With two variables that name them, a person could name two local models and the test would pass without a request ever leaving the machine. The cost is that the test knows eight names of the settings: `local_model`, `model`, `endpoint`, `kind`, `name`, `endpoints`, `id`, and `base_url`. A rename there fails the test with the message that says what it did not find.
  - A cloud model is one whose endpoint is not at a loopback address. The test asks `GatewayEndpoint::is_loopback`, the rule this crate already applies to the Gateway's own address. By this rule a model served from another machine on the same network counts as a cloud model.
  - One test makes both runs, the local one first, as the QA plan words it. When the cloud run fails after the local run passed, the fault is on the cloud route.
  - One plan and one branch, `qa-plan-tests`, hold all three parts. The user's direction, given on 2026-11-20: "All of the work for this effort should be in one branch."
- Rejected alternatives:
  - Two tests of which model a round asks the broker for, in `crates/runvil/tests/suite/broker.rs`. They sent the shipped `hello.md` to a broker that lists two models, and they were part of this work until 2026-11-21. They served the QA plan's test "Select no model in that Host and send `hello.md`. The broker is asked for the first model." That day upstream commit `72b95a376` added `a_run_with_no_selection_binds_the_first_listed_model` and made `a_prompt_reads_the_selected_model_through_a_roles_handle` select the second of two listed models. Those two tests fail under the two breaks of `current_model` that the removed tests were written for: binding the last listed model when none is selected, and binding the first listed model whatever the selection. The removed tests would have checked the same rule a second time. The one break that only they caught is now break 6.
  - A test that lists the `prompts/` folder and fails on a file with no test. It reminds, but it cannot tell whether a listed prompt is run. Revisit if a sixth prompt ships without a test.
  - A test that joins the Harness and the Gateway client against a stub HTTP server. It would cover the hand-off and repeat what the client's own tests check. Revisit if a break is found that only the joined test would notice.
  - A new command-line program in `examples/` in place of the live tests. A test can be asked for by name and reports pass or fail, and `crates/runvil/examples/run-prompt.rs` already shows a Host's steps.
  - A live test that starts its own Gateway and fetches a model. It would tie this repository to the Gateway's settings format and to one model. The second live test reads eight names of that format and writes none. Revisit when several tests need the same Gateway and a shared way to start it.
  - Two variables that name the local and the cloud model. See the decision on the Gateway's settings.
  - A local and cloud test in the Gateway's repository. Its tests send requests to stand-in servers and run no prompt.
  - A local and cloud test through Workshop. It adds Workshop's server and its agents to the path, and the choice of a model in Workshop has a test of its own there.
- Assumptions, risks, and notes:
  - Existing tests already check each behavior the samples use, with prompts written inside the tests: `a_prompt_with_no_args_key_runs_on_the_empty_object_and_on_one_string_named_text`, `finite_pipeline_runs_the_unified_surface_end_to_end`, `a_switch_in_one_section_is_still_in_effect_in_the_next`, and `a_context_minimum_above_the_filling_models_is_refused`. What is new here is that the shipped files are run.
  - The tests of the shipped prompts fail whenever a sample's text changes. That is intended: the person who changes a sample updates its test in the same change.
  - A model round's record names the role it ran under as `alias` (`crates/runvil-internal/runtime/src/execute/run/effect.rs`). The tests of the shipped prompts read the role from there.
  - The Engine's side of the hand-off is checked by `models_use_forwards_binding_completion_options_to_the_gateway`, and the client's side by `complete_sends_completion_options_and_stream_flags_on_the_wire`. When the test of the hand-off was written, dropping a round's messages failed at least eleven tests, so the messages need no new check.
  - `cargo nextest run -p harness-gateway-client --test live` with no other test selected ends with "no tests to run", since every test of that file is ignored. A run of the crate or the workspace reports them as skipped.
  - Every result below that names no other commit was measured on `upstream/master` at commit `665ff7d87`, onto which the branch was rebased on 2026-11-21. Without this plan's tests the workspace runs 4,002 tests there and skips 22. With them it runs 4,008 and skips 24.
  - Result of the breaks without this plan's tests: with breaks 1 to 5 applied together, and with each of breaks 6, 7, and 8 applied alone, all 4,002 tests passed.
  - Result of breaks 1 to 5 with the tests: each fails exactly the test named for it, and the other six tests of `shipped.rs` pass. With all five applied, the workspace ran 4,008 tests, the five new ones failed, and the other 4,003 passed.
  - Result of breaks 6 to 8 with the tests: under each, the workspace ran 4,008 tests, the new test failed, and the other 4,007 passed.
  - Result of the two breaks of `current_model` without this plan's tests: with `catalog.models().first()` changed to `catalog.models().last()`, `a_run_with_no_selection_binds_the_first_listed_model` failed. With `catalog.get(&id)` changed to `catalog.get(&id).and(catalog.models().first())`, `a_prompt_reads_the_selected_model_through_a_roles_handle` failed. The other 4,001 tests passed under each.
  - The live runs used a Gateway built from `cppalliance/runvil-gateway` at commit `52fd92d`. It served Qwen3 0.6B from a local file and, for the cloud runs, Claude Haiku 5.5 at Anthropic's OpenAI-compatible address, `https://api.anthropic.com/v1`. The cloud entries of its settings are the ones the Gateway's settings page writes from its model sheet.
  - Result of the runs of the first live test: with the Gateway running, it passed with no model named, in 0.19 seconds, and three times with the model named, in 0.11 to 0.13 seconds. With the Gateway stopped it failed with `Transport`, and the message named `http://127.0.0.1:8189/v1/models` and "Connection refused".
  - Result of the run of the second live test with no cloud model served: it failed before any run with "the Gateway's settings name no chat model whose endpoint is off its machine". The crate's own run reported 160 passed and 2 skipped.
  - Result of the cloud runs, which were made on 2026-11-20, before the rebase. With a provider key that was not scoped to one workspace, the test found both models, the local run ended with text, and the cloud run failed: the provider refused the request with status 400, because of the key and not because of the request, "This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header with the ID of the workspace to use." With a key scoped to one workspace, the test passed three times in a row, in 0.65 to 0.82 seconds a run. The Gateway sent straight to the provider, and its log shows no refused request.
  - The cloud run was not made again after the rebase, because each cloud run is charged. Since the last one, the run both tests share changed in three ways: its timer is the crate's stock `TokioTimer`, handed to the run's setup, its Host is built through `Host::builder`, and its read of `hello.md` names the file when it fails. The first live test makes the same run and passed after all three.
  - Found in the cloud runs: a Gateway cannot use a provider key that is not scoped to one workspace. An `[[endpoint]]` entry has an id, a protocol, an address, a key, and a dominion, and no setting for a request header. In a trial, a relay on the Gateway's machine added the header and passed each request on, and the provider then answered, so the header was the only thing missing.
  - Found in the cloud runs: the Gateway's caller is told only "upstream returned 400". The provider's reason is in the Gateway's log and nowhere in the run's error.
  - Found in the cloud runs: the Gateway lists the cloud model before the local one. The first live test selects no model unless one is named, so it runs on the cloud model whenever the Gateway serves one.
  - Found by the `dylint` job of the draft pull request, the one check not run locally: `io_try_without_context` refused the read of `hello.md` in the shared run, whose `?` passed on an error that named no file. The read now names its file, in the words the other tests of this repository use.
  - Result of the exit criteria. `cargo nextest run --locked --workspace --all-features` ran 4,008 tests, all passed, and 24 were skipped, the two live tests among them. `cargo clippy --workspace --all-targets --all-features`, `cargo fmt --all --check`, `cargo test -p build-xtask`, and the wording check passed. The docs builds, `xtask api --check`, and the custom lints were not run locally. `cargo-dylint` is not installed on the author's machine.
  - CI runs the checks that were not run locally on the draft pull request, cppalliance/runvil-harness#60. Its eleven checks passed on the branch as it stood on 2026-11-20, before the rebase: `api-surface`, `ci-green`, `clippy`, `docs`, `dylint`, `fmt`, `msrv`, `supply-chain`, `test`, `test-windows`, and `wording`.
  - `vibelight` outlines each crate's library code and not its `tests/` folder, so it was run on each changed test file alone, last on 2026-11-21. It reports no lead in `effect_loop-broker.rs` or `live.rs`, and two in `shipped.rs`: the bodies of the greet and hello tests are alike, because the two samples do the same thing, and the literal `web/search` repeats in three functions that were already there. Earlier runs flagged three things that were then changed: a role passed as a string in `shipped.rs`, now a `ModelRole`; near-identical bodies of its one-round tests, which now share one checking helper; and a type alias used once in `effect_loop-broker.rs`, now a small struct.
  - This work was first written as five plans. They were merged into this plan on 2026-11-21, at the user's word: "let's merge those into 1".

### Deferred and Out of Scope

- Out of scope: tool calls, streaming, and reasoning on a cloud model. The second live test makes one round with no tools.
- Out of scope: every other test of the QA plan of attack.

</decision-record>
<project-survey>

## Project Survey

None

</project-survey>
<execution-plan>

## Execution Instructions

- Work items:
  - Commit this plan alone on the branch `qa-plan-tests`, made from `upstream/master`.
  - Add the tests of each part in a commit of its own: the shipped prompts, what a round hands the broker, the first live test, and the second live test.
  - Apply and revert each break, recording which tests fail.
  - Make the live runs and record their results.
  - Run the exit criteria.
  - Push the branch to the fork and open the PR only when the user asks.
- Dependencies and verification:
  - The six tests of a normal run depend on nothing outside the repository.
  - The live runs need a Gateway that serves a chat model. The second live test needs one that also serves a cloud chat model, which needs a key the provider accepts from the Gateway.
  - The breaks are never committed. `git status` shows no change under `prompts/` or to `effect_loop.rs` before each commit.

</execution-plan>
