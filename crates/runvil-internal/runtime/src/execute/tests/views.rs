//! A `var` table and an `args` table at the executor's boundaries: handed
//! to a tool call, to `tasks.fanout`, to a spawn's `item`, and to a bare
//! global in prose, each arrives as the data it stands for, and so does
//! an `args` table written to `var` or held as a message record's `meta`.

use runvil_prompt::test_support::{PromptSource, one_section, prompt};

use super::*;

/// A declaration of an object that takes any field, so each `args` test
/// passes the shapes it hands on.
const ANY_FIELDS: &str = "args: { type: object, additionalProperties: true }";

/// A tool that answers with its arguments as JSON text.
#[track_caller]
fn args_tool() -> FnTool {
    FnTool::new("tools/args", json!({ "type": "object" }), |args| {
        Ok(ToolOutput::trusted(args.to_string()))
    })
    .description("Answer with the arguments as JSON text.")
}

#[test]
fn a_var_table_handed_to_a_tool_call_arrives_as_its_data() {
    let source = one_section(
        "var.payload = { value = 'hi', tags = { 'a', 'b' }, none = json.array() }\n\
         return tools.call('tools/args', var.payload) .. ' ' ..\n\
                tools.call('tools/args', { inner = var.payload.tags })",
    );
    let trace = Scenario::new(source).tool(args_tool()).run();
    assert_eq!(
        trace.text(),
        r#"{"none":[],"tags":["a","b"],"value":"hi"} {"inner":["a","b"]}"#
    );
}

#[test]
fn a_var_list_fans_out_one_arm_for_each_member() {
    let source = prompt(
        "",
        &[
            (
                "Parent",
                "var.rows = { { name = 'alpha' }, { name = 'beta' } }\n\
                 local r = tasks.fanout('Worker', var.rows)\n\
                 return #r .. ':' .. r[1].text .. ',' .. r[2].text .. ':' .. r[2].item.name",
            ),
            ("### Worker", "return item.name .. '-' .. sys.index"),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "2:alpha-1,beta-2:beta");
}

#[test]
fn a_var_table_as_a_spawn_item_arrives_as_its_data() {
    let source = prompt(
        "",
        &[
            (
                "Parent",
                "var.rows = { { name = 'alpha' } }\n\
                 local task = tasks.spawn('Worker', { item = { row = var.rows[1] } })\n\
                 local _, ok, result = tasks.join_any({ task })\n\
                 return tostring(ok) .. ':' .. result",
            ),
            ("### Worker", "return item.row.name"),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "true:alpha");
}

#[test]
fn a_bare_global_holding_a_var_table_renders_as_its_data_in_prose() {
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        var.files = { 'a.md', 'b.md' }
        files = var.files
        first = { name = var.files[1], all = var.files }
        ```

        Files: {{ files }}; first {{ first.name }} of {{ first.all }}.

        ```lua
        return prose
        ```",
    );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "Files: [\"a.md\",\"b.md\"]; first a.md of [\"a.md\",\"b.md\"]."
    );
}

#[test]
fn an_args_table_handed_to_a_tool_call_arrives_as_its_data() {
    let source = prompt(
        ANY_FIELDS,
        &[(
            "Only",
            "return tools.call('tools/args', args) .. ' ' ..\n\
                    tools.call('tools/args', { inner = args.tags, row = args.rows[1] })",
        )],
    );
    let trace = Scenario::new(source)
        .args(r#"{"value":"hi","tags":["a","b"],"none":[],"rows":[{"name":"alpha"}]}"#)
        .tool(args_tool())
        .run();
    assert_eq!(
        trace.text(),
        r#"{"none":[],"rows":[{"name":"alpha"}],"tags":["a","b"],"value":"hi"} {"inner":["a","b"],"row":{"name":"alpha"}}"#
    );
}

#[test]
fn an_args_list_fans_out_one_arm_for_each_member() {
    let source = prompt(
        ANY_FIELDS,
        &[
            (
                "Parent",
                "local r = tasks.fanout('Worker', args.rows)\n\
                 return #r .. ':' .. r[1].text .. ',' .. r[2].text .. ':' .. r[2].item.name",
            ),
            ("### Worker", "return item.name .. '-' .. sys.index"),
        ],
    );
    let trace = Scenario::new(source)
        .args(r#"{"rows":[{"name":"alpha"},{"name":"beta"}]}"#)
        .run();
    assert_eq!(trace.text(), "2:alpha-1,beta-2:beta");
}

#[test]
fn an_args_table_as_a_spawn_item_arrives_as_its_data() {
    let source = prompt(
        ANY_FIELDS,
        &[
            (
                "Parent",
                "local task = tasks.spawn('Worker', { item = { row = args.rows[1], whole = args } })\n\
                 local _, ok, result = tasks.join_any({ task })\n\
                 return tostring(ok) .. ':' .. result",
            ),
            (
                "### Worker",
                "return item.row.name .. ' of ' .. json.encode(item.whole)",
            ),
        ],
    );
    let trace = Scenario::new(source)
        .args(r#"{"rows":[{"name":"alpha"}]}"#)
        .run();
    assert_eq!(trace.text(), r#"true:alpha of {"rows":[{"name":"alpha"}]}"#);
}

#[test]
fn an_args_table_written_to_var_is_its_data_which_var_can_then_change() {
    let source = prompt(
        ANY_FIELDS,
        &[
            (
                "First",
                "var.copy = args\n\
                 var.tags = args.tags\n\
                 var.copy.value = 'rewritten'\n\
                 var.tags[#var.tags + 1] = 'c'",
            ),
            (
                "Second",
                "return json.encode(var.copy) .. ' ' .. json.encode(var.tags) .. ' ' ..\n\
                        args.value .. ' ' .. #args.tags",
            ),
        ],
    );
    let trace = Scenario::new(source)
        .args(r#"{"value":"hi","tags":["a","b"]}"#)
        .run();
    assert_eq!(
        trace.text(),
        r#"{"tags":["a","b"],"value":"rewritten"} ["a","b","c"] hi 2"#,
        "the write to `var` copies the data, so `args` reads as the caller sent it"
    );
}

#[test]
fn an_args_table_as_a_message_records_meta_arrives_as_its_data() {
    let source = prompt(
        ANY_FIELDS,
        &[(
            "Only",
            "messages:user('look', args)\n\
             messages:append({ role = 'user', content = 'go on', meta = { tags = args.tags } })\n\
             return json.encode(messages[1].meta) .. ' ' .. json.encode(messages[2].meta)",
        )],
    );
    let trace = Scenario::new(source)
        .args(r#"{"value":"hi","tags":["a","b"]}"#)
        .run();
    assert_eq!(
        trace.text(),
        r#"{"tags":["a","b"],"value":"hi"} {"tags":["a","b"]}"#
    );
}

#[test]
fn a_bare_global_holding_an_args_table_renders_as_its_data_in_prose() {
    let source = PromptSource::new().front(ANY_FIELDS).prose(
        "Only",
        "
        ```lua
        files = args.files
        first = { name = args.files[1], all = args.files, whole = args }
        ```

        Files: {{ files }}; first {{ first.name }} of {{ first.all }}; whole {{ first.whole }}.

        ```lua
        return prose
        ```",
    );
    let trace = Scenario::new(source.build())
        .args(r#"{"files":["a.md","b.md"]}"#)
        .run();
    assert_eq!(
        trace.text(),
        r#"Files: ["a.md","b.md"]; first a.md of ["a.md","b.md"]; whole {"files":["a.md","b.md"]}."#
    );
}
