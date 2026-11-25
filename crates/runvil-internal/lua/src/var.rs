//! The guarded `var` proxy that sandboxed author code reads and writes.

#[cfg(test)]
use std::cell::Cell;
use std::sync::Arc;

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::Value as Json;

use crate::error::{self, Result, author_refusal, make_lua_runtime_error};
use crate::json::{NOT_UTF8, to_stored_json};
use crate::plain_json::json_of;
use crate::proxy::{index_step, key_path, key_step, name_step, view_len};

/// The registry key holding the `var` proxy's hidden data table.
///
/// The registry is unreachable from sandboxed author code (no `debug`
/// library), so only Engine code holding a `&Lua` can read or replace the data
/// table behind the guarded global.
const VAR_DATA_REGISTRY: &str = "runvil.var_data";

/// The registry key holding the `var` proxy table itself, so read-back can
/// detect an author reassigning the global (`var = 5`), which would
/// otherwise silently strand the hidden data table.
const VAR_PROXY_REGISTRY: &str = "runvil.var_proxy";

/// Registry map from every guarded proxy table to its hidden data table.
const VAR_GUARDED_DATA_REGISTRY: &str = "runvil.var_guarded_data";

/// The step of the root `var` table, whose fields are all named.
const ROOT_STEP: &str = "var";

/// 2^63, one past the largest integer a Lua table key holds.
const ABOVE_INTEGERS: f64 = 9_223_372_036_854_775_808.0;

#[cfg(test)]
thread_local! {
    /// How many paths [`Place::path`] wrote on this thread. A write that
    /// is taken writes none, and a test holds it to that.
    pub(crate) static PATHS_BUILT: Cell<usize> = const { Cell::new(0) };
}

/// The integer `key` is as a table key: an integer as it is, and a float
/// with no fractional part as the integer it equals, which is the key a
/// Lua table stores for it. A metamethod receives such a float as written.
fn whole_key(key: &Value) -> Option<i64> {
    match key {
        Value::Integer(index) => Some(*index),
        Value::Number(float)
            if float.fract() == 0.0 && (-ABOVE_INTEGERS..ABOVE_INTEGERS).contains(float) =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the float is a whole number inside i64's range"
            )]
            let index = *float as i64;
            Some(index)
        }
        _ => None,
    }
}

/// Why the table at `place`, whose data is `data`, cannot take a write
/// under `key`, or `None` when it can. `removes` says that the write
/// assigns nil.
///
/// A table of JSON data is an object, whose keys are strings, or a list,
/// whose keys are the indexes 1 to its length. The root is an object. A
/// write that would leave a table as neither is refused where it is
/// written, because the read-back of `var` would fail on the table, or
/// lose a part of it, long after the write:
///
/// - a key that is neither a string nor an integer
/// - a string key that is not UTF-8
/// - a named field in a list, and a list item in a table that holds named
///   fields
/// - an index below 1, or past the next item of the list
/// - the removal of an item that is not the last of its list
///
/// A nil under any other key removes what the key holds, or nothing.
fn write_refusal(
    lua: &Lua,
    place: &Place,
    key: &Value,
    removes: bool,
    data: &mlua::Table,
) -> Option<String> {
    let len = data.raw_len();
    let marked = data
        .metatable()
        .is_some_and(|metatable| metatable == lua.array_metatable());
    if let Value::String(name) = key {
        let is_list = marked || len > 0;
        if removes || (!is_list && name.to_str().is_ok()) {
            return None;
        }
        let path = place.path();
        let field = key_path(&path, key);
        return Some(if is_list {
            format!("{field} is a named field, and {path} is a list")
        } else {
            format!("{field} {NOT_UTF8}")
        });
    }
    let index = whole_key(key).filter(|_| !place.is_root());
    let Some(index) = index else {
        let allowed = if place.is_root() {
            "a string"
        } else {
            "a string or a list index"
        };
        return (!removes).then(|| {
            let path = place.path();
            format!("a key of {path} must be {allowed}, got {}", key.type_name())
        });
    };
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    if removes {
        return (1..len).contains(&index).then(|| {
            let path = place.path();
            format!(
                "{path}[{index}] is not the last item of the list, whose length is {len}; \
                 remove it with table.remove"
            )
        });
    }
    if index < 1 {
        let path = place.path();
        return Some(format!(
            "a list index of {path} must be 1 or more, got {index}"
        ));
    }
    if !marked && len == 0 && !data.is_empty() {
        let path = place.path();
        return Some(format!(
            "{path}[{index}] is a list item, and {path} holds named fields"
        ));
    }
    (index > len.saturating_add(1)).then(|| {
        let path = place.path();
        format!("{path}[{index}] is past the end of the list, whose length is {len}")
    })
}

/// `value` as guarded data: a guarded table for a list or an object, at
/// the place that `place` gives, and any other value as it is. `place`
/// is asked only for a table, so a value that is no table keeps no place.
fn guarded_json_value(
    lua: &Lua,
    value: &Json,
    place: &dyn Fn() -> Arc<Place>,
    guarded_data: &mlua::Table,
) -> mlua::Result<Value> {
    match value {
        Json::Array(values) => {
            let place = place();
            let data = lua.create_table_with_capacity(values.len(), 0)?;
            data.set_metatable(Some(lua.array_metatable()))?;
            for (index, value) in values.iter().enumerate() {
                data.raw_set(
                    index + 1,
                    guarded_json_value(
                        lua,
                        value,
                        &|| place.under(index_step(index + 1)),
                        guarded_data,
                    )?,
                )?;
            }
            guarded_table(lua, data, place, guarded_data).map(Value::Table)
        }
        Json::Object(values) => {
            let place = place();
            let data = lua.create_table_with_capacity(0, values.len())?;
            for (key, value) in values {
                data.raw_set(
                    key.as_str(),
                    guarded_json_value(
                        lua,
                        value,
                        &|| place.under(name_step(key.as_bytes())),
                        guarded_data,
                    )?,
                )?;
            }
            guarded_table(lua, data, place, guarded_data).map(Value::Table)
        }
        _ => lua.to_value(value),
    }
}

/// The guarded table over `data`, which sits at `place`: an empty proxy
/// that reads `data` and checks every write to it.
fn guarded_table(
    lua: &Lua,
    data: mlua::Table,
    place: Arc<Place>,
    guarded_data: &mlua::Table,
) -> mlua::Result<mlua::Table> {
    let proxy = lua.create_table()?;
    guarded_data.raw_set(proxy.clone(), data.clone())?;
    let metatable = lua.create_table()?;
    metatable.set("__index", data.clone())?;
    metatable.set("__len", view_len(lua, &data)?)?;

    let write_data = data;
    let nested_data = guarded_data.clone();
    let newindex = lua.create_function(
        move |lua, (_proxy, key, value): (Value, Value, Value)| -> mlua::Result<()> {
            // A whole float names the item its integer names.
            let key = whole_key(&key).map_or(key, Value::Integer);
            let removes = value.is_nil();
            if let Some(refusal) = write_refusal(lua, &place, &key, removes, &write_data) {
                return Err(author_refusal(refusal));
            }
            // A nil removes the key, where the conversion would store a
            // null under it.
            if removes {
                return write_data.raw_set(key, value);
            }
            // The conversion reads through each view inside the value,
            // refuses what JSON data cannot hold with the path of the
            // failing part, and spends the budget of the VM, so a value
            // that would outgrow the heap is refused before it converts.
            // It counts the levels of the value from the level of this
            // table, so the whole of `var` nests as deep as JSON text
            // that reads back, however many writes made it. It asks for
            // the path of the target only when it refuses the value.
            let target = || key_path(&place.path(), &key);
            let json = to_stored_json(lua, &target, place.level + 1, &value)?;
            let rebuilt =
                guarded_json_value(lua, &json, &|| place.under(key_step(&key)), &nested_data)?;
            write_data.raw_set(key, rebuilt)
        },
    )?;
    metatable.set("__newindex", newindex)?;
    metatable.set("__metatable", "var is guarded")?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

/// Copies a guarded value into plain tables for JSON conversion. A list's
/// data table carries mlua's array metatable, and its copy takes it too,
/// so an empty list converts to `[]` rather than `{}`.
fn materialize_guarded_value(
    lua: &Lua,
    value: Value,
    guarded_data: &mlua::Table,
) -> mlua::Result<Value> {
    let Value::Table(table) = value else {
        return Ok(value);
    };
    let source = match guarded_data.raw_get::<Value>(table.clone())? {
        Value::Table(data) => data,
        _ => table,
    };
    let plain = lua.create_table()?;
    let marker = lua.array_metatable();
    if source
        .metatable()
        .is_some_and(|metatable| metatable == marker)
    {
        plain.set_metatable(Some(marker))?;
    }
    // `pairs` order is unspecified; each iteration only assigns one entry
    // into the fresh table, so the copy's content is fixed.
    for pair in source.pairs::<Value, Value>() {
        let (key, value) = pair?;
        plain.raw_set(key, materialize_guarded_value(lua, value, guarded_data)?)?;
    }
    Ok(Value::Table(plain))
}

/// Builds the guarded `var` global: an empty proxy table over a hidden data
/// table.
///
/// The proxy is empty; reads go through `__index` against the data table, so
/// present values surface directly and absent keys read as Lua nil. `__len`
/// is the data table's length, and `pairs` and `next` walk the data table,
/// so a `var` table measures and iterates as the table it stands for.
/// `__newindex` converts the assigned value under the `json.encode` rules -
/// a function, userdata, or thread is rejected at the assigning line, as
/// is a table at any depth that mixes list items and named keys or has a
/// key that is not a string, and a view inside the value is stored as the
/// data it stands for - then rebuilds it as fresh guarded data before
/// writing through. It refuses, at the assigning line, a write that would
/// leave its table neither a list nor an object ([`write_refusal`]).
/// Assigning nil removes the key. Every nested table is itself an empty
/// proxy over hidden
/// data, so later incremental writes cross the same validation boundary instead
/// of mutating a stored table directly. `__metatable` is set so author code
/// cannot replace the guard. The root data table and the proxy live under
/// [`VAR_DATA_REGISTRY`] and [`VAR_PROXY_REGISTRY`] for [`var_to_json`]'s
/// read-back.
pub(crate) fn guarded_var(lua: &Lua, initial: Option<&Json>) -> Result<mlua::Table> {
    let initial = match initial {
        Some(Json::Object(values)) => Json::Object(values.clone()),
        Some(other) => {
            return Err(error::make_internal_error(format!(
                "var must seed from a JSON object, got {}",
                lua.to_value(other)
                    .map_err(error::make_lua_error)?
                    .type_name()
            )));
        }
        None => Json::Object(serde_json::Map::new()),
    };
    let guarded_data = lua.create_table().map_err(error::make_lua_error)?;
    let Value::Table(proxy) = guarded_json_value(lua, &initial, &Place::root, &guarded_data)
        .map_err(error::make_lua_error)?
    else {
        return Err(error::make_internal_error(
            "guarded var root was not a table",
        ));
    };
    let Value::Table(data) = guarded_data
        .raw_get::<Value>(proxy.clone())
        .map_err(error::make_lua_error)?
    else {
        return Err(error::make_internal_error(
            "guarded var data table was missing",
        ));
    };
    lua.set_named_registry_value(VAR_DATA_REGISTRY, data)
        .map_err(error::make_lua_error)?;
    lua.set_named_registry_value(VAR_PROXY_REGISTRY, proxy.clone())
        .map_err(error::make_lua_error)?;
    lua.set_named_registry_value(VAR_GUARDED_DATA_REGISTRY, guarded_data)
        .map_err(error::make_lua_error)?;
    Ok(proxy)
}

/// Returns the hidden `var` data table as a plain deep-copied table.
///
/// The guarded `var` proxy itself serializes empty; this reads what
/// [`var_to_json`] reads, materializing nested proxies, for the shims'
/// `var_snapshot` capture: every `call` and `tasks.spawn` request, each
/// `tasks.fanout` arm included, holds the caller's `var` as a clone.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the author reassigned the `var`
/// global or if a registry slot cannot be read (Engine values
/// were never injected) or a nested proxy cannot be copied, and
/// an `Internal` failure if the root data value is not a table.
pub(crate) fn var_snapshot_table(lua: &Lua) -> Result<mlua::Table> {
    let proxy: mlua::Table = lua
        .named_registry_value(VAR_PROXY_REGISTRY)
        .map_err(error::make_lua_error)?;
    // A raw read: a `var` that a block took out of the globals is
    // reassigned, whatever an `__index` of the block's on the globals
    // would answer for it.
    let current = lua
        .globals()
        .raw_get::<Value>("var")
        .map_err(error::make_lua_error)?;
    match current {
        Value::Table(ref table) if table.equals(&proxy).map_err(error::make_lua_error)? => {}
        _ => {
            return Err(make_lua_runtime_error(
                "the `var` global was reassigned; write `var.<field>` instead".to_owned(),
            ));
        }
    }
    let data: mlua::Table = lua
        .named_registry_value(VAR_DATA_REGISTRY)
        .map_err(error::make_lua_error)?;
    let guarded_data: mlua::Table = lua
        .named_registry_value(VAR_GUARDED_DATA_REGISTRY)
        .map_err(error::make_lua_error)?;
    match materialize_guarded_value(lua, Value::Table(data), &guarded_data)
        .map_err(error::make_lua_error)?
    {
        Value::Table(plain) => Ok(plain),
        // The root data value is always a table; `guarded_var` builds it.
        _ => Err(error::make_internal_error(
            "guarded var data table was not a table",
        )),
    }
}

/// Reads the hidden `var` data table back as JSON.
///
/// # Errors
/// Fails as [`var_snapshot_table`] does, and with a `LuaRuntime` failure if
/// the data cannot be represented as JSON.
pub(crate) fn var_to_json(lua: &Lua) -> Result<Json> {
    json_of(lua, Value::Table(var_snapshot_table(lua)?)).map_err(error::make_lua_error)
}

/// The place of a guarded table in `var`: the step to it from the table
/// that holds it, and the place of that table. A table keeps its own
/// step and shares the places above it, so the places of `var` take
/// memory with the count of its tables and not with their depth. A Pack
/// prelude's read-only view of such a table keeps its place the same way.
pub(crate) struct Place {
    /// The place of the table that holds this one, `None` for the root.
    above: Option<Arc<Place>>,
    /// The step to the table: `var` for the root, and `.name` or `[index]`.
    step: String,
    /// The level of the table in the JSON form of `var`: 1 for the root,
    /// and 2 for a table that the root holds.
    level: usize,
}

impl Place {
    /// The place of the root `var` table.
    pub(crate) fn root() -> Arc<Self> {
        Arc::new(Self {
            above: None,
            step: ROOT_STEP.to_owned(),
            level: 1,
        })
    }

    /// The place of the table that `step` leads to from this one.
    pub(crate) fn under(self: &Arc<Self>, step: String) -> Arc<Self> {
        Arc::new(Self {
            above: Some(Arc::clone(self)),
            step,
            level: self.level + 1,
        })
    }

    /// The path of the table as a refusal shows it, from `var` down.
    pub(crate) fn path(&self) -> String {
        #[cfg(test)]
        PATHS_BUILT.with(|built| built.set(built.get() + 1));
        let mut steps = Vec::with_capacity(self.level);
        let mut place = Some(self);
        while let Some(current) = place {
            steps.push(current.step.as_str());
            place = current.above.as_deref();
        }
        steps.into_iter().rev().collect::<String>()
    }

    fn is_root(&self) -> bool {
        self.above.is_none()
    }
}
