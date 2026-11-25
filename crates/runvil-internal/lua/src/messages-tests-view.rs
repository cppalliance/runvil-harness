//! Tests for the read-only record views a list's index and iteration
//! return, and the record JSON form they serialize through.

use mlua::{LuaSerdeExt, Value};
use serde_json::{Value as Json, json};

use super::list::refusal;
use crate::protocol::{
    ContentPart, JsonSize, MessageContent, MessageRecord, NonEmpty, ToolCallRecord, parse_record,
};
use crate::test_vm::{BuiltVm, TestVm};
use crate::tests::{assistant, system, tool_result, user};

/// One record table of each shape, held in the global `shapes` and
/// appended to `messages` in order: plain text, content parts, tool calls
/// with a string `meta`, and a tool result with a table `meta`.
#[track_caller]
fn shapes(vm: &BuiltVm) {
    vm.exec(
        "shapes = { \
           { role = 'system', content = 'be terse' }, \
           { role = 'user', content = { \
             { type = 'text', text = 'look' }, \
             { type = 'image_url', image_url = { url = 'data:image/png;base64,AA==' } } } }, \
           { role = 'assistant', content = 'working', meta = 'plan', \
             tool_calls = { { id = 'call_1', name = 'echo', arguments = { value = 'hi' } } } }, \
           { role = 'tool', content = 'echoed: hi', tool_call_id = 'call_1', \
             meta = { source = 'echo', score = 1 } } } \
         for _, record in ipairs(shapes) do messages:append(record) end",
    );
}

/// What `expr` evaluates to in its JSON form, or `None` when it is nil.
#[track_caller]
fn read(vm: &BuiltVm, expr: &str) -> Option<Json> {
    match vm.eval::<Value>(&format!("return {expr}")) {
        Value::Nil => None,
        value => Some(
            vm.lua()
                .from_value(value)
                .expect("the value converts to JSON"),
        ),
    }
}

#[test]
fn to_json_round_trips_every_record_shape_through_parse_record() {
    let records = [
        system("be terse"),
        MessageRecord {
            content: MessageContent::Parts(NonEmpty::new(
                ContentPart::Text("look".to_owned()),
                vec![ContentPart::ImageUrl(
                    "data:image/png;base64,AA==".to_owned(),
                )],
            )),
            ..user("")
        },
        MessageRecord {
            tool_calls: vec![ToolCallRecord {
                id: "call_1".to_owned(),
                name: "echo".to_owned(),
                arguments: json!({ "value": "hi" }),
            }],
            meta: Some(json!("plan")),
            ..assistant("working")
        },
        MessageRecord {
            meta: Some(json!({ "source": "echo", "score": 1 })),
            ..tool_result("call_1", "echoed: hi")
        },
    ];
    assert_eq!(
        records
            .iter()
            .map(MessageRecord::to_json)
            .collect::<Vec<_>>(),
        vec![
            json!({ "role": "system", "content": "be terse" }),
            json!({ "role": "user", "content": [
                { "type": "text", "text": "look" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AA==" } },
            ] }),
            json!({ "role": "assistant", "content": "working", "meta": "plan", "tool_calls": [
                { "id": "call_1", "name": "echo", "arguments": { "value": "hi" } },
            ] }),
            json!({
                "role": "tool",
                "content": "echoed: hi",
                "tool_call_id": "call_1",
                "meta": { "source": "echo", "score": 1 },
            }),
        ],
        "absent tool_calls, tool_call_id, and meta stay out of the JSON form"
    );
    for (position, record) in records.iter().enumerate() {
        assert_eq!(
            parse_record(position + 1, &record.to_json(), usize::MAX).as_ref(),
            Ok(record),
            "{record:?}"
        );
        assert_eq!(
            record.json_size(),
            JsonSize::of(&record.to_json()),
            "{record:?}"
        );
    }
    // An object, two strings of 6 and 8 bytes, and keys of 4 and 7.
    assert_eq!(
        records[0].json_size(),
        JsonSize {
            depth: 1,
            values: 3,
            bytes: 25
        }
    );
}

#[test]
fn a_view_inside_a_value_nests_as_the_record_it_stands_for() {
    let vm = TestVm::new().build();
    vm.exec("messages:assistant('seed')");
    // Each record holds the one before it, four levels down: a record
    // object, its calls, a call, and the arguments.
    let nest = "messages:assistant('', { \
                  { id = 'c', name = 'f', arguments = { prev = messages[#messages] } } })";
    for _ in 1..=31 {
        vm.exec(nest);
    }
    assert_eq!(
        refusal(&vm, nest),
        "messages[33] tool_calls must be JSON-representable: \
         value[1].arguments.prev is nested more than 127 levels deep"
    );
    let last = read(&vm, "messages[#messages]").expect("the list holds 32 records");
    assert_eq!(
        serde_json::from_str::<Json>(&last.to_string())
            .ok()
            .as_ref(),
        Some(&last),
        "the deepest record reads back from its JSON text"
    );
    // A record whose arguments nest `depth` tables is three levels deeper
    // than they are, and a record whose meta nests `depth` tables is one
    // level deeper.
    let with_arguments = |depth: usize| {
        format!(
            "local arguments = {{}} \
             local inner = arguments \
             for _ = 2, {depth} do inner.n = {{}} inner = inner.n end \
             messages:assistant('', {{ {{ id = 'c', name = 'f', arguments = arguments }} }})"
        )
    };
    let with_meta = |depth: usize| {
        format!(
            "local meta = {{}} \
             for _ = 2, {depth} do meta = {{ meta }} end \
             messages:user('x', meta)"
        )
    };
    // A record 127 deep is the deepest a builder takes. A view of it is
    // appended again, and its JSON text reads back.
    for deepest in [with_arguments(124), with_meta(126)] {
        vm.exec(&deepest);
        vm.exec("messages:append(messages[#messages])");
        let record = read(&vm, "messages[#messages]").expect("the list holds the record");
        assert_eq!(
            serde_json::from_str::<Json>(&record.to_string())
                .ok()
                .as_ref(),
            Some(&record)
        );
    }
    // Each field of these is within the limit by itself, and the record
    // around it is not.
    for too_deep in [with_arguments(125), with_meta(127)] {
        assert_eq!(
            refusal(&vm, &too_deep),
            "messages[37] is nested 128 levels deep as JSON; the limit is 127 levels"
        );
    }
    assert_eq!(read(&vm, "#messages"), Some(json!(36)));
    // A list is its items, so a view under another key of it is left out
    // and is not checked: one of a record 127 deep is no fault there, and
    // beside a fault it is not the fault, though its key comes first.
    vm.exec("messages:user('ok', { 'a', x = { messages[36] } })");
    assert_eq!(read(&vm, "messages[37].meta"), Some(json!(["a"])));
    assert_eq!(
        refusal(
            &vm,
            "messages:user('ok', { function() end, [true] = { messages[36] } })"
        ),
        "messages[38] meta must be JSON-representable: unsupported value type `function`"
    );
}

#[test]
fn a_view_inside_a_value_spends_the_budget_of_the_record_it_stands_for() {
    // 262,144 bytes of text and 32,768 values.
    let vm = TestVm::new().limits(262_144, 16).build();
    vm.exec("messages:user(string.rep('x', 4096))");
    let with_views = |count: usize| {
        format!(
            "local value = {{ z = function() end }} \
             for i = 1, {count} do value['k' .. i] = messages[1] end \
             messages:user('ok', value)"
        )
    };
    assert_eq!(
        refusal(&vm, &with_views(10)),
        "messages[2] meta must be JSON-representable: unsupported value type `function`"
    );
    assert_eq!(
        refusal(&vm, &with_views(65)),
        "messages[2] meta must be JSON-representable: the value is too large to convert",
        "65 views of 4,096 bytes hold more text than the budget"
    );
    vm.exec(
        "local list = {} \
         for i = 1, 1000 do list[i] = i end \
         messages:assistant('', { { id = 'c', name = 'f', arguments = { list = list } } })",
    );
    assert_eq!(
        refusal(
            &vm,
            "local value = {} \
             for i = 1, 40 do value[i] = messages[2] end \
             messages:user('ok', value)"
        ),
        "messages[3] meta must be JSON-representable: the value is too large to convert",
        "40 views of 1,009 values hold more values than the budget"
    );
}

#[test]
fn a_view_inside_a_value_spends_the_values_of_the_table_it_reads_as() {
    // 32,768 values. The record is three values, its object and two
    // strings, and so is the table of the same data. `rows` lists of
    // `each` of either, with `more` values beside them, are
    // `rows * (1 + 3 * each) + 1 + more` values.
    let vm = TestVm::new().limits(262_144, 16).build();
    vm.exec("messages:user('seed')");
    let refused = |item: &str, rows: usize, each: usize, more: &str| {
        refusal(
            &vm,
            &format!(
                "local item, row, value = {item}, {{}}, {{ {more} }} \
                 for i = 1, {each} do row[i] = item end \
                 for i = 1, {rows} do value['r' .. i] = row end \
                 messages:user('ok', value)"
            ),
        )
    };
    let too_large =
        "messages[2] meta must be JSON-representable: the value is too large to convert";
    for item in ["messages[1]", "{ role = 'user', content = 'seed' }"] {
        // The walk before the conversion: 32,768 values convert, and the
        // data is then too long for a `meta`, and 32,769 do not.
        assert_eq!(
            refused(item, 217, 50, ""),
            "messages[2] meta is 359896 bytes as JSON; the limit is 8192 bytes",
            "{item}"
        );
        assert_eq!(refused(item, 217, 50, "last = true"), too_large, "{item}");
        // The walk that names a fault: 32,768 values with a function among
        // them are refused for the function, and 32,769 are too large.
        assert_eq!(
            refused(item, 258, 42, "f = function() end"),
            "messages[2] meta must be JSON-representable: unsupported value type `function`",
            "{item}"
        );
        assert_eq!(
            refused(item, 258, 42, "f = function() end, last = true"),
            too_large,
            "{item}"
        );
    }
}

#[test]
fn a_record_holds_at_most_the_values_one_conversion_spends() {
    // 65,536 values. A record of `parts` text parts is its object, its
    // role, the list of the parts, and three values for each part, so
    // `3 + 3 * parts` values and those of its `meta`.
    let vm = TestVm::new().limits(524_288, 16).build();
    let user = |meta: &str| {
        format!(
            "local part, parts = {{ type = 'text', text = 'x' }}, {{}} \
             for i = 1, 21844 do parts[i] = part end \
             messages:user(parts, {meta})"
        )
    };
    // 65,536 values: the largest record a builder takes, and a view of it
    // is appended again.
    vm.exec(&user("1"));
    vm.exec("messages:append(messages[1])");
    // 65,537 values. The content is 65,533 of them and the `meta` two, so
    // each field converts by itself.
    assert_eq!(
        refusal(&vm, &user("{ 1 }")),
        "messages[3] holds 65537 values as JSON; the limit is 65536 values"
    );
    assert_eq!(read(&vm, "#messages"), Some(json!(2)));
}

#[test]
fn each_view_field_reads_like_the_record_table_and_a_view_is_userdata() {
    let vm = TestVm::new().build();
    shapes(&vm);
    for index in 1..=4 {
        assert_eq!(
            read(&vm, &format!("type(messages[{index}])")),
            Some(json!("userdata"))
        );
        for field in ["role", "content", "tool_calls", "tool_call_id", "meta"] {
            assert_eq!(
                read(&vm, &format!("messages[{index}].{field}")),
                read(&vm, &format!("shapes[{index}].{field}")),
                "messages[{index}].{field}"
            );
        }
    }
}

#[test]
fn an_integral_float_index_reads_like_its_integer_and_other_keys_read_nil() {
    let vm = TestVm::new().build();
    vm.exec("messages:user('u1'):append({ role = 'user', content = 'u2', extra = 1 })");
    assert_eq!(
        read(&vm, "messages[2.0]"),
        Some(json!({ "role": "user", "content": "u2" }))
    );
    assert_eq!(read(&vm, "messages[2]"), read(&vm, "messages[2.0]"));
    for expr in [
        "messages[1.5]",
        "messages.nope",
        "messages[true]",
        "messages[0]",
        "messages[-1]",
        "messages[3]",
        "messages[2].extra",
    ] {
        assert_eq!(read(&vm, expr), None, "{expr}");
    }
}

#[test]
fn assigning_any_view_field_raises_the_read_only_error() {
    let vm = TestVm::new().build();
    shapes(&vm);
    let before = vm.messages().to_records();
    for field in [
        "role",
        "content",
        "tool_calls",
        "tool_call_id",
        "meta",
        "extra",
    ] {
        assert_eq!(
            refusal(&vm, &format!("messages[3].{field} = 'x'")),
            "records read from messages are read-only; rewrite history with \
             models.compact(compactor, reason)",
            "{field}"
        );
    }
    assert_eq!(vm.messages().to_records(), before);
}

#[test]
fn pairs_over_a_view_visits_its_present_fields_in_order() {
    let vm = TestVm::new().build();
    shapes(&vm);
    let visits: Vec<String> = vm.eval(
        "local visits = {} \
         for i = 1, #messages do \
           local fields = {} \
           for key, value in pairs(messages[i]) do \
             fields[#fields + 1] = key .. ':' .. type(value) \
           end \
           visits[i] = table.concat(fields, ' ') \
         end \
         return visits",
    );
    assert_eq!(
        visits,
        [
            "role:string content:string",
            "role:string content:table",
            "role:string content:string tool_calls:table meta:string",
            "role:string content:string tool_call_id:string meta:table",
        ]
    );
}

#[test]
fn each_builders_last_argument_and_an_appended_meta_field_set_the_records_meta() {
    let vm = TestVm::new().build();
    vm.exec(
        "messages:system('s', { n = 1 })\
         :user('u', { n = 2 })\
         :assistant('a', { { id = 'call_1', name = 'echo' } }, { n = 3 })\
         :tool('r', 'call_1', { n = 4 })\
         :assistant('done', nil, { n = 5 })\
         :append({ role = 'user', content = 'more', meta = { n = 6 } })\
         :user('plain')",
    );
    let expected: Vec<Option<Json>> = (1..=6)
        .map(|n| Some(json!({ "n": n })))
        .chain([None])
        .collect();
    let held: Vec<Option<Json>> = vm
        .messages()
        .to_records()
        .into_iter()
        .map(|record| record.meta)
        .collect();
    assert_eq!(held, expected);
    for (position, meta) in expected.iter().enumerate() {
        let index = position + 1;
        assert_eq!(
            read(&vm, &format!("messages[{index}].meta")).as_ref(),
            meta.as_ref(),
            "messages[{index}].meta"
        );
    }
    assert_eq!(
        refusal(&vm, "messages:assistant('x', { n = 8 })"),
        "messages[8] tool_calls must be an array",
        "a table in assistant's second place is its tool calls"
    );
}

#[test]
fn a_view_appended_or_converted_gives_the_same_record_as_its_table_form() {
    let vm = TestVm::new().build();
    shapes(&vm);
    vm.exec("for i = 2, 4 do messages:append(messages[i]) end");
    let records = vm.messages().to_records();
    assert_eq!(records[4..], records[1..4]);
    for index in 1..=4 {
        let view = read(&vm, &format!("messages[{index}]")).expect("the view is present");
        let table = read(&vm, &format!("shapes[{index}]")).expect("the table is present");
        assert_eq!(
            parse_record(index, &view, usize::MAX),
            parse_record(index, &table, usize::MAX),
            "messages[{index}]"
        );
    }
}

#[test]
fn writing_into_a_views_nested_tables_leaves_the_record_unchanged() {
    let vm = TestVm::new().build();
    shapes(&vm);
    let before = vm.messages().to_records();
    let (content, tool_calls, meta): (Value, Value, Value) = vm.eval(
        "local parts_view, calls_view, result_view = messages[2], messages[3], messages[4] \
         local parts = parts_view.content \
         parts[1].text = 'changed' \
         parts[3] = { type = 'text', text = 'more' } \
         local calls = calls_view.tool_calls \
         calls[1].id = 'call_9' \
         calls[1].arguments.value = 'changed' \
         calls[2] = calls[1] \
         local meta = result_view.meta \
         meta.source = 'changed' \
         meta.extra = true \
         return parts_view.content, calls_view.tool_calls, result_view.meta",
    );
    assert_eq!(vm.messages().to_records(), before);
    let converted = |value: Value| -> Json {
        vm.lua()
            .from_value(value)
            .expect("the value converts to JSON")
    };
    assert_eq!(
        Some(converted(content)),
        read(&vm, "shapes[2].content"),
        "the same view reads its original content again"
    );
    assert_eq!(
        Some(converted(tool_calls)),
        read(&vm, "shapes[3].tool_calls"),
        "the same view reads its original tool calls again"
    );
    assert_eq!(
        Some(converted(meta)),
        read(&vm, "shapes[4].meta"),
        "the same view reads its original meta again"
    );
}

#[test]
fn pairs_ipairs_and_a_counted_loop_visit_every_record_in_order_as_views() {
    let vm = TestVm::new().build();
    let visit = "local seen = { pairs = {}, ipairs = {}, counted = {} } \
                 local function note(into, i, view) \
                   into[#into + 1] = i .. ':' .. type(view) .. ':' .. view.content \
                 end \
                 for i, view in pairs(messages) do note(seen.pairs, i, view) end \
                 for i, view in ipairs(messages) do note(seen.ipairs, i, view) end \
                 for i = 1, #messages do note(seen.counted, i, messages[i]) end \
                 return table.concat(seen.pairs, ' '), table.concat(seen.ipairs, ' '), \
                   table.concat(seen.counted, ' ')";
    let empty: (String, String, String) = vm.eval(visit);
    assert_eq!(empty, (String::new(), String::new(), String::new()));
    vm.exec("messages:user('u1'):assistant('a1'):user('u2')");
    let full: (String, String, String) = vm.eval(visit);
    let every = "1:userdata:u1 2:userdata:a1 3:userdata:u2".to_owned();
    assert_eq!(full, (every.clone(), every.clone(), every));
}

#[test]
fn a_pairs_loop_that_appends_ends_after_the_records_it_started_with() {
    let vm = TestVm::new().build();
    vm.exec("messages:user('u1'):assistant('a1'):user('u2')");
    let ended: (i64, i64) = vm.eval(
        "local n = 0 \
         for _ in pairs(messages) do \
           n = n + 1 \
           if n > 10 then error('runaway pairs') end \
           messages:user('x') \
         end \
         return n, #messages",
    );
    assert_eq!(ended, (3, 6));
}

#[test]
fn a_pairs_loop_stops_early_when_the_list_shrinks_below_its_next_index() {
    let vm = TestVm::new().build();
    vm.messages().set_compacting(true);
    vm.exec("messages:user('u1'):assistant('a1'):user('u2')");
    let seen: String = vm.eval(
        "local seen = {} \
         for i, view in pairs(messages) do \
           if i == 1 then messages:replace(2, 3) end \
           seen[#seen + 1] = i .. ':' .. view.content \
         end \
         return table.concat(seen, ' ')",
    );
    assert_eq!(seen, "1:u1");
}

#[test]
fn pairs_keeps_its_starting_length_while_ipairs_reads_the_live_list() {
    let walk = |iterate: &str| -> String {
        let vm = TestVm::new().build();
        vm.messages().set_compacting(true);
        vm.exec("messages:user('u1'):assistant('a1'):user('u2')");
        vm.eval(&format!(
            "local seen = {{}} \
             for i, view in {iterate}(messages) do \
               if i == 1 then messages:append({{ role = 'user', content = 'u3' }}) end \
               if i == 2 then messages:replace(3, 3, {{ role = 'user', content = 'u2b' }}) end \
               seen[#seen + 1] = i .. ':' .. view.content \
             end \
             return table.concat(seen, ' ')"
        ))
    };
    assert_eq!(walk("pairs"), "1:u1 2:a1 3:u2b", "pairs");
    assert_eq!(walk("ipairs"), "1:u1 2:a1 3:u2b 4:u3", "ipairs");
}
