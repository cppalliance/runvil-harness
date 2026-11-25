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
//! The Lua side is two chunks. `__impl_stdlib.lua` holds the replacements
//! and compiles with no debug info, so a replacement has no line and no
//! name for a variable, as the C function it replaces has none.
//! `error(text, 2)` in a metamethod the replacement calls then puts no
//! position in front of `text`, and `error(text, 3)` puts the position of
//! the author's call there, as under the stock function. A failure Lua
//! raises in the replacement itself, such as a read of a value that takes
//! none, names no variable of the replacement and starts with `?:?:`,
//! which [`engine_position_len`](crate::chunk_line::engine_position_len)
//! finds wherever the Engine reads a failure. `__impl_stdlib_stock.lua`
//! holds the calls of the stock functions and keeps its debug info, which
//! marks a failure Lua raises there.
//!
//! The Rust side answers what the replacements cannot ask in Lua:
//! [`unaddressed`], [`length_refusal`], [`order_refusal`],
//! [`type_refusal`], and [`is_bare`] read a metamethod as Lua finds it.
//! Lua code cannot read the metatable of a userdata, or one that
//! `__metatable` covers, or tell a Rust callback's failure from any other
//! userdata. [`argument_error`] reads how the author's call of a
//! replacement is written, which names the replacement in an argument
//! error, as stock Lua names the stock function.
//!
//! A capture makes no protected call and no Lua string while it takes a
//! value and refuses nothing: it reads a metamethod by a name that
//! [`Events`] made once. `mlua` makes a string under a protected call,
//! and Lua counts a protected call among its nested C calls. A `__len`
//! that calls the replacement again would otherwise reach Lua's limit of
//! those inside the capture, and the failure would be a Rust callback's,
//! an error table, where Lua's own is the string `C stack overflow`.
//! Three reads still make a string: of a userdata, whose metatable
//! `mlua` reads by a Rust string alone, of a `__name` with a zero byte in
//! it, and of a Rust callback's failure.
//!
//! A replacement asks before it takes a length or compares two values
//! whether Lua can call the `__len` or the `__lt`. In a Lua function Lua
//! says which metamethod it could not call, as in `attempt to call a
//! number value (metamethod 'len')`, and in a C function it does not.
//! Every other refusal of Lua's has the stock function's words in a
//! replacement. A protected call around the operation would keep those
//! words out too, and would also catch a failure of the author's
//! metamethod: the handler of the author's `xpcall` would then run after
//! the metamethod's frame is gone and its variables are closed, and the
//! Engine would read the failing line from the replacement's raise.

use std::sync::LazyLock;

use mlua::{Function, Lua, LuaString, Table, Value, Variadic};

use crate::error::{self, Error, Result};
use crate::error_value::failure_message;
use crate::program::LuaProgram;

/// Replaces `math.randomseed`, `tostring`, `string.format`, `table.concat`,
/// and `table.sort` on a fresh VM, and seeds its generator with a fixed
/// value.
///
/// Must run after the `_G` guard is installed, whose `setmetatable` the
/// chunk captures, while the `rawget` global is still there, which the
/// chunk captures too, and before any other chunk captures a function
/// this one replaces. It must also run before the run's memory limit is
/// set, which [`Events`] gives the reason for.
///
/// The `mistyped` function made here is the one of the VM: [`mistyped`]
/// gives it to a chunk installed later, so nothing makes an [`Events`]
/// under the run's limit. The metatable of strings is kept for
/// [`strings_metatable`] for the same reason.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the chunk cannot load or run.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let events = Events::new(lua).map_err(error::make_lua_error)?;
    let strings = events.strings.clone();
    lua.set_named_registry_value(STRINGS_REGISTRY, strings.clone())
        .map_err(error::make_lua_error)?;
    let read = events.clone();
    let unaddressed = lua
        .create_function(move |lua, value: Value| {
            Ok(match unaddressed(lua, &read, &value)? {
                Shown::AsName(name) => (Some(name), Value::Nil),
                Shown::By(rendering) => (None, rendering),
                Shown::AsType => (None, Value::Nil),
            })
        })
        .map_err(error::make_lua_error)?;
    let read = events.clone();
    let unmeasured = lua
        .create_function(move |lua, list: Value| text(lua, length_refusal(&read, &list)?))
        .map_err(error::make_lua_error)?;
    let read = events.clone();
    let unordered = lua
        .create_function(move |lua, (left, right): (Value, Value)| {
            text(lua, order_refusal(&read, &left, &right)?)
        })
        .map_err(error::make_lua_error)?;
    let mistyped = lua
        .create_function(
            move |lua, (value, expected, wanted): (Value, LuaString, Variadic<LuaString>)| {
                text(
                    lua,
                    type_refusal(&events, &value, &expected.as_bytes(), &wanted)?,
                )
            },
        )
        .map_err(error::make_lua_error)?;
    lua.set_named_registry_value(MISTYPED_REGISTRY, &mistyped)
        .map_err(error::make_lua_error)?;
    let bare = lua
        .create_function(|_, list: Value| Ok(is_bare(&list)))
        .map_err(error::make_lua_error)?;
    let argument_error = argument_error(lua).map_err(error::make_lua_error)?;
    let stock = STOCK_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call::<Table>(&argument_error)
        .map_err(error::make_lua_error)?;
    let captures = (
        unaddressed,
        unmeasured,
        unordered,
        mistyped,
        bare,
        stock,
        argument_error,
        strings,
    );
    STDLIB_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call::<()>(captures)
        .map_err(error::make_lua_error)
}

/// The Lua function `mistyped(value, expected, ...)`, which gives
/// [`type_refusal`] of a value for the type `expected` and the events
/// after it, nil where the value is taken. The Engine's `xpcall` names a
/// handler that it refuses by it too.
///
/// [`install`] makes the function, and this is that one.
///
/// # Errors
/// Returns the `mlua` error of a read of the registry that fails, or of
/// a VM that [`install`] did not run on.
pub(crate) fn mistyped(lua: &Lua) -> mlua::Result<Function> {
    lua.named_registry_value(MISTYPED_REGISTRY)
}

/// The one metatable all strings share, as [`install`] found it, or `None`
/// on a VM that [`install`] did not run on or whose strings have none.
/// [`Events`] tells why nothing takes it from Lua a second time.
///
/// # Errors
/// Returns the `mlua` error of a read of the registry that fails.
pub(crate) fn strings_metatable(lua: &Lua) -> mlua::Result<Option<Table>> {
    lua.named_registry_value(STRINGS_REGISTRY)
}

/// The Lua function `argument_error(level, function, argument, reason)`,
/// which gives the text stock Lua refuses an argument with
/// (`luaL_argerror` in `lauxlib.c`), for a function the Engine writes in
/// Lua in place of a stock one. That function is the one at `level`,
/// where 1 is the function that calls this one, `function` is its name in
/// the library, as in `table.sort`, `argument` is the number of the
/// argument, and `reason` is what stock Lua says of it.
///
/// Stock Lua names the function as the call is written, and so does this:
/// by the variable or the field of the call, which Lua reads from the
/// code of the caller. For a call written `value:name(...)` the count
/// leaves the value out, and a refusal of the value itself reads `calling
/// 'name' on bad self`. A caller that is no Lua code, such as `pcall`,
/// writes no name, and the name is then `function`. A replacement that
/// runs a metamethod is such a caller too, as the C function it stands
/// in for is: Lua would name the event, as in `index`.
///
/// A tail call leaves nothing of the call to read, so the name is then
/// the last part of `function`, which is what `return table.sort(list)`
/// has in stock Lua.
///
/// # Errors
/// Returns the `mlua` error of a function that cannot be created.
pub(crate) fn argument_error(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(
        |lua, (level, function, argument, reason): (usize, LuaString, i64, LuaString)| {
            let function = function.as_bytes();
            let called = lua.inspect_stack(level, |frame| {
                let tail_call = frame.is_tail_call();
                let names = frame.names();
                let name = names.name.map(|name| name.into_owned().into_bytes());
                (name, names.name_what == Some("method"), tail_call)
            });
            let (name, method) = match called {
                Some((Some(_), ..)) if stands_for_c(lua, level + 1)? => (function.to_vec(), false),
                Some((Some(name), method, _)) => (name, method),
                Some((None, _, true)) => {
                    let field = function.rsplit(|byte| *byte == b'.').next();
                    (field.unwrap_or(&function).to_vec(), false)
                }
                _ => (function.to_vec(), false),
            };
            let argument = argument - i64::from(method);
            let reason = reason.as_bytes();
            lua.create_string(if method && argument == 0 {
                [b"calling '", &*name, b"' on bad self (", &*reason, b")"].concat()
            } else {
                let argument = argument.to_string();
                let front = [b"bad argument #", argument.as_bytes(), b" to '"].concat();
                [&*front, &*name, b"' (", &*reason, b")"].concat()
            })
        },
    )
}

/// The registry key of the `mistyped` function that [`install`] makes.
const MISTYPED_REGISTRY: &str = "runvil.stdlib.mistyped";

/// The registry key of the metatable of strings that [`install`] keeps.
const STRINGS_REGISTRY: &str = "runvil.stdlib.strings";

/// How many `__call` metamethods Lua follows from a value that is no
/// function before it refuses the call, `MAX_CCMT` in Lua's `lstate.h`.
const CALL_CHAIN: usize = 15;

/// The source Lua reports for a function of a chunk with no debug info.
const NO_SOURCE: &str = "=?";

/// Whether the function at `level` stands in for a C function of stock
/// Lua: it is of an Engine chunk with no debug info and is no part of
/// the guard of the globals. The guard stands for no stock function. It
/// runs a handler of the author's for the read or the write that the
/// author's code makes, which Lua names by its event.
///
/// # Errors
/// Returns the `mlua` error of a read of the guard that fails.
fn stands_for_c(lua: &Lua, level: usize) -> mlua::Result<bool> {
    let engine = lua.inspect_stack(level, |frame| {
        (frame.source().source.as_deref() == Some(NO_SOURCE)).then(|| frame.function())
    });
    match engine.flatten() {
        Some(function) => Ok(!crate::globals::is_guard(lua, &function)?),
        None => Ok(false),
    }
}

/// What the captures read a metamethod with: the name of each event they
/// ask for, and the one metatable all strings share. [`install`] makes
/// both once, on a fresh VM, far below the memory limit that a VM has
/// from its start.
///
/// A read by a name that is a Lua string already makes no string, so it
/// needs no memory and no protected call. No Lua code of a section VM
/// can put another metatable in the place of the strings' one, and
/// `Lua::type_metatable` pushes an empty string with no protected call
/// around the push. A memory error of that push ends the process where
/// no Lua code runs, and inside a callback it would leave through the
/// Rust frames of the callback. So nothing takes that metatable again
/// once the run's limit is set.
#[derive(Clone)]
struct Events {
    tostring: LuaString,
    len: LuaString,
    lt: LuaString,
    call: LuaString,
    name: LuaString,
    strings: Option<Table>,
}

impl Events {
    /// # Errors
    /// Returns the `mlua` error of a string that cannot be created.
    fn new(lua: &Lua) -> mlua::Result<Self> {
        Ok(Self {
            tostring: lua.create_string("__tostring")?,
            len: lua.create_string("__len")?,
            lt: lua.create_string("__lt")?,
            call: lua.create_string("__call")?,
            name: lua.create_string("__name")?,
            strings: lua.type_metatable::<LuaString>(),
        })
    }
}

/// How a section VM prints a value that stock Lua prints with an address:
/// a table, function, thread, or userdata.
enum Shown {
    /// As this text: the `__name` of a table or a userdata with no
    /// `__tostring`, which Lua prints in front of the address, and
    /// [`failure_message`] of a Rust callback's failure.
    AsName(LuaString),
    /// By this `__tostring` metamethod.
    By(Value),
    /// As the name `type` gives, with no `__tostring` and no `__name`.
    AsType,
}

/// How a section VM prints `value`.
///
/// Stock Lua prints a table as `{name}: {address}`. The address is where
/// the allocator put the value, which no run input fixes, so a section VM
/// prints the name alone. `mlua` prints a callback's failure as its own
/// text for it, which holds such an address when the failure is a table
/// that a metamethod raised.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails, or of a
/// string that cannot be created.
fn unaddressed(lua: &Lua, events: &Events, value: &Value) -> mlua::Result<Shown> {
    if let Value::Error(failure) = value {
        return Ok(Shown::AsName(
            lua.create_string(failure_message(lua, failure))?,
        ));
    }
    let rendering = metamethod(events, value, &events.tostring)?;
    if rendering != Value::Nil {
        return Ok(Shown::By(rendering));
    }
    let Value::String(name) = metamethod(events, value, &events.name)? else {
        return Ok(Shown::AsType);
    };
    // Stock Lua prints the name as a C string, which ends at its first
    // zero byte. A name with none is printed as the string it is.
    let cut = c_string(&name);
    Ok(Shown::AsName(if cut.len() == name.as_bytes().len() {
        name
    } else {
        lua.create_string(cut)?
    }))
}

/// The text Lua refuses `#list` with for a `__len` it cannot call, and
/// `None` otherwise: `list` is a string, whose length Lua takes with no
/// look at `__len`, or it has no `__len`, or one Lua can call. The caller
/// passes a table, or a value that holds a `__len`.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn length_refusal(events: &Events, list: &Value) -> mlua::Result<Option<Vec<u8>>> {
    let handler = metamethod(events, list, &events.len)?;
    if handler == Value::Nil || list.is_string() {
        return Ok(None);
    }
    call_refusal(events, &handler)
}

/// The text Lua refuses `left < right` with for a `__lt` it cannot call,
/// and `None` otherwise: the first `__lt` of the two, the left one's
/// first, is one Lua can call, or neither has one, which Lua refuses in
/// the stock function's words. The caller passes two values that are not
/// both numbers and not both strings, which Lua compares with no
/// metamethod.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn order_refusal(events: &Events, left: &Value, right: &Value) -> mlua::Result<Option<Vec<u8>>> {
    let mut handler = metamethod(events, left, &events.lt)?;
    if handler == Value::Nil {
        handler = metamethod(events, right, &events.lt)?;
    }
    if handler == Value::Nil {
        return Ok(None);
    }
    call_refusal(events, &handler)
}

/// The name stock Lua gives the type of an argument it refuses, `None`
/// when it takes `value` where the type `expected` belongs: a value of
/// that type, or a value of another type whose metatable holds every one
/// of `wanted`, which is how a stock function of `table` takes a value
/// that is no table as its list. With no event, only a value of the type
/// is taken.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn type_refusal(
    events: &Events,
    value: &Value,
    expected: &[u8],
    wanted: &[LuaString],
) -> mlua::Result<Option<Vec<u8>>> {
    if base_type_name(value) == expected {
        return Ok(None);
    }
    let mut taken = !wanted.is_empty();
    for event in wanted {
        taken = taken && metamethod(events, value, event)? != Value::Nil;
    }
    if taken {
        return Ok(None);
    }
    argument_type_name(events, value).map(Some)
}

/// Whether `list` is a table with no metatable. Lua takes the length of
/// such a table and reads and writes its items with no metamethod, so
/// none of the three runs the author's code, and none is refused.
fn is_bare(list: &Value) -> bool {
    match list {
        Value::Table(table) => table.metatable().is_none(),
        _ => false,
    }
}

/// The text Lua refuses a call of `callee` with, `None` when it calls it:
/// `callee` is a function, or [`CALL_CHAIN`] or fewer `__call` metamethods
/// lead from it to one.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn call_refusal(events: &Events, callee: &Value) -> mlua::Result<Option<Vec<u8>>> {
    let mut callee = callee.clone();
    for followed in 0..=CALL_CHAIN {
        if callee.is_function() {
            return Ok(None);
        }
        let next = metamethod(events, &callee, &events.call)?;
        if next == Value::Nil {
            let name = type_name(events, &callee)?;
            return Ok(Some(
                [b"attempt to call a ", name.as_slice(), b" value"].concat(),
            ));
        }
        if followed == CALL_CHAIN {
            break;
        }
        callee = next;
    }
    Ok(Some(b"'__call' chain too long".to_vec()))
}

/// The metamethod `event` of `value` as Lua finds it, read raw from the
/// metatable of a table or a userdata and from the one metatable all
/// strings share. Nil for a value of any other type, which has no
/// metatable in a section VM.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn metamethod(events: &Events, value: &Value, event: &LuaString) -> mlua::Result<Value> {
    match value {
        Value::Table(table) => match table.metatable() {
            Some(metatable) => metatable.raw_get(event),
            None => Ok(Value::Nil),
        },
        // mlua exposes no metatable for a userdata it did not create or one
        // already destructed, so such a value has no metamethod here. It
        // reads the metatable of a userdata by a Rust string alone, which
        // it pushes under a protected call.
        Value::UserData(userdata) => match userdata.metatable() {
            Ok(metatable) => metatable.get(event.to_string_lossy()),
            Err(_) => Ok(Value::Nil),
        },
        Value::String(_) => match &events.strings {
            Some(strings) => strings.raw_get(event),
            None => Ok(Value::Nil),
        },
        _ => Ok(Value::Nil),
    }
}

/// The name of the type of `value` in a failure Lua raises about an
/// operation on it: the `__name` of the metatable of a table or a userdata
/// when that is a string, and the name `type` gives otherwise.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn type_name(events: &Events, value: &Value) -> mlua::Result<Vec<u8>> {
    let name = match value {
        Value::Table(_) | Value::UserData(_) => metamethod(events, value, &events.name)?,
        _ => Value::Nil,
    };
    Ok(match name {
        Value::String(name) => c_string(&name),
        _ => base_type_name(value).to_vec(),
    })
}

/// The name of the type of `value` where a stock function refuses it as an
/// argument: the `__name` of its metatable when that is a string, `light
/// userdata` for one, and the name `type` gives otherwise.
///
/// # Errors
/// Returns the `mlua` error of a metatable read that fails.
fn argument_type_name(events: &Events, value: &Value) -> mlua::Result<Vec<u8>> {
    Ok(match (metamethod(events, value, &events.name)?, value) {
        (Value::String(name), _) => c_string(&name),
        (_, Value::LightUserData(_)) => b"light userdata".to_vec(),
        (_, value) => base_type_name(value).to_vec(),
    })
}

/// The name `type` gives for `value`. Lua has one name for a light and a
/// full userdata, and a Rust callback's failure is a userdata. It has one
/// for an integer and a float.
fn base_type_name(value: &Value) -> &'static [u8] {
    match value {
        Value::LightUserData(_) | Value::Error(_) => b"userdata",
        Value::Integer(_) | Value::Number(_) => b"number",
        value => value.type_name().as_bytes(),
    }
}

/// `name` as stock Lua prints it, as a C string, which ends at its first
/// zero byte.
fn c_string(name: &LuaString) -> Vec<u8> {
    let name = name.as_bytes();
    let end = name.iter().position(|byte| *byte == 0);
    name[..end.unwrap_or(name.len())].to_vec()
}

/// `text` as a Lua string, nil for `None`.
fn text(lua: &Lua, text: Option<Vec<u8>>) -> mlua::Result<Option<LuaString>> {
    text.map(|text| lua.create_string(text)).transpose()
}

/// The replacement chunk's name, which only a failure to compile it
/// shows.
const STDLIB_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_stdlib.lua";

/// The replacement source.
const STDLIB_SOURCE: &str = include_str!("__impl_stdlib.lua");

/// The replacement program, with no debug info.
static STDLIB_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal_stripped(STDLIB_SOURCE, STDLIB_CHUNK_NAME));

/// The name of the chunk of the stock functions' calls.
const STOCK_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_stdlib_stock.lua";

/// The source of the stock functions' calls.
const STOCK_SOURCE: &str = include_str!("__impl_stdlib_stock.lua");

/// The program of the stock functions' calls.
static STOCK_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(STOCK_SOURCE, STOCK_CHUNK_NAME));

#[cfg(test)]
mod tests {
    use super::{STDLIB_CHUNK_NAME, STDLIB_SOURCE, STOCK_CHUNK_NAME, STOCK_SOURCE};
    use crate::tests::assert_chunk_name_resolves;

    #[test]
    fn the_chunk_name_resolves_to_the_embedded_file() {
        assert_chunk_name_resolves("STDLIB_CHUNK_NAME", STDLIB_CHUNK_NAME, STDLIB_SOURCE);
        assert_chunk_name_resolves("STOCK_CHUNK_NAME", STOCK_CHUNK_NAME, STOCK_SOURCE);
    }
}
