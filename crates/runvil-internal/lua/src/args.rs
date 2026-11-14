//! The `args` global: the chain's arguments, one JSON object, as a
//! read-only table.
//!
//! `args` is read-only in every section. It installs at Engine injection
//! on the `_G` guard ([`crate::globals`]): `args` is never a raw global,
//! so every read and every write of the name crosses the guard, which
//! serves the installed table and refuses the assignment before any
//! metatable author code set on `_G` sees the key. The table itself sits
//! behind read-only proxy tables whose `__newindex` rejects every write,
//! at any depth.

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::Value as Json;

use crate::error::{self, Result};
use crate::proxy::{key_text, read_only_proxy};

/// The refusal an assignment of the `args` global raises.
pub(crate) const ASSIGNMENT_REFUSAL: &str =
    "args is read-only: assign to `var` or a section global instead";

/// The label `getmetatable` returns for `args` and each table inside it.
const LABEL: &str = "args is read-only";

/// Installs `args`, the chain's arguments, on the `_G` guard. The value is
/// a JSON object, so `args` is a table in every section, an empty one
/// when the chain was given no fields. Reads work (absent fields read
/// nil), and every assignment - `args = ...` or a field write at any
/// depth - raises.
///
/// # Errors
/// Returns an `Internal` failure if `args` is not a JSON object, and a
/// `LuaRuntime` failure if the value cannot be bridged or the guard cannot
/// record it.
pub(crate) fn install(lua: &Lua, args: &Json) -> Result<()> {
    if !args.is_object() {
        return Err(error::make_internal_error(format!(
            "args must be a JSON object, got {}",
            lua.to_value(args)
                .map_err(error::make_lua_error)?
                .type_name()
        )));
    }
    let value = read_only_json_value(lua, args)?;
    crate::globals::set_args(lua, value)
}

/// Builds the read-only Lua form of a JSON value inside `args`: tables
/// become read-only proxies and scalars bridge directly, so a null reads
/// `json.null`. A list's data table carries mlua's array metatable, so an
/// empty list read through the proxy still encodes as `[]`, and a list
/// holding a null encodes whole.
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
            format!("args is read-only: cannot set field {field}")
        },
        LABEL,
    )
    .map_err(error::make_lua_error)
}
