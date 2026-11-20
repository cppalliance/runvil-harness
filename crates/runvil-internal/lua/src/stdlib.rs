//! The standard-library functions a section VM replaces.
//!
//! A run with the same seed, start time, and answers must produce the same
//! result. Four stock functions read something no run input fixes:
//! `math.randomseed` with no argument and `table.sort` read the clock and a
//! memory address, and `tostring` and `string.format` print the address of a
//! table, function, thread, or userdata. [`install`] replaces each with a
//! function whose result depends only on its arguments and on the VM's
//! generator, which starts from a fixed seed until
//! [`SectionVm::seed_random`](crate::SectionVm::seed_random) gives it the
//! run's. The same chunk holds the `table.concat` wrapper, which lets a value
//! with a `__tostring` metamethod join like a string.
//!
//! The Lua side lives in `__impl_stdlib.lua`; the Rust side is
//! [`unaddressed`], the one question the chunk cannot answer itself, because
//! Lua code cannot read the metatable of a userdata, or tell a Rust
//! callback's failure from any other userdata.

use std::sync::LazyLock;

use mlua::{Lua, Value};

use crate::error::{self, Error, Result};
use crate::error_value::failure_message;
use crate::program::LuaProgram;

/// Replaces `math.randomseed`, `tostring`, `string.format`, `table.concat`,
/// and `table.sort` on a fresh VM, and seeds its generator with a fixed
/// value.
///
/// Must run after the `_G` guard is installed, whose `getmetatable` the
/// chunk captures, and before any other chunk captures a function this one
/// replaces.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the chunk cannot load or run.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let unaddressed = lua
        .create_function(|lua, value: Value| {
            let (name, rendering) = unaddressed(lua, &value)?;
            let name = name.map(|name| lua.create_string(name)).transpose()?;
            Ok((name, rendering))
        })
        .map_err(error::make_lua_error)?;
    STDLIB_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call::<()>(unaddressed)
        .map_err(error::make_lua_error)
}

/// The text a section VM prints `value` as, when stock Lua prints it with
/// an address. For a table, function, thread, or userdata that is the text
/// Lua prints in front of the address: the `__name` of its metatable when
/// that is a string, and its type name otherwise. For a Rust callback's
/// failure it is [`failure_message`]. `None` for a value Lua prints without
/// an address: a nil, boolean, number, or string, and a table or userdata
/// whose metatable has `__tostring`. The second value is that `__tostring`,
/// and nil where there is none.
///
/// Stock Lua prints a table as `{name}: {address}`. The address is where
/// the allocator put the value, which no run input fixes, so a section VM
/// prints the name alone. `mlua` prints a callback's failure as its own
/// text for it, which holds such an address when the failure is a table
/// that a metamethod raised.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn unaddressed(lua: &Lua, value: &Value) -> mlua::Result<(Option<Vec<u8>>, Value)> {
    let (rendering, name) = match value {
        Value::Error(failure) => {
            return Ok((Some(failure_message(lua, failure).into_bytes()), Value::Nil));
        }
        Value::Table(table) => match table.metatable() {
            Some(metatable) => (
                metatable.raw_get::<Value>("__tostring")?,
                metatable.raw_get::<Value>("__name")?,
            ),
            None => (Value::Nil, Value::Nil),
        },
        // mlua exposes no metatable for a userdata it did not create or one
        // already destructed, so such a value prints as its type name.
        Value::UserData(userdata) => match userdata.metatable() {
            Ok(metatable) => (
                metatable.get::<Value>("__tostring")?,
                metatable.get::<Value>("__name")?,
            ),
            Err(_) => (Value::Nil, Value::Nil),
        },
        Value::Function(_) | Value::Thread(_) | Value::LightUserData(_) => (Value::Nil, Value::Nil),
        _ => return Ok((None, Value::Nil)),
    };
    if rendering != Value::Nil {
        return Ok((None, rendering));
    }
    let name = match (name, value) {
        // Stock Lua prints the name as a C string, which ends at its first
        // zero byte.
        (Value::String(name), _) => {
            let name = name.as_bytes();
            let end = name.iter().position(|byte| *byte == 0);
            name[..end.unwrap_or(name.len())].to_vec()
        }
        // Lua has one type name for a light and a full userdata.
        (_, Value::LightUserData(_)) => b"userdata".to_vec(),
        (_, value) => value.type_name().as_bytes().to_vec(),
    };
    Ok((Some(name), Value::Nil))
}

/// The replacement chunk's name.
const STDLIB_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_stdlib.lua";

/// The replacement source.
const STDLIB_SOURCE: &str = include_str!("__impl_stdlib.lua");

/// The replacement program.
static STDLIB_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(STDLIB_SOURCE, STDLIB_CHUNK_NAME));

#[cfg(test)]
mod tests {
    use super::{STDLIB_CHUNK_NAME, STDLIB_SOURCE};
    use crate::tests::assert_chunk_name_resolves;

    #[test]
    fn the_chunk_name_resolves_to_the_embedded_file() {
        assert_chunk_name_resolves("STDLIB_CHUNK_NAME", STDLIB_CHUNK_NAME, STDLIB_SOURCE);
    }
}
