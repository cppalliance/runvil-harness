//! The `_G` guard: the one metatable on a section VM's globals table.
//!
//! The guard owns the three Engine-owned globals, and no metatable author
//! code sets on `_G` ever sees any of their keys. `args`, the read-only
//! table of the chain's arguments ([`crate::args`]), is never a raw
//! global: reads return the table injection installed and writes raise
//! the read-only refusal. `prose`, each block's
//! lazily rendered text ([`crate::prose`]), is never a raw global either:
//! reads render through the Engine and writes raise the read-only refusal.
//! `messages`, the section's one conversation ([`crate::messages`]), is
//! never a raw global: reads return the list injection installed and
//! writes raise the reassignment refusal. Every other global read or write
//! goes to the author metatable's `__index` or `__newindex`, looked up
//! live on each access, and otherwise behaves as on a table with no
//! metatable.
//!
//! The guard carries `__metatable`, and the VM's `setmetatable` and
//! `getmetatable` globals are replacements that treat the globals table,
//! compared by raw identity, as the one special case:
//! `setmetatable(_G, mt)` records `mt` as the author's metatable behind the
//! guard, copying its other fields onto the guard, and `getmetatable(_G)`
//! returns the author's metatable (or its `__metatable` field), never the
//! guard. For every other value both behave as the base functions,
//! argument errors and `__metatable` protection included. With no debug
//! library, `rawset`, or code loading in the sandbox, author code has no
//! route to the guard or to a raw `args`, `prose`, or `messages` global.
//!
//! The Lua side lives in `__impl_globals.lua`; the Rust side is the slot
//! table the chunk reads, held in the registry.
//!
//! [`RESERVED_NAMES`] is the other half of `_G`'s contract: every name the
//! globals table holds once section setup ends, before any Pack
//! prelude installs, plus the Lua keywords. A Pack prelude global may not
//! take one.

use std::fmt;
use std::sync::LazyLock;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::error::{self, Error, Result};
use crate::meta_names::MetaNames;
use crate::program::LuaProgram;
use crate::weak::{ModeBudget, WEAK_METATABLE, index_weakness, is_weak};

/// The guard chunk's name.
const GLOBALS_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_globals.lua";

/// The guard source.
const GLOBALS_SOURCE: &str = include_str!("__impl_globals.lua");

/// The registry key of the slot table the guard chunk reads: `args`,
/// `prose`, `messages`, and `author`.
const STATE_REGISTRY: &str = "runvil.globals.state";

/// The guard program, with no debug info, so a failure Lua raises in the
/// guard names no variable of the chunk.
static GLOBALS_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal_stripped(GLOBALS_SOURCE, GLOBALS_CHUNK_NAME));

/// Installs the `_G` guard and the `setmetatable` and `getmetatable`
/// replacements on a fresh VM.
///
/// Must run before hardening removes `rawequal`, `rawget`, and `rawset`,
/// which the chunk captures, and before any other chunk captures the base
/// `setmetatable` or `getmetatable`.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the chunk cannot load or run, or the guard
/// and the replacements cannot be installed.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    let state = lua.create_table().map_err(error::make_lua_error)?;
    let refuse_args = refusal(lua, crate::args::ASSIGNMENT_REFUSAL)?;
    let refuse_prose = refusal(lua, crate::prose::ASSIGNMENT_REFUSAL)?;
    let refuse_messages = refusal(lua, crate::messages::ASSIGNMENT_REFUSAL)?;
    let argument_error = crate::stdlib::argument_error(lua).map_err(error::make_lua_error)?;
    let (guard, replace_metatable, read_metatable): (Table, Function, Function) = GLOBALS_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call((
            globals.clone(),
            state.clone(),
            refuse_args,
            refuse_prose,
            refuse_messages,
            argument_error,
        ))
        .map_err(error::make_lua_error)?;
    lua.set_named_registry_value(STATE_REGISTRY, state)
        .map_err(error::make_lua_error)?;
    globals
        .raw_set("setmetatable", replace_metatable)
        .map_err(error::make_lua_error)?;
    globals
        .raw_set("getmetatable", read_metatable)
        .map_err(error::make_lua_error)?;
    globals
        .set_metatable(Some(guard))
        .map_err(error::make_lua_error)
}

/// Whether `function` is the guard's `__index` or its `__newindex`.
///
/// # Errors
/// Returns the `mlua` error of a read of the guard that fails.
pub(crate) fn is_guard(lua: &Lua, function: &Function) -> mlua::Result<bool> {
    let Some(guard) = lua.globals().metatable() else {
        return Ok(false);
    };
    for event in ["__index", "__newindex"] {
        if guard.raw_get::<Value>(event)? == Value::Function(function.clone()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Puts `value`, the read-only table of the chain's arguments, in the
/// `args` slot, which every later read of the global returns. Every
/// assignment of the global raises the read-only refusal, with or without
/// a value in the slot.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the guard's slot table is missing or cannot
/// be written.
pub(crate) fn set_args(lua: &Lua, value: Value) -> Result<()> {
    state(lua)?
        .raw_set("args", value)
        .map_err(error::make_lua_error)
}

/// Puts `render` in the `prose` slot, nil leaving `prose` reading nil, and
/// returns the render it replaced, nil before the first install.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the guard's slot table is missing or cannot
/// be read or written.
pub(crate) fn replace_prose(lua: &Lua, render: Value) -> Result<Value> {
    let state = state(lua)?;
    let replaced = state.raw_get("prose").map_err(error::make_lua_error)?;
    state
        .raw_set("prose", render)
        .map_err(error::make_lua_error)?;
    Ok(replaced)
}

/// Puts `list`, the section's [`crate::MessageList`] userdata, in the
/// `messages` slot, which every later read of the global returns.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the guard's slot table is missing or cannot
/// be written.
pub(crate) fn set_messages(lua: &Lua, list: AnyUserData) -> Result<()> {
    state(lua)?
        .raw_set("messages", list)
        .map_err(error::make_lua_error)
}

/// The section's list, as the `messages` slot holds it.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the guard's slot table is missing or cannot
/// be read, and an `Internal` failure if no list is installed.
pub(crate) fn messages(lua: &Lua) -> Result<AnyUserData> {
    match state(lua)?
        .raw_get::<Value>("messages")
        .map_err(error::make_lua_error)?
    {
        Value::UserData(list) => Ok(list),
        _ => Err(error::make_internal_error(
            "the section's messages list is not installed",
        )),
    }
}

/// What a refusal says of `_G`, after the words that name it, when a
/// read of the global `name` looks into a table that it must not look
/// into, or `None` when it meets none. Past `budget`, the words are
/// those `budget` gives.
///
/// The guard answers a global that `_G` lacks from the `__index` of the
/// metatable the author gave `_G`: a function is called, and any other
/// value is read by `name`, as Lua reads a field. So the read is refused
/// for a weak table on that way, as [`index_weakness`] tells one, and
/// for an author's metatable that is a weak table itself, whose
/// `__index` is there or not as the collector leaves it. `args`,
/// `prose`, and `messages` are the guard's own, and a global that `_G`
/// holds is read from `_G`, which is never weak.
///
/// The caller makes the read next, with no code of the author between
/// the two. One that reads many globals in one Rust call counts them
/// against one `budget`: every such read goes the same way.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the guard's slot table is missing,
/// or a table or a metatable cannot be read.
pub(crate) fn read_weakness(
    lua: &Lua,
    name: &str,
    budget: &mut ModeBudget<'_>,
) -> Result<Option<String>> {
    if GUARDED.contains(&name) {
        return Ok(None);
    }
    let key = lua
        .create_string(name)
        .map(Value::String)
        .map_err(error::make_lua_error)?;
    let held = lua
        .globals()
        .raw_get::<Value>(&key)
        .map_err(error::make_lua_error)?;
    if !held.is_nil() {
        return Ok(None);
    }
    let Value::Table(author) = state(lua)?
        .raw_get::<Value>("author")
        .map_err(error::make_lua_error)?
    else {
        return Ok(None);
    };
    if is_weak(&author).map_err(error::make_lua_error)? {
        return Ok(Some(WEAK_METATABLE.to_owned()));
    }
    let index = author
        .raw_get::<Value>("__index")
        .map_err(error::make_lua_error)?;
    let names = MetaNames::default();
    index_weakness(lua, &names, &index, &key, budget).map_err(error::make_lua_error)
}

/// The globals the guard answers itself, from its slot table.
const GUARDED: [&str; 3] = ["args", "prose", "messages"];

/// The guard's slot table.
fn state(lua: &Lua) -> Result<Table> {
    lua.named_registry_value(STATE_REGISTRY)
        .map_err(error::make_lua_error)
}

/// An Engine function that raises `message` as a runtime error.
fn refusal(lua: &Lua, message: &'static str) -> Result<Function> {
    lua.create_function(move |_, ()| -> mlua::Result<()> { Err(mlua::Error::runtime(message)) })
        .map_err(error::make_lua_error)
}

/// Why a name is reserved in a section VM's global namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reserved {
    /// A global the Engine installs: on every section VM, or only on some
    /// (`item` in a spawned chain).
    EngineGlobal,
    /// A Lua standard-library global the sandbox keeps.
    LuaGlobal,
    /// A Lua 5.5 keyword.
    LuaKeyword,
}

impl fmt::Display for Reserved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Reserved::EngineGlobal => "an Engine global",
            Reserved::LuaGlobal => "a Lua standard-library global",
            Reserved::LuaKeyword => "a Lua keyword",
        })
    }
}

/// Every name reserved in a section VM's global namespace, sorted within
/// each kind: the globals a section VM holds once section setup
/// ends, before any Pack prelude installs, and the Lua 5.5 keywords.
///
/// A global section setup installs must be listed here: the Engine's
/// section setup tests compare this list against a set-up VM's globals in
/// both directions.
pub const RESERVED_NAMES: [(&str, Reserved); 62] = [
    ("args", Reserved::EngineGlobal),
    ("call", Reserved::EngineGlobal),
    ("compactors", Reserved::EngineGlobal),
    ("execute", Reserved::EngineGlobal),
    ("item", Reserved::EngineGlobal),
    ("json", Reserved::EngineGlobal),
    ("jump", Reserved::EngineGlobal),
    ("log", Reserved::EngineGlobal),
    ("markdown", Reserved::EngineGlobal),
    ("messages", Reserved::EngineGlobal),
    ("models", Reserved::EngineGlobal),
    ("prompt", Reserved::EngineGlobal),
    ("prose", Reserved::EngineGlobal),
    ("require", Reserved::EngineGlobal),
    ("store", Reserved::EngineGlobal),
    ("sys", Reserved::EngineGlobal),
    ("tasks", Reserved::EngineGlobal),
    ("tools", Reserved::EngineGlobal),
    ("untrusted", Reserved::EngineGlobal),
    ("var", Reserved::EngineGlobal),
    ("_G", Reserved::LuaGlobal),
    ("_VERSION", Reserved::LuaGlobal),
    ("assert", Reserved::LuaGlobal),
    ("error", Reserved::LuaGlobal),
    ("getmetatable", Reserved::LuaGlobal),
    ("ipairs", Reserved::LuaGlobal),
    ("math", Reserved::LuaGlobal),
    ("next", Reserved::LuaGlobal),
    ("pairs", Reserved::LuaGlobal),
    ("pcall", Reserved::LuaGlobal),
    ("select", Reserved::LuaGlobal),
    ("setmetatable", Reserved::LuaGlobal),
    ("string", Reserved::LuaGlobal),
    ("table", Reserved::LuaGlobal),
    ("tonumber", Reserved::LuaGlobal),
    ("tostring", Reserved::LuaGlobal),
    ("type", Reserved::LuaGlobal),
    ("utf8", Reserved::LuaGlobal),
    ("xpcall", Reserved::LuaGlobal),
    ("and", Reserved::LuaKeyword),
    ("break", Reserved::LuaKeyword),
    ("do", Reserved::LuaKeyword),
    ("else", Reserved::LuaKeyword),
    ("elseif", Reserved::LuaKeyword),
    ("end", Reserved::LuaKeyword),
    ("false", Reserved::LuaKeyword),
    ("for", Reserved::LuaKeyword),
    ("function", Reserved::LuaKeyword),
    // The vendored Lua 5.5 builds with `LUA_COMPAT_GLOBAL`, so its lexer
    // reads `global` as a contextual keyword rather than a reserved word;
    // the language reserves it all the same.
    ("global", Reserved::LuaKeyword),
    ("goto", Reserved::LuaKeyword),
    ("if", Reserved::LuaKeyword),
    ("in", Reserved::LuaKeyword),
    ("local", Reserved::LuaKeyword),
    ("nil", Reserved::LuaKeyword),
    ("not", Reserved::LuaKeyword),
    ("or", Reserved::LuaKeyword),
    ("repeat", Reserved::LuaKeyword),
    ("return", Reserved::LuaKeyword),
    ("then", Reserved::LuaKeyword),
    ("true", Reserved::LuaKeyword),
    ("until", Reserved::LuaKeyword),
    ("while", Reserved::LuaKeyword),
];

/// Returns why `name` is reserved in a section VM's global namespace, or
/// `None` when a Pack prelude global may take it. The match is
/// case-sensitive, as Lua names are.
#[must_use]
pub fn reserved_name(name: &str) -> Option<Reserved> {
    RESERVED_NAMES
        .iter()
        .find(|(reserved, _)| *reserved == name)
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
#[path = "globals-tests.rs"]
mod tests;
