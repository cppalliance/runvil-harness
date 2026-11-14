//! The `args` surface. A run's arguments are one JSON object. The run's
//! first step checks it against the prompt's `args` declaration, and every
//! section then reads the checked object as the read-only table `args`,
//! in Lua and through `{{ args.field }}` placeholders. The check fills a
//! declared default, reads a `null` for a field that is not required as
//! the field left out, holds a whole number under `integer` as an
//! integer, and keeps the undeclared fields of an object that takes them.
//! A prompt with no `args:` key declares one optional string named
//! `text`. A tool handler's `args` parameter is that call's arguments.
//!
//! The first-step refusals sit in `refusals`, and the arguments the chain
//! of a `call`, of a `tasks.spawn`, and of a fanout arm reads sit in
//! `chains`.

use runvil_prompt::test_support::{PromptSource, one_section, prompt};
use runvil_types::error::ErrorCode;

use crate::test_support::scenario::Scenario;
use crate::test_support::{at_line, authored, refused};

/// A declaration of three arguments: a required string `query`, an
/// integer `limit` that defaults to 3, and a string `depth` that is
/// `quick` or `full`.
const SEARCH: &str = "args: { type: object, properties: { query: { type: string }, \
     limit: { type: integer, default: 3 }, depth: { type: string, enum: [quick, full] } }, \
     required: [query] }";

/// A declaration with nested shapes: an object `opts` whose integer
/// `depth` defaults to 2, and a list of strings `focus`.
const NESTED: &str = "args: { type: object, properties: { \
     opts: { type: object, properties: { depth: { type: integer, default: 2 } } }, \
     focus: { type: array, items: { type: string } } } }";

/// A prompt declaring [`SEARCH`] whose one section `Only` runs `lua`.
fn search_prompt(lua: &str) -> String {
    prompt(SEARCH, &[("Only", lua)])
}

/// A run of `source` whose caller passes the one argument `query`, the
/// string `papers`.
fn papers(source: impl Into<String>) -> Scenario {
    Scenario::new(source).args(r#"{"query":"papers"}"#)
}

#[test]
fn a_section_reads_its_arguments_as_fields_of_the_args_table() {
    let source = search_prompt(
        "
        assert(type(args) == 'table', 'args is a table')
        assert(args.depth == nil, 'a field the caller left out reads nil')
        assert(args.absent == nil, 'a field the declaration lacks reads nil')
        return args.query .. ', limit ' .. args.limit",
    );
    assert_eq!(papers(source).run().text(), "papers, limit 3");
}

#[test]
fn a_placeholder_reads_a_field_of_args_at_any_depth() {
    let source = PromptSource::new().front(NESTED).prose(
        "Only",
        "
        Depth {{ args.opts.depth }} of {{ args.opts }}, on {{ args.focus }}.

        ```lua
        return prose
        ```",
    );
    let trace = Scenario::new(source.build())
        .args(r#"{"opts":{},"focus":["intro","proofs"]}"#)
        .run();
    assert_eq!(
        trace.text(),
        r#"Depth 2 of {"depth":2}, on ["intro","proofs"]."#
    );
}

#[test]
fn a_bare_args_placeholder_renders_the_whole_object_as_compact_json() {
    let whole = "
        Whole: {{ args }}

        ```lua
        return prose
        ```";
    let declared = PromptSource::new().front(SEARCH).prose("Only", whole);
    let trace = Scenario::new(declared.build())
        .args(r#"{ "query": "papers", "depth": "full" }"#)
        .run();
    assert_eq!(
        trace.text(),
        r#"Whole: {"depth":"full","limit":3,"query":"papers"}"#,
        "the keys are sorted and the default is filled"
    );
    let undeclared = PromptSource::new().prose("Only", whole);
    assert_eq!(
        Scenario::new(undeclared.build()).run().text(),
        "Whole: {}",
        "an empty object renders as one"
    );
}

#[test]
fn a_placeholder_for_a_field_args_lacks_is_a_substitution_error_pcall_catches() {
    let source = PromptSource::new().front(SEARCH).prose(
        "Only",
        "
        Depth: {{ args.depth }}.

        ```lua
        local ok, err = pcall(function() return prose end)
        assert(not ok, 'a field the caller left out must not render as empty text')
        return err.kind .. '|' .. tostring(err)
        ```",
    );
    assert_eq!(
        papers(source.build()).run().text(),
        format!(
            "lua|prompt:10: missing {{{{ args.depth }}}}. {}",
            ErrorCode::Substitution.prose()
        )
    );
}

#[test]
fn an_uncaught_placeholder_for_a_field_args_lacks_ends_the_run_on_its_template_line() {
    let source = PromptSource::new().front(SEARCH).prose(
        "Only",
        "
        Query: {{ args.query }}.
        Depth: {{ args.depth }}.

        ```lua
        return prose
        ```",
    );
    let trace = papers(source.build()).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::Substitution, "{error:?}");
    assert_eq!(
        error.message,
        "missing {{ args.depth }} [MissingKey at byte 32]"
    );
    assert_eq!(error.at, at_line(11), "{error:?}");
}

#[test]
fn an_args_table_measures_and_iterates_as_the_object_it_holds() {
    let source = PromptSource::new().front(NESTED).section(
        "Only",
        "
        local keys = {}
        for key in pairs(args) do keys[#keys + 1] = key end
        table.sort(keys)
        local indexed = {}
        for index, name in ipairs(args.focus) do indexed[#indexed + 1] = index .. '=' .. name end
        local first_key, first_value = next(args.opts)
        local a, b, c = table.unpack(args.focus)
        return table.concat({
          table.concat(keys, ','), #args.focus, table.concat(args.focus, '+'),
          table.concat(indexed, ','), first_key .. '=' .. first_value, a .. b .. c,
        }, '|')",
    );
    let trace = Scenario::new(source.build())
        .args(r#"{"opts":{"depth":4},"focus":["a","b","c"]}"#)
        .run();
    assert_eq!(trace.text(), "focus,opts|3|a+b+c|1=a,2=b,3=c|depth=4|abc");
}

/// Lua that tries to assign `args` and to set a field of it, each under
/// `pcall`, and returns the two refusals and the field as it then reads.
const TWO_WRITES: &str = "
    local function refused(write)
      local ok, err = pcall(write)
      assert(not ok, 'the write is refused')
      return tostring(err)
    end
    return refused(function() args = { query = 'other' } end)
      .. '|' .. refused(function() args.query = 'other' end)
      .. '|' .. tostring(args.query)";

/// Asserts `text` is what [`TWO_WRITES`] returns when its block starts on
/// `first_line` of the prompt: each write is refused with the read-only
/// rule on its own line, and `args.query` still reads `query`.
#[track_caller]
fn assert_both_writes_refused(text: &str, first_line: u32, query: &str) {
    let expected = [
        refused(
            first_line + 5,
            "args is read-only: assign to `var` or a section global instead",
        ),
        refused(
            first_line + 6,
            "args is read-only: cannot set field 'query'",
        ),
        query.to_owned(),
    ];
    assert_eq!(text, expected.join("|"));
}

#[test]
fn args_is_read_only_in_a_walk_section() {
    let trace = papers(search_prompt(TWO_WRITES)).run();
    assert_both_writes_refused(trace.text(), 11, "papers");
}

#[test]
fn args_is_read_only_in_the_preamble() {
    let source = PromptSource::new()
        .front(SEARCH)
        .preamble(TWO_WRITES)
        .section("Only", "return 'unreachable'");
    let trace = papers(source.build()).run();
    assert_both_writes_refused(trace.text(), 9, "papers");
}

#[test]
fn args_is_read_only_in_a_called_chain() {
    let source = prompt(
        SEARCH,
        &[
            ("Main", "return call('../Sub', { query = 'chain' })"),
            ("Sub", TWO_WRITES),
        ],
    );
    let trace = papers(source).run();
    assert_both_writes_refused(trace.text(), 17, "chain");
}

#[test]
fn args_is_read_only_in_a_task() {
    let source = prompt(
        SEARCH,
        &[
            (
                "Main",
                "
                local task = tasks.spawn('../Child', { args = { query = 'task' } })
                local _, ok, result = tasks.join_any({ task })
                assert(ok, 'the task catches both refusals')
                return result",
            ),
            ("Child", TWO_WRITES),
        ],
    );
    let trace = papers(source).run();
    assert_both_writes_refused(trace.text(), 20, "task");
}

#[test]
fn args_is_read_only_in_a_fanout_arm() {
    let source = prompt(
        SEARCH,
        &[
            (
                "Parent",
                "
                local r = tasks.fanout('Worker', { 'alpha' })
                assert(r[1].ok, 'the arm catches both refusals')
                return r[1].text",
            ),
            ("### Worker", TWO_WRITES),
        ],
    );
    let trace = papers(source).run();
    assert_both_writes_refused(trace.text(), 19, "papers");
}

#[test]
fn an_uncaught_write_to_args_ends_the_run_as_a_lua_error_on_its_line() {
    for (write, refusal) in [
        (
            "args = { query = 'other' }",
            "args is read-only: assign to `var` or a section global instead",
        ),
        (
            "args.query = 'other'",
            "args is read-only: cannot set field 'query'",
        ),
        ("args[1] = 'other'", "args is read-only: cannot set field 1"),
    ] {
        let trace = papers(search_prompt(&format!("local ready = true\n{write}"))).run();
        let error = trace.error();
        assert_eq!(error.code, ErrorCode::LuaRuntime, "{write}: {error:?}");
        assert_eq!(
            error.prose,
            authored(ErrorCode::LuaRuntime, refusal),
            "{write}"
        );
        assert_eq!(error.at, at_line(12), "{write}: {error:?}");
    }
}

#[test]
fn a_field_the_caller_left_out_takes_its_declared_default_at_any_depth() {
    let top = search_prompt("return args.query .. ', limit ' .. args.limit");
    assert_eq!(papers(&top).run().text(), "papers, limit 3");
    let sent = Scenario::new(top)
        .args(r#"{"query":"papers","limit":10}"#)
        .run();
    assert_eq!(
        sent.text(),
        "papers, limit 10",
        "a field the caller sent keeps its value"
    );

    let nested = prompt(
        NESTED,
        &[("Only", "return tostring(args.opts and args.opts.depth)")],
    );
    let inside = Scenario::new(&nested).args(r#"{"opts":{}}"#).run();
    assert_eq!(
        inside.text(),
        "2",
        "an object the caller sent has its own defaults filled"
    );
    assert_eq!(
        Scenario::new(nested).run().text(),
        "nil",
        "an object with no default of its own stays left out"
    );
}

#[test]
fn a_null_for_a_field_that_is_not_required_counts_as_the_field_left_out() {
    let source = search_prompt(
        "
        assert(args.depth == nil, 'a null with no default reads nil, as a field left out does')
        return args.query .. ', limit ' .. args.limit",
    );
    let trace = Scenario::new(source)
        .args(r#"{"query":"papers","limit":null,"depth":null}"#)
        .run();
    assert_eq!(
        trace.text(),
        "papers, limit 3",
        "the default fills the null"
    );
}

#[test]
fn an_integer_argument_takes_a_whole_number_in_any_notation_and_holds_an_integer() {
    let source = PromptSource::new().front(SEARCH).prose(
        "Only",
        "
        Limit {{ args.limit }}.

        ```lua
        return prose .. ' ' .. math.type(args.limit) .. ' ' .. args.limit
        ```",
    );
    for (sent, held) in [("5.0", "5"), ("1e3", "1000"), ("7", "7"), ("-2.0", "-2")] {
        let trace = Scenario::new(source.build())
            .args(&format!(r#"{{"query":"papers","limit":{sent}}}"#))
            .run();
        assert_eq!(
            trace.text(),
            format!("Limit {held}. integer {held}"),
            "limit sent as {sent}"
        );
    }
}

#[test]
fn an_object_that_takes_undeclared_fields_keeps_them_as_sent() {
    let source = prompt(
        "args: { type: object, properties: { query: { type: string } }, \
         additionalProperties: true }",
        &[(
            "Only",
            "
            assert(args.spare == json.null, 'a null in an undeclared field is kept')
            return args.query .. '|' .. args.n .. '|' .. json.encode(args.extra)",
        )],
    );
    let trace = Scenario::new(source)
        .args(r#"{"query":"papers","n":2,"extra":{"deep":[1,null]},"spare":null}"#)
        .run();
    assert_eq!(trace.text(), r#"papers|2|{"deep":[1,null]}"#);
}

/// A prompt with no `args:` key whose one section returns `args` as JSON.
fn undeclared_prompt() -> String {
    one_section("return json.encode(args)")
}

#[test]
fn an_empty_object_runs_a_prompt_with_no_args_key() {
    let source = one_section(
        "
        assert(type(args) == 'table', 'args is a table with nothing passed')
        assert(next(args) == nil and #args == 0, 'an empty args holds nothing')
        assert(args.text == nil, 'the optional text reads nil when left out')
        return json.encode(args)",
    );
    assert_eq!(Scenario::new(source).args("{}").run().text(), "{}");
}

#[test]
fn a_prompt_with_no_args_key_takes_one_optional_string_named_text() {
    let trace = Scenario::new(undeclared_prompt())
        .args(r#"{"text":"hello"}"#)
        .run();
    assert_eq!(trace.text(), r#"{"text":"hello"}"#);
    let empty = Scenario::new(undeclared_prompt())
        .args(r#"{"text":""}"#)
        .run();
    assert_eq!(
        empty.text(),
        r#"{"text":""}"#,
        "an empty string is a value, not the field left out"
    );
    let null = Scenario::new(undeclared_prompt())
        .args(r#"{"text":null}"#)
        .run();
    assert_eq!(null.text(), "{}", "a null text counts as left out");
}

#[test]
fn a_handlers_args_parameter_is_the_calls_arguments_and_the_section_still_reads_the_runs() {
    let source = search_prompt(
        "
        local grab = tools.adhoc({ name = 'grab', description = 'Grab a value',
          parameters = { value = 'string' } }, function(args)
          return 'call ' .. args.value .. ' ' .. tostring(args.query)
        end)
        local probe = tools.adhoc({ name = 'probe', description = 'Read the run',
          parameters = { value = 'string' } }, function(call_args)
          return 'global ' .. args.query .. ' ' .. call_args.value
        end)
        local grabbed = tools.call(grab, { value = 'hi' })
        local probed = tools.call(probe, { value = 'yo' })
        return grabbed .. '|' .. probed .. '|run ' .. args.query .. ' ' .. tostring(args.value)",
    );
    assert_eq!(
        papers(source).run().text(),
        "call hi nil|global papers yo|run papers nil",
        "the parameter shadows the global inside the handler alone"
    );
}

#[path = "args_surface-chains.rs"]
mod chains;
#[path = "args_surface-refusals.rs"]
mod refusals;
