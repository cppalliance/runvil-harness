//! The fanout collection's member enumeration and the item renderer, the
//! two captures the `fanout` shim runs over.
//!
//! A section's Lua calls `tasks.fanout(worker, collection)`; the collection
//! is any Lua table, and the shim spawns one arm per member in a fixed
//! order. The list comes first, in order: the indices from 1 up to the
//! first that holds nothing, which [`listed`] counts. Every other entry
//! follows as a `{ key, value }` pair table, in the order of [`WalkPlace`],
//! because the order of Lua's own walk depends on the string hash seed and
//! a fanout's arm order must not. That is the order the Engine's `pairs`
//! visits the keys in, so the arms of a collection come as a `pairs` loop
//! over it meets its keys. A member of the list arrives as the arm's
//! `item` value as itself; a keyed member arrives as the pair table
//! (`item.key` / `item.value`), and so does an index past a gap, which
//! comes before every key that is no integer from 1 up. A collection that
//! is a sealed view enumerates as the table it stands for.

use mlua::{Lua, LuaSerdeExt, Table, Value};

use crate::error::{self, Result, make_lua_runtime_error};
use crate::meta_names::MetaNames;
use crate::order::{FirstRefusal, KeyError, WalkPlace, listed, sort_key, sort_list};
use crate::proxy::{plain, view_data};

/// Enumerates fanout's collection argument as the sequence of members the
/// shim spawns arms over: the list in order, then every other entry as a
/// `{ key = k, value = v }` pair table, in the order of [`WalkPlace`].
/// Members stay Lua values; the `spawn` request converts each one at its
/// own boundary.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the value is not a table (the message says
/// how to read the items of a list in a section), when a member is a
/// function, userdata, or thread (the error names the member's index),
/// when the table is weak, or when a hash key is not a string, number, or
/// boolean. Of several
/// members to refuse in the list, the failure is that of the first, and
/// among the keyed members that of the one whose key comes first.
pub(crate) fn collection_members(lua: &Lua, collection: &Value) -> Result<Table> {
    let Value::Table(table) = collection else {
        return Err(make_lua_runtime_error(format!(
            "tasks.fanout's second parameter is a collection, got {}; for the items of a list \
             in a section, pass prompt.section(path, \"list\").items",
            collection.type_name()
        )));
    };
    // The members go to a Lua list as they are read, so the collector can
    // run in the middle of the read and take a weak table's unread ones.
    // A weak view on the way to the members is refused as well, and so is
    // a table whose metatable is weak.
    let table = &view_data(lua, &MetaNames::default(), table)
        .map_err(error::make_lua_error)?
        .map_err(|reason| {
            make_lua_runtime_error(format!("tasks.fanout's collection is {reason}"))
        })?;
    let border = listed(table).map_err(error::make_lua_error)?;
    let members = lua
        .create_table_with_capacity(border, 0)
        .map_err(error::make_lua_error)?;
    for index in 1..=border {
        let member = table
            .raw_get::<Value>(index)
            .map_err(error::make_lua_error)?;
        check_member(&member, &index.to_string())?;
        members
            .raw_set(index, member)
            .map_err(error::make_lua_error)?;
    }
    // Rust holds no reference to a pair past the step that reads it: the
    // pair goes to `members` as Lua's own walk meets it, and only its place
    // in the order stays here. Of the pairs to refuse, only the refusal
    // with the least key is kept.
    let mut places = Vec::new();
    let mut refused = FirstRefusal::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, member) = pair.map_err(error::make_lua_error)?;
        // The list was already emitted above, in order.
        if let Value::Integer(index) = &key
            && usize::try_from(*index).is_ok_and(|index| (1..=border).contains(&index))
        {
            continue;
        }
        match keyed_member(&key, &member) {
            Ok(position) => {
                let entry = lua
                    .create_table_with_capacity(0, 2)
                    .map_err(error::make_lua_error)?;
                entry.raw_set("key", key).map_err(error::make_lua_error)?;
                entry
                    .raw_set("value", member)
                    .map_err(error::make_lua_error)?;
                places.push(position);
                members
                    .raw_set(border + places.len(), entry)
                    .map_err(error::make_lua_error)?;
            }
            Err(error) => refused.refuse(&key, error),
        }
    }
    refused.into_result()?;
    sort_list(&members, border, &places, WalkPlace::compare).map_err(error::make_lua_error)?;
    Ok(members)
}

/// The place of `key` among the keyed members of a collection, when the
/// key and its `member` can be an arm's item.
///
/// The shared classifier yields each scalar key's sort position and its
/// diagnostic label; the failure is mapped to fanout's own message here, so
/// a non-scalar key is rejected before any later code path.
fn keyed_member(key: &Value, member: &Value) -> Result<WalkPlace> {
    let (position, key_label) = sort_key(key).map_err(|error| match error {
        KeyError::NotFinite => {
            make_lua_runtime_error("fanout collection key is not a finite number".to_owned())
        }
        KeyError::NotUtf8(source) => error::make_lua_error(source),
        KeyError::Unsortable(type_name) => make_lua_runtime_error(format!(
            "fanout collection key must be a string, number, or boolean, got {type_name}"
        )),
    })?;
    check_member(member, &key_label)?;
    Ok(WalkPlace::from(position))
}

/// Rejects a member that cannot cross into an arm.
///
/// Functions, userdata, and threads cannot serialize, so they are rejected at
/// the call boundary with an error naming the member's index rather than the
/// spawn's own type error.
fn check_member(member: &Value, index: &str) -> Result<()> {
    match member {
        Value::Function(_) | Value::UserData(_) | Value::Thread(_) => {
            Err(make_lua_runtime_error(format!(
                "fanout collection member at index {index} is a {}; members must be data",
                member.type_name()
            )))
        }
        _ => Ok(()),
    }
}

/// Renders a fanout arm's item for prose substitution and stub text:
/// strings verbatim, numbers and booleans in their natural string form, and
/// arrays, objects, and null as compact JSON.
#[must_use]
pub fn render_item(item: &serde_json::Value) -> String {
    match item {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        // Serializing a `Value` cannot fail; the default is unreachable.
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(item).unwrap_or_default()
        }
    }
}

/// Renders a Lua member value as [`render_item`] renders its JSON form: the
/// `fanout` shim's capture for the tool-loop-exhausted stub, so the stub's
/// heading reads exactly as `{{ item }}` would render the same member.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the value has no JSON form.
pub(crate) fn render_item_value(lua: &Lua, item: Value) -> Result<String> {
    let json: serde_json::Value = plain(lua, item)
        .and_then(|item| lua.from_value(item))
        .map_err(error::make_lua_error)?;
    Ok(render_item(&json))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::iteration::install_deterministic_iteration;
    use crate::walk_budget::HeapCeiling;

    fn eval(lua: &mlua::Lua, source: &str) -> Value {
        lua.load(source).eval::<Value>().expect("chunk evaluates")
    }

    /// Reads the member sequence back as JSON for comparison.
    fn members_json(lua: &Lua, source: &str) -> Vec<serde_json::Value> {
        let value = eval(lua, source);
        let members = collection_members(lua, &value).expect("the collection enumerates");
        lua.from_value(Value::Table(members))
            .expect("the members are JSON data")
    }

    #[test]
    fn collection_members_rejects_a_non_table() {
        let lua = mlua::Lua::new();
        for (source, kind) in [
            ("return 'Items'", "string"),
            ("return 5", "integer"),
            ("return true", "boolean"),
            ("return nil", "nil"),
        ] {
            let value = eval(&lua, source);
            let error =
                collection_members(&lua, &value).expect_err("a non-table is not a collection");
            assert_eq!(
                error.to_string(),
                format!(
                    "tasks.fanout's second parameter is a collection, got {kind}; for the items \
                     of a list in a section, pass prompt.section(path, \"list\").items"
                ),
                "{source}"
            );
        }
    }

    #[test]
    fn collection_members_preserves_array_order_and_member_types() {
        let lua = mlua::Lua::new();
        let items = members_json(&lua, "return {'b', 2, true, {nested='x'}}");
        assert_eq!(
            items,
            vec![json!("b"), json!(2), json!(true), json!({"nested": "x"})]
        );
    }

    #[test]
    fn collection_members_sorts_the_hash_part_by_key() {
        let lua = mlua::Lua::new();
        // Five string keys: `pairs` order varies per state, the members do not.
        let items = members_json(
            &lua,
            "return {zeta=1, alpha='two', mid=true, beta=4, omega=5}",
        );
        assert_eq!(
            items,
            vec![
                json!({"key": "alpha", "value": "two"}),
                json!({"key": "beta", "value": 4}),
                json!({"key": "mid", "value": true}),
                json!({"key": "omega", "value": 5}),
                json!({"key": "zeta", "value": 1}),
            ]
        );
    }

    #[test]
    fn collection_members_orders_mixed_keys_indices_then_booleans_numbers_and_strings() {
        let lua = mlua::Lua::new();
        let items = members_json(
            &lua,
            "return {[true]='t', [7]='seven', b='bee', [false]='f', [2.5]='half', a='ay'}",
        );
        assert_eq!(
            items,
            vec![
                json!({"key": 7, "value": "seven"}),
                json!({"key": false, "value": "f"}),
                json!({"key": true, "value": "t"}),
                json!({"key": 2.5, "value": "half"}),
                json!({"key": "a", "value": "ay"}),
                json!({"key": "b", "value": "bee"}),
            ]
        );
    }

    #[test]
    fn collection_members_orders_large_integer_keys_exactly() {
        // 2^53 and 2^53 + 1 round to the same f64: a float-converting sort
        // would tie them and leave their order to `pairs`. Integer keys
        // compare as integers. The integers from 1 up come first, and a
        // float key sorts exactly against the integers below 1: one beyond
        // i64's range lands past every one of them, one inside it between
        // its integer neighbors.
        let lua = mlua::Lua::new();
        let items = members_json(
            &lua,
            "return {[9007199254740993]='b', [9007199254740992]='a', [1e300]='big', \
             [-1e300]='small', [2.5]='half', [2]='two', [3]='three', [-2]='low', \
             [-1.5]='between', [-1]='near', [0]='zero'}",
        );
        assert_eq!(
            items,
            vec![
                json!({"key": 2, "value": "two"}),
                json!({"key": 3, "value": "three"}),
                json!({"key": 9_007_199_254_740_992_i64, "value": "a"}),
                json!({"key": 9_007_199_254_740_993_i64, "value": "b"}),
                json!({"key": -1e300, "value": "small"}),
                json!({"key": -2, "value": "low"}),
                json!({"key": -1.5, "value": "between"}),
                json!({"key": -1, "value": "near"}),
                json!({"key": 0, "value": "zero"}),
                json!({"key": 2.5, "value": "half"}),
                json!({"key": 1e300, "value": "big"}),
            ]
        );
    }

    #[test]
    fn collection_members_emits_the_array_part_before_the_hash_part() {
        let lua = mlua::Lua::new();
        let items = members_json(&lua, "return {'a', 'b', extra='c'}");
        assert_eq!(
            items,
            vec![
                json!("a"),
                json!("b"),
                json!({"key": "extra", "value": "c"})
            ]
        );
    }

    #[test]
    fn collection_members_keeps_integer_keys_outside_the_border_as_pairs() {
        let lua = mlua::Lua::new();
        let items = members_json(&lua, "return {[5]='five'}");
        assert_eq!(items, vec![json!({"key": 5, "value": "five"})]);
    }

    #[test]
    fn collection_members_enumerates_a_view_as_the_table_it_stands_for() {
        let lua = mlua::Lua::new();
        let items = members_json(
            &lua,
            "return setmetatable({}, { __index = { 'a', 'b', extra = 'c' }, __metatable = 'view' })",
        );
        assert_eq!(
            items,
            vec![
                json!("a"),
                json!("b"),
                json!({"key": "extra", "value": "c"})
            ]
        );
    }

    #[test]
    fn collection_members_returns_an_empty_sequence_for_an_empty_table() {
        let lua = mlua::Lua::new();
        let items = members_json(&lua, "return {}");
        assert!(items.is_empty());
    }

    #[test]
    fn collection_members_rejects_a_function_member_naming_its_index() {
        let lua = mlua::Lua::new();
        let value = eval(&lua, "return {'a', function() end}");
        let error = collection_members(&lua, &value).expect_err("a function member must error");
        let rendered = error.to_string();
        assert_eq!(
            rendered,
            "fanout collection member at index 2 is a function; members must be data"
        );

        let value = eval(&lua, "return {cb=function() end}");
        let error = collection_members(&lua, &value)
            .expect_err("a hash-position function member must error");
        let rendered = error.to_string();
        assert!(rendered.contains("index cb"), "error was: {rendered}");
        assert!(rendered.contains("function"), "error was: {rendered}");
    }

    struct Stub;
    impl mlua::UserData for Stub {}

    #[test]
    fn collection_members_refuses_the_keyed_member_whose_key_comes_first_on_every_state() {
        // Each fresh state walks the hash part under its own hash seed.
        // Every member of a case is refused, so the refusal is that of the
        // first key in the shared order.
        for (collection, expected) in [
            (
                "return { zulu = type, mike = type, alpha = type, oscar = type, bravo = type }",
                "fanout collection member at index alpha is a function; members must be data",
            ),
            (
                "return { [{}] = 1, [type] = 1, zulu = type, [7.5] = type }",
                "fanout collection member at index 7.5 is a function; members must be data",
            ),
            (
                "return { [{}] = 1, [type] = 1 }",
                "fanout collection key must be a string, number, or boolean, got function",
            ),
        ] {
            for state in 0..16 {
                let lua = mlua::Lua::new();
                let value = eval(&lua, collection);
                let error = collection_members(&lua, &value).expect_err("every member is refused");
                assert_eq!(error.to_string(), expected, "state {state}: {collection}");
            }
        }
    }

    #[test]
    fn collection_members_ends_the_list_at_the_first_gap_on_every_state() {
        // `#t` of this table is 3 in one state and 5 in another: the
        // indices sit in the hash part, where Lua looks for the end of the
        // list with the seed of the state. A list that ended at `#t` held
        // a nil member and `e` as itself in the states that gave 5.
        for state in 0..32 {
            let lua = mlua::Lua::new();
            assert_eq!(
                members_json(
                    &lua,
                    "return { [1] = 'a', [2] = 'b', [3] = 'c', [5] = 'e', [false] = 'f' }"
                ),
                vec![
                    json!("a"),
                    json!("b"),
                    json!("c"),
                    json!({ "key": 5, "value": "e" }),
                    json!({ "key": false, "value": "f" }),
                ],
                "state {state}"
            );
        }
    }

    #[test]
    fn collection_members_come_in_the_order_pairs_visits_their_keys() {
        // The Engine's `pairs` is the other walk of a table an author
        // has, so it says what the order of the members is: a member of
        // the list stands for its index, and a pair for its key.
        let lua = mlua::Lua::new();
        install_deterministic_iteration(&lua).expect("the walk installs");
        for collection in [
            "{ [1] = 'a', [2] = 'b', [3] = 'c', [5] = 'e', [false] = 'f' }",
            "{ [true] = 't', [7] = 'seven', b = 'bee', [false] = 'f', [2.5] = 'half', a = 'ay' }",
            "{ 'a', nil, 'c', [0] = 'z', [-1] = 'm', [1.5] = 'h', x = 'x' }",
            "{ [10] = 'x', [20] = 'y', [15.5] = 'h', [false] = 'f', [-3] = 'n' }",
        ] {
            let walked = eval(
                &lua,
                &format!(
                    "local keys = {{}} \
                     for key in pairs({collection}) do keys[#keys + 1] = key end \
                     return keys"
                ),
            );
            let walked = lua
                .from_value::<Vec<serde_json::Value>>(walked)
                .expect("the keys are JSON data");
            let members = members_json(&lua, &format!("return {collection}"))
                .into_iter()
                .zip(1..)
                .map(|(member, index)| match member {
                    serde_json::Value::Object(pair) => pair["key"].clone(),
                    _ => json!(index),
                })
                .collect::<Vec<_>>();
            assert_eq!(members, walked, "{collection}");
        }
    }

    #[test]
    fn collection_members_refuses_a_weak_collection() {
        // The values are in use elsewhere, so the collector takes none of
        // them: the refusal is for the table, whatever it holds.
        let lua = mlua::Lua::new();
        for mode in ["k", "v"] {
            let value = eval(
                &lua,
                &format!(
                    "KEEP = {{ {{}}, {{}} }} \
                     return setmetatable({{ KEEP[1], a = KEEP[2] }}, {{ __mode = '{mode}' }})"
                ),
            );
            assert_eq!(
                collection_members(&lua, &value)
                    .expect_err("a weak collection is refused")
                    .to_string(),
                "tasks.fanout's collection is a weak table, one whose metatable sets __mode: the \
                 entries the collector leaves in it are not the same in two runs",
                "{mode}"
            );
        }
        // An empty weak table in the shape of a view of a list is refused
        // too, and not read as that list.
        let view = eval(
            &lua,
            "return setmetatable({}, { __mode = 'v', __metatable = false, __index = { 'a' } })",
        );
        assert_eq!(
            collection_members(&lua, &view)
                .expect_err("a weak view is refused")
                .to_string(),
            "tasks.fanout's collection is a weak table, one whose metatable sets __mode: the \
             entries the collector leaves in it are not the same in two runs"
        );
        // Five views that share one `__mode` of 1 MiB, which makes none of
        // them weak: one more scan than a heap of 4 MiB allows.
        lua.set_app_data(HeapCeiling(4 * 1024 * 1024));
        let chain = eval(
            &lua,
            "local mode = string.rep('x', 2^20) \
             local t = { 'a' } \
             for _ = 1, 5 do \
               t = setmetatable({}, { __index = t, __metatable = false, __mode = mode }) \
             end \
             return t",
        );
        assert_eq!(
            collection_members(&lua, &chain)
                .expect_err("a chain past the budget is refused")
                .to_string(),
            "tasks.fanout's collection is a table with 5242880 bytes of __mode to scan: one call \
             scans at most 4194304 bytes"
        );
    }

    #[test]
    fn collection_members_holds_no_reference_to_each_keyed_member() {
        // `mlua` panics when the stack that holds the Lua values Rust refers
        // to cannot grow, and a reference to each of 50,000 keys and members
        // needs it to. With this little memory left the stack cannot grow,
        // and the members do not fit either, which Lua reports.
        let lua = mlua::Lua::new();
        let value = eval(
            &lua,
            "local t = {} for i = 1, 50000 do t[i + 0.5] = 'x' end return t",
        );
        lua.set_memory_limit(lua.used_memory() + 256 * 1024)
            .expect("the limit is set");
        assert_eq!(
            collection_members(&lua, &value)
                .expect_err("the members do not fit")
                .to_string(),
            "memory error: not enough memory"
        );
    }

    #[test]
    fn collection_members_rejects_a_userdata_member_naming_its_index() {
        let lua = mlua::Lua::new();
        let userdata = lua.create_userdata(Stub).expect("userdata creates");
        let table = lua.create_table().expect("table creates");
        table.raw_set(1, userdata).expect("member installs");
        let error = collection_members(&lua, &Value::Table(table))
            .expect_err("a userdata member must error");
        let rendered = error.to_string();
        assert!(rendered.contains("index 1"), "error was: {rendered}");
        assert!(rendered.contains("userdata"), "error was: {rendered}");
    }

    #[test]
    fn collection_members_accepts_a_json_null_member_as_json_null() {
        let lua = mlua::Lua::new();
        let table = lua.create_table().expect("table creates");
        table.raw_set(1, lua.null()).expect("member installs");
        let members =
            collection_members(&lua, &Value::Table(table)).expect("a null member is data");
        let items: Vec<serde_json::Value> = lua
            .from_value(Value::Table(members))
            .expect("the members are JSON data");
        assert_eq!(items, vec![serde_json::Value::Null]);
    }

    #[test]
    fn collection_members_rejects_a_non_scalar_key() {
        let lua = mlua::Lua::new();
        let value = eval(&lua, "local t = {}; t[{}] = 'x'; return t");
        let error = collection_members(&lua, &value).expect_err("a table key must error");
        assert_eq!(
            error.to_string(),
            "fanout collection key must be a string, number, or boolean, got table"
        );
    }

    #[test]
    fn collection_members_keeps_member_identity() {
        // Members are handed back as the author's own values, not copies:
        // the arm's `item` converts at the spawn boundary, and the result's
        // `.item` is the value the author passed in.
        let lua = mlua::Lua::new();
        let value = eval(&lua, "return {{n=3}}");
        let members = collection_members(&lua, &value).expect("the collection enumerates");
        let Value::Table(source) = &value else {
            panic!("the collection is a table");
        };
        let original: Value = source.raw_get(1).expect("the member reads");
        let member: Value = members.raw_get(1).expect("the member reads");
        assert_eq!(member, original, "the member is the author's own table");
    }

    #[test]
    fn render_item_renders_by_type() {
        assert_eq!(render_item(&json!("plain")), "plain");
        assert_eq!(render_item(&json!(7)), "7");
        assert_eq!(render_item(&json!(2.5)), "2.5");
        assert_eq!(render_item(&json!(true)), "true");
        assert_eq!(render_item(&json!([7, "x"])), "[7,\"x\"]");
        assert_eq!(
            render_item(&json!({"key": "alpha", "value": 1})),
            "{\"key\":\"alpha\",\"value\":1}"
        );
        assert_eq!(render_item(&serde_json::Value::Null), "null");
    }

    #[test]
    fn render_item_value_renders_a_lua_member_through_its_json_form() {
        let lua = mlua::Lua::new();
        let table = eval(&lua, "return {key='alpha', value=1}");
        assert_eq!(
            render_item_value(&lua, table).expect("a data table renders"),
            "{\"key\":\"alpha\",\"value\":1}"
        );
        let text = eval(&lua, "return 'alpha'");
        assert_eq!(
            render_item_value(&lua, text).expect("a string renders"),
            "alpha"
        );
        let view = eval(
            &lua,
            "return setmetatable({}, { __index = { key = 'alpha', value = 1 }, __metatable = 'view' })",
        );
        assert_eq!(
            render_item_value(&lua, view).expect("a view renders as its data"),
            "{\"key\":\"alpha\",\"value\":1}"
        );
    }
}
