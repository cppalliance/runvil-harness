//! Deterministic `pairs`/`next` for section VMs.
//!
//! Lua leaves a table's hash traversal order unspecified, so two runs (or two
//! fresh VMs) can visit the same table differently. [`install_deterministic_iteration`]
//! replaces the `pairs` and `next` globals with one deterministic walk, in
//! the order of [`WalkPlace`]: the integer keys from 1 up by value, then
//! the other keys by the shared [`SortKey`] order (booleans, then numbers,
//! then strings). A table that holds a key of any other type is refused, as
//! the end of this text explains. A `__pairs` metamethod on a table's or a
//! userdata's metatable still wins, as in stock Lua; any other non-table
//! value, a userdata without `__pairs` included, is refused.
//!
//! The place of a key is the key's own. It does not follow `#t`, which for
//! a table with a gap in its indices is any index before a gap, picked
//! with the seed of the state ([`order::listed`] tells more). So a list
//! walks in index order, a table with a gap walks the same way in every
//! run, and a key keeps its place when the block clears another.
//!
//! A sealed view (see [`crate::proxy::unseal`]) walks as the table it
//! stands for: the keys are its data table's, each value is the one a
//! read through the view gives, and the state `pairs` returns is the view
//! itself, so a walk never hands out the data table.
//!
//! `pairs` captures that order once and advances it by position, so a key
//! whose value becomes `nil` mid-traversal is skipped rather than visited with
//! a nil value. `next` stays stateless and reads the
//! live table on each call: its answer is the least key after the one it was
//! given, whether the table still holds that one or the block cleared it.
//! So a loop of `next` that clears keys as it goes meets every key left
//! once.
//! One step of `next` reads every key of the table, so a loop of `next`
//! over `n` keys costs `n * n` reads, where `pairs` sorts once.
//!
//! Rust holds no Lua reference for a walk between two steps, and a fixed
//! number within one. `mlua` gives every Lua value a Rust value holds a
//! slot on one stack of about a million, and panics when it cannot get one.
//! So `pairs` writes the keys to a Lua list one at a time, sorts their
//! places as plain data, moves each key of the list to its slot, and hands
//! the list to an iterator that is a Lua function (`__impl_iteration.lua`,
//! which tells what an iterator in Rust did). `next` keeps one candidate
//! while it reads. Both give the table's own keys: a copy of a string key
//! would cost its length again from the VM's memory.
//!
//! The ordering covers every boolean, number, and string key: a string that
//! is not valid UTF-8 sorts by its bytes with the other strings, and an
//! infinite number by its value with the other numbers. A table, function,
//! userdata, or thread key has no place in it. Only the memory address tells
//! two such keys apart, Lua's own `next` orders them by a hash of that
//! address, and the address differs from one run to the next. So `pairs` and
//! `next` refuse a table that holds such a key, and `next` refuses one as
//! the key to go on from. A walk that visited them in Lua's order would let
//! a block act on the layout of memory; one that dropped them would lose
//! data without a word. The table itself stays usable: `t[key]` reads and
//! writes it, and only a walk is refused.
//!
//! A weak table is refused for the same reason: what a walk finds in it
//! depends on Lua's own order ([`order::is_weak`] tells how). It stays
//! usable by key too.

use std::cmp::Ordering;
use std::sync::LazyLock;

use mlua::{Function, Lua, MultiValue, Table, Value};

use crate::error::{self, Error, Result};
use crate::order::{self, FirstRefusal, SortKey};
use crate::program::LuaProgram;
use crate::proxy::view_data;

/// Installs the deterministic `next` and `pairs` globals, replacing the base
/// library's hash-order versions.
///
/// Must run while the `rawget` global is still there, which the iterator
/// chunk captures.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the iterator chunk cannot load or run,
/// or if either global cannot be created or installed.
pub(crate) fn install_deterministic_iteration(lua: &Lua) -> Result<()> {
    let make_iterator = ITERATION_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call::<Function>(())
        .map_err(error::make_lua_error)?;
    let globals = lua.globals();
    let next_fn = lua
        .create_function(deterministic_next)
        .map_err(error::make_lua_error)?;
    globals
        .raw_set("next", &next_fn)
        .map_err(error::make_lua_error)?;
    let pairs_fn = lua
        .create_function(move |lua, value: Value| pairs(lua, &make_iterator, value))
        .map_err(error::make_lua_error)?;
    globals
        .raw_set("pairs", pairs_fn)
        .map_err(error::make_lua_error)
}

/// Where a key sits in the walk order: an integer from 1 up comes before
/// every other key, by its value, and the other keys follow by
/// [`SortKey`]. So the items of a list precede a boolean key, though
/// [`SortKey`] ranks a boolean below a number.
enum WalkPlace {
    /// An integer key from 1 up.
    Index(i64),
    /// Any other key: never an integer from 1 up.
    Scalar(SortKey),
}

impl WalkPlace {
    /// The place of `key`, or the name of its type when the order has no
    /// place for it.
    fn of(key: &Value) -> std::result::Result<Self, &'static str> {
        Ok(match order::position(key)? {
            SortKey::Integer(index) if index >= 1 => WalkPlace::Index(index),
            place => WalkPlace::Scalar(place),
        })
    }

    /// Orders two places.
    fn compare(&self, other: &WalkPlace) -> Ordering {
        match (self, other) {
            (WalkPlace::Index(left), WalkPlace::Index(right)) => left.cmp(right),
            (WalkPlace::Index(_), WalkPlace::Scalar(_)) => Ordering::Less,
            (WalkPlace::Scalar(_), WalkPlace::Index(_)) => Ordering::Greater,
            (WalkPlace::Scalar(left), WalkPlace::Scalar(right)) => left.compare(right),
        }
    }
}

/// The refusal of a table that holds a key the walk order has no place for,
/// in the name of the function the author called.
fn refused(function: &str, type_name: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "'{function}' walks boolean, number, and string keys only: the table has a {type_name} \
         key, which has no order that is the same in two runs"
    ))
}

/// Refuses a weak table, in the name of the function the author called:
/// what a walk finds in one follows Lua's own order, as [`order::is_weak`]
/// tells. The check is made at each walk, since a table can become weak
/// after it has its metatable.
fn refuse_weak(function: &str, data: &Table) -> mlua::Result<()> {
    if order::is_weak(data)? {
        return Err(mlua::Error::runtime(format!(
            "'{function}' does not walk a weak table, one whose metatable sets __mode: the \
             entries the collector leaves in it are not the same in two runs"
        )));
    }
    Ok(())
}

/// Whether `key` is an index of the list, `1..=listed`.
fn in_list(key: &Value, listed: usize) -> bool {
    matches!(key, Value::Integer(index)
        if usize::try_from(*index).is_ok_and(|index| (1..=listed).contains(&index)))
}

/// The index `index` of the list as a Lua integer.
fn array_key(index: usize) -> mlua::Result<Value> {
    i64::try_from(index)
        .map(Value::Integer)
        .map_err(|_| mlua::Error::runtime("table index exceeds the integer range".to_owned()))
}

/// The `next(table, key)` replacement: the key strictly after `key` in the
/// walk order, with its value, or `(nil, nil)` at the end. A sealed view
/// is read as the table it stands for. A weak table is refused, a table
/// that holds a key with no place in the order is, and so is such a key as
/// `key`.
///
/// One read of the table finds the answer: the least of its keys that
/// comes after `key`. The key and the value returned are the table's own,
/// and the read keeps one candidate, so a step holds a fixed number of Lua
/// references whatever the table's size.
fn deterministic_next(_: &Lua, (table, previous): (Table, Value)) -> mlua::Result<(Value, Value)> {
    let data = view_data(&table)?;
    refuse_weak("next", &data)?;
    // The table is refused before the key is, so the key's refusal waits
    // for the read below, which then takes no candidate.
    let after = match &previous {
        Value::Nil => Ok(None),
        key => previous_place(key).map(Some),
    };
    let mut least: Option<(WalkPlace, Value, Value)> = None;
    let mut unplaced = FirstRefusal::default();
    for pair in data.pairs::<Value, Value>() {
        let (key, value) = pair?;
        if value.is_nil() {
            continue;
        }
        let place = match WalkPlace::of(&key) {
            Ok(place) => place,
            Err(type_name) => {
                unplaced.refuse(&key, type_name);
                continue;
            }
        };
        let later = match &after {
            Ok(Some(after)) => place.compare(after) == Ordering::Greater,
            Ok(None) => true,
            Err(_) => false,
        };
        let earlier = least
            .as_ref()
            .is_none_or(|(least, ..)| place.compare(least) == Ordering::Less);
        if later && earlier {
            least = Some((place, key, value));
        }
    }
    unplaced
        .into_result()
        .map_err(|type_name| refused("next", type_name))?;
    after?;
    Ok(least.map_or((Value::Nil, Value::Nil), |(_, key, value)| (key, value)))
}

/// The `pairs(value)` replacement: a `__pairs` metamethod's results when the
/// table's or userdata's metatable holds one, otherwise a stateful iterator
/// over the table's walk order, where a sealed view is read as the table it
/// stands for. A userdata without `__pairs` is refused like any other
/// non-table value, and so are a weak table and a table that holds a key
/// with no place in the walk order. `make_iterator` is the function of the
/// iterator chunk.
fn pairs(lua: &Lua, make_iterator: &Function, value: Value) -> mlua::Result<MultiValue> {
    let metamethod = match &value {
        Value::Table(table) => match table.metatable() {
            Some(metatable) => metatable.raw_get::<Value>("__pairs")?,
            None => Value::Nil,
        },
        // mlua exposes no metatable for a userdata it did not create or one
        // already destructed, so such a value is refused like any userdata
        // without `__pairs`.
        Value::UserData(userdata) => match userdata.metatable() {
            Ok(metatable) => metatable.get::<Value>("__pairs")?,
            Err(_) => Value::Nil,
        },
        _ => Value::Nil,
    };
    match metamethod {
        Value::Nil => {}
        Value::Function(callable) => return callable.call::<MultiValue>(value),
        other => {
            return Err(mlua::Error::runtime(format!(
                "attempt to call a {} value (metamethod '__pairs')",
                other.type_name()
            )));
        }
    }
    let table = match value {
        Value::Table(table) => table,
        other => {
            return Err(mlua::Error::runtime(format!(
                "bad argument #1 to 'pairs' (table expected, got {})",
                other.type_name()
            )));
        }
    };
    // Capture the walk order once, then advance it by position. Advancing a
    // snapshot rather than re-deriving order from the live table means a key
    // cleared mid-loop reads as nil and is skipped, and no key-based lookup
    // can end the walk early. A key the loop adds is not in the snapshot, so
    // the refusal looks at the table as it was when the walk began.
    let data = view_data(&table)?;
    refuse_weak("pairs", &data)?;
    let (order, count) = walk_list(lua, &data)?;
    let iterator = make_iterator.call::<Function>((data, order, count))?;
    let state = table;
    // Stock `pairs` yields `(next, table, nil)`; the iterator ignores the
    // state, but keeping the shape means a caller reading the second result
    // sees the table, exactly as with the pre-replacement `pairs`.
    let mut iterator_value = MultiValue::new();
    iterator_value.push_back(Value::Function(iterator));
    iterator_value.push_back(Value::Table(state));
    iterator_value.push_back(Value::Nil);
    Ok(iterator_value)
}

/// The table's live keys as a Lua list in the walk order, and their count:
/// the indices of the list, which [`order::listed`] counts, then every
/// other key by [`WalkPlace`].
///
/// The list holds the table's own keys. A copy of a string key would cost
/// its length again from the VM's memory, and a table with one long key
/// could then not be walked. Rust holds one key at a time: the other keys
/// go to the list as Lua's own walk meets them, and [`order::sort_list`]
/// then moves each key to its slot. The indices of the list need no sort
/// and no place kept for them, so a walk of a list costs a read and a
/// write for each item.
///
/// # Errors
/// Refuses, in the name of `pairs`, a table that holds a key
/// [`order::position`] has no place for. The refusal names the type of
/// such a key, and of several types the first by name: the key Lua's own
/// walk meets first differs from one run to the next, and so would a
/// refusal that named it.
fn walk_list(lua: &Lua, table: &Table) -> mlua::Result<(Table, usize)> {
    let listed = order::listed(table)?;
    let list = lua.create_table_with_capacity(listed, 0)?;
    for index in 1..=listed {
        list.raw_set(index, array_key(index)?)?;
    }
    let mut places = Vec::new();
    let mut unplaced = FirstRefusal::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        if value.is_nil() || in_list(&key, listed) {
            continue;
        }
        match WalkPlace::of(&key) {
            Ok(place) => {
                places.push(place);
                list.raw_set(listed + places.len(), key)?;
            }
            Err(type_name) => unplaced.refuse(&key, type_name),
        }
    }
    unplaced
        .into_result()
        .map_err(|type_name| refused("pairs", type_name))?;
    order::sort_list(&list, listed, &places, WalkPlace::compare)?;
    Ok((list, listed + places.len()))
}

/// The walk place of `previous`, the key a `next` call goes on from,
/// whether the table holds it or not. A float with the value of an integer
/// has that integer's place, as it is that integer's key when Lua itself
/// reads a table by it.
///
/// The place is the key's own and exact: `mlua` compares an integer with a
/// float after it rounds the integer to a float, and so takes
/// `math.maxinteger` and `0x1p63` for one key.
///
/// # Errors
/// Refuses a key [`order::position`] has no place for. The table holds no
/// such key, or the walk would have refused it, so there is no key to go on
/// from.
fn previous_place(previous: &Value) -> mlua::Result<WalkPlace> {
    let place = match previous {
        Value::Number(number) => match integer_of(*number) {
            Some(integer) => WalkPlace::of(&Value::Integer(integer)),
            None => WalkPlace::of(previous),
        },
        key => WalkPlace::of(key),
    };
    place.map_err(|type_name| {
        mlua::Error::runtime(format!(
            "'next' walks boolean, number, and string keys only: got a {type_name} key to go on \
             from"
        ))
    })
}

/// The integer `number` has the exact value of, when there is one.
fn integer_of(number: f64) -> Option<i64> {
    /// -2^63, the least integer, and the least float with an integer's value.
    const MIN: f64 = -9_223_372_036_854_775_808.0;
    /// 2^63, one past the greatest integer.
    const ABOVE_MAX: f64 = 9_223_372_036_854_775_808.0;
    let whole = number.fract() == 0.0 && (MIN..ABOVE_MAX).contains(&number);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the float has no fraction and is inside i64's range"
    )]
    whole.then_some(number as i64)
}

/// The iterator chunk's name.
const ITERATION_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_iteration.lua";

/// The iterator chunk's source.
const ITERATION_SOURCE: &str = include_str!("__impl_iteration.lua");

/// The iterator chunk's program.
static ITERATION_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(ITERATION_SOURCE, ITERATION_CHUNK_NAME));

#[cfg(test)]
#[path = "iteration-tests.rs"]
mod tests;
