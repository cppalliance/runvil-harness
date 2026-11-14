//! A `var` table at the executor's boundaries: handed to a tool call, to
//! `tasks.fanout`, to a spawn's `item`, and to a bare global in prose, it
//! arrives as the data it stands for.

use runvil_prompt::test_support::{PromptSource, one_section, prompt};

use super::*;

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
