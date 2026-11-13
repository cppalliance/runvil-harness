//! The `argv` global: the parsed form of the run's args string.
//!
//! `argv` is read-only in every section. It installs at Engine injection
//! on the `_G` guard ([`crate::globals`]): `argv` is never a raw global,
//! so every read and every write of the name crosses the guard, which
//! serves the installed value and refuses the assignment before any
//! metatable author code set on `_G` sees the key. The table value itself
//! sits behind read-only proxy tables whose `__newindex` rejects every
//! write, at any depth.

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::Value as Json;

use crate::error::{self, Result};
use crate::proxy::{key_text, read_only_proxy};

/// The refusal an assignment of the `argv` global raises.
pub(crate) const ASSIGNMENT_REFUSAL: &str =
    "argv is read-only: assign to `var` or a section global instead";

/// The label `getmetatable` returns for `argv` and each table inside it.
const LABEL: &str = "argv is read-only";

/// Installs `argv` on the `_G` guard. `None` (malformed args, or JSON
/// null) installs nil, so `if argv then` is the idiomatic malformed check.
/// Reads work (absent fields read nil), and every assignment - `argv = ...`
/// or a field write at any depth - raises.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the value cannot be bridged or the guard
/// cannot record it.
pub(crate) fn install(lua: &Lua, argv: Option<&Json>) -> Result<()> {
    let value = match argv {
        None | Some(Json::Null) => Value::Nil,
        Some(json) => read_only_json_value(lua, json)?,
    };
    crate::globals::set_argv(lua, value)
}

/// Builds the read-only Lua form of an argv JSON value: tables become
/// read-only proxies and scalars bridge directly. A nested null reads
/// `json.null`; only a top-level absent or null argv reads nil, which
/// [`install`] handles before this runs. A list's data table carries
/// mlua's array metatable, so an empty list read through the proxy still
/// encodes as `[]`, and a list holding a null encodes whole.
fn read_only_json_value(lua: &Lua, value: &Json) -> Result<Value> {
    match value {
        Json::Array(values) => {
            let data = lua
                .create_table_with_capacity(values.len(), 0)
                .map_err(error::make_lua_error)?;
            data.set_metatable(Some(lua.array_metatable()))
                .map_err(error::make_lua_error)?;
            for (index, value) in values.iter().enumerate() {
                data.raw_set(index + 1, read_only_json_value(lua, value)?)
                    .map_err(error::make_lua_error)?;
            }
            read_only_table(lua, data).map(Value::Table)
        }
        Json::Object(values) => {
            let data = lua
                .create_table_with_capacity(0, values.len())
                .map_err(error::make_lua_error)?;
            for (key, value) in values {
                data.raw_set(key.as_str(), read_only_json_value(lua, value)?)
                    .map_err(error::make_lua_error)?;
            }
            read_only_table(lua, data).map(Value::Table)
        }
        scalar => lua.to_value(scalar).map_err(error::make_lua_error),
    }
}

/// Wraps a plain data table in a read-only proxy: reads pass through
/// `__index` to the data (whose nested tables are already read-only
/// proxies, and whose absent keys read nil), and every write raises the
/// field refusal.
fn read_only_table(lua: &Lua, data: mlua::Table) -> Result<mlua::Table> {
    read_only_proxy(
        lua,
        Value::Table(data),
        |key| {
            let field = match key {
                Value::String(_) => format!("'{}'", key_text(key)),
                other => key_text(other),
            };
            format!("argv is read-only: cannot set field {field}")
        },
        LABEL,
    )
    .map_err(error::make_lua_error)
}
