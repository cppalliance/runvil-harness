//! A Lua value as the JSON data the Engine hands on: the arguments of a
//! tool call, the `item` of a spawn, a field of a message, a global that
//! prose names.
//!
//! `mlua`'s `from_value` makes the conversion, into the JSON data of
//! [`json_of`], which reads every key of a table as a name. Left to itself
//! it gives one value two results in two runs, in three ways, and
//! [`data_of`] closes each:
//!
//! - It reads a table with a positive length as a list of that many
//!   items. For a table with a gap the length differs from one run to the
//!   next, as [`listed`] tells, so the list would have three items in one
//!   run and five in another. Such a table is refused.
//! - It walks a table that it reads as a map in Lua's own order, which
//!   differs from one run to the next, and stops at the first entry it
//!   cannot convert. Of several such entries it would name a different one
//!   in each run. [`Faults`] names the entry whose key comes first in the
//!   shared order of keys. A key that is a table, a function, a userdata,
//!   or a thread has no place in that order, and `from_value` words its
//!   refusal by what the key holds, so [`Faults`] words it by the key's
//!   type alone.
//! - It reads a weak table, whose entries the collector can take between
//!   two reads. Such a table is refused, and so is a table whose metatable
//!   is weak, which is a view or not as the collector leaves that
//!   metatable.
//!
//! It also has no limit on how deep a value nests, and overflows the
//! stack on a value nested some thousands of tables deep. A value nested
//! past [`DEPTH_LIMIT`] is refused, which is also the deepest value that
//! reads back from its JSON text.
//!
//! The data is a copy of the value's text, made outside the Lua heap,
//! with one copy for each time a string occurs. A value that fits the
//! heap can hold one string many times over, so both walks count the
//! bytes of every string against the budget, each time it occurs, and a
//! value past it is refused.
//!
//! A view of a message record converts to the record's JSON form, which
//! nests and holds values and text as a table does, and can hold what an
//! earlier conversion made of another view. Both walks count a view as
//! that form, by its [`JsonSize`]: its nesting adds to the tables it sits
//! under, and its values and bytes are spent from the budget.
//!
//! The rules hold for the tables `from_value` reads. It reads the items
//! of a list and no other entry of that table, so what any other key of a
//! list holds is left out of the data and is not checked.

use std::ffi::c_void;

use mlua::{Lua, LuaSerdeExt, LuaString, Table, Value};
use serde_json::Value as Json;

use crate::json::{key_type, missing_list_item, too_deep};
use crate::messages::viewed_size;
use crate::meta_names::MetaNames;
use crate::order::{FirstRefusal, listed, position};
use crate::plain_json::json_of;
use crate::protocol::JsonSize;
use crate::proxy::{key_step, view_data_within};
use crate::walk_budget::{DEPTH_LIMIT, TooLarge, WalkBudget};

/// The JSON data `value` is, the same in every run. A view inside the
/// value is read as the table it stands for.
///
/// # Errors
/// Returns a deserialize error for a value with no JSON form: a value
/// past the budget of [`plain`], a table that [`plain`] refuses, and a
/// value `from_value` cannot convert. Of several entries of one table to
/// refuse, the error is that of the entry whose key comes first in the
/// shared order of keys. A value whose strings hold more bytes than the
/// budget is refused as too large, whatever else is wrong with it.
/// Returns the failure of a read as it is.
pub(crate) fn data_of(lua: &Lua, value: Value) -> mlua::Result<Json> {
    let given = value.clone();
    match plain(lua, value).and_then(|plain| json_of(lua, plain)) {
        Ok(json) => Ok(json),
        Err(error) => Err(Faults::new(lua)
            .first(&given, true)?
            .map_or(error, Fault::into_error)),
    }
}

/// `value` with a plain copy of the view's data in place of every sealed
/// view inside it, for a caller that converts the value with
/// `from_value`, which reads a view as the empty table it is.
///
/// A value that holds no view is returned as it is, so its conversion is
/// the one the caller always made. A table that holds a view becomes a
/// fresh table whose other entries are the table's own, and a copy keeps
/// the list marker of the table it copies, so an empty list still converts
/// to `[]`. The walk counts every value it visits against a
/// [`WalkBudget`], and the bytes of every string, a key or a value, each
/// time it occurs. A view whose data only a function can read, and a
/// chain of views that leads back to one of its own, stays the empty table
/// it is, which converts to an empty object.
///
/// Every table `from_value` reads of the result is one the walk passed
/// under fewer than [`DEPTH_LIMIT`] tables, or a copy of one. The walk
/// refuses a table it does not pass wherever `from_value` would read it,
/// so no table reaches the conversion unchecked: in a copy, a table that
/// holds itself is one `from_value` has not yet met, and it would read
/// that table a second time, below the depth the walk counted.
///
/// # Errors
/// Returns a deserialize error once the walk passes its budget, and for a
/// table that `from_value` would read and that is weak, has a gap in its
/// list, sits past [`DEPTH_LIMIT`], or holds itself: [`data_of`] gives
/// that refusal its text. Returns the failure of reading a table or of
/// building a copy.
pub(crate) fn plain(lua: &Lua, value: Value) -> mlua::Result<Value> {
    let mut walk = PlainWalk {
        lua,
        names: MetaNames::default(),
        marker: lua.array_metatable(),
        budget: WalkBudget::for_vm(lua),
        path: Vec::new(),
    };
    Ok(walk.value(&value, true)?.unwrap_or(value))
}

/// What [`plain`] says when a value holds more values than its budget.
const TOO_LARGE_TO_CONVERT: &str = "the value is too large to convert";

/// What [`plain`] says of a table it refuses, until [`Faults`] finds the
/// table and says why.
const REFUSED: &str = "the value has no JSON form";

/// What `from_value` says of a table that contains itself.
const RECURSIVE: &str = "recursive table detected";

/// What a refusal calls the converted value, before the path inside it.
const ROOT: &str = "value";

/// A string that is not UTF-8. `from_value` refuses every such string in
/// the same words, so [`Faults`] asks it about this one and never hands it
/// a string of the author's to read a second time.
const NOT_UTF8: [u8; 1] = [0xff];

/// One [`plain`] walk: the names it reads metatables by, the list marker
/// a copy keeps, what is left of the budget, and the data tables on the
/// path to the current value.
struct PlainWalk<'lua> {
    lua: &'lua Lua,
    names: MetaNames,
    marker: Table,
    budget: WalkBudget,
    path: Vec<*const c_void>,
}

impl PlainWalk<'_> {
    /// The plain form of `value`, or `None` when `value` holds no view and
    /// stays as it is. `live` says that `from_value` reads `value`.
    fn value(&mut self, value: &Value, live: bool) -> mlua::Result<Option<Value>> {
        if let Some(size) = charge_value(&mut self.budget, value)? {
            let deep = live && nests_too_deep(self.path.len(), &size);
            return if deep { Err(refused()) } else { Ok(None) };
        }
        let table = match value {
            Value::Table(table) => table,
            Value::String(text) => {
                charge_text(&mut self.budget, text)?;
                return Ok(None);
            }
            _ => return Ok(None),
        };
        let budget = &mut self.budget;
        let data = view_data_within(self.lua, &self.names, table, |cost| {
            budget.charge_read(cost).map_err(too_large)
        })?;
        // Each of these tables is left unwalked whether or not the
        // conversion reads it, so the budget spent does not follow `live`.
        // A weak table is one of them, and so are a view of one or through
        // one and a table whose metatable is weak.
        let walked = data.ok().filter(|data| {
            !self.path.contains(&data.to_pointer()) && self.path.len() < DEPTH_LIMIT
        });
        let Some(data) = walked else {
            return if live { Err(refused()) } else { Ok(None) };
        };
        let pointer = data.to_pointer();
        let is_view = pointer != table.to_pointer();
        self.path.push(pointer);
        let copy = self.entries(&data, is_view, live);
        self.path.pop();
        Ok(copy?.map(Value::Table))
    }

    /// A copy of `data` with the plain form of each value, made when
    /// `data` is a view's data or holds a view, and `None` otherwise. The
    /// copy starts at the first entry that needs it and takes the entries
    /// before that one as they are, so a table that needs none is read
    /// once and never copied.
    fn entries(&mut self, data: &Table, is_view: bool, live: bool) -> mlua::Result<Option<Table>> {
        let mut copy = if is_view {
            Some(self.like(data)?)
        } else {
            None
        };
        let reads = live.then(|| Reads::of(data, &self.marker));
        let mut items = Items::default();
        for (seen, pair) in data.pairs::<Value, Value>().enumerate() {
            let (key, value) = pair?;
            items.add(&key);
            if let Value::String(name) = &key {
                charge_text(&mut self.budget, name)?;
            }
            let read = reads.as_ref().is_some_and(|reads| reads.takes(&key));
            let plain = self.value(&value, read)?;
            if copy.is_none() && plain.is_some() {
                let fresh = self.like(data)?;
                for earlier in data.pairs::<Value, Value>().take(seen) {
                    let (key, value) = earlier?;
                    fresh.raw_set(key, value)?;
                }
                copy = Some(fresh);
            }
            if let Some(copy) = &copy {
                copy.raw_set(key, plain.unwrap_or(value))?;
            }
        }
        if live && items.has_gap() {
            return Err(refused());
        }
        Ok(copy)
    }

    /// An empty table that converts as `data` does: marked as a list when
    /// `data` is.
    fn like(&self, data: &Table) -> mlua::Result<Table> {
        let copy = self.lua.create_table()?;
        if data
            .metatable()
            .is_some_and(|metatable| metatable == self.marker)
        {
            copy.set_metatable(Some(self.marker.clone()))?;
        }
        Ok(copy)
    }
}

/// Why a value has no JSON form, as the walk that finds it hands it up.
///
/// A table can hold as many entries to refuse as it has entries, under a
/// path as long as the value is deep, and the walk keeps one of them. So
/// a refusal with a place is not put in words where it is found: it takes
/// one step of its path at each table it is handed up through, and
/// [`Fault::into_error`] writes the path once, for the one the walk
/// keeps.
enum Fault {
    /// A failure that names no place in the value: one in `from_value`'s
    /// own words, and that of a table that holds itself.
    Whole(mlua::Error),
    /// A refusal of the value at a place, which `reason` says why. `steps`
    /// lead from the value the walk is at down to the place, the last
    /// step first, each as [`key_step`] gives it.
    Placed { reason: String, steps: Vec<String> },
}

impl Fault {
    /// The refusal of the value the walk is at, which `reason` says why.
    fn here(reason: String) -> Self {
        Self::Placed {
            reason,
            steps: Vec::new(),
        }
    }

    /// This fault of the value under `key`, as the table that holds the
    /// key hands it up.
    fn under(mut self, key: &Value) -> Self {
        if let Self::Placed { steps, .. } = &mut self {
            steps.push(key_step(key));
        }
        self
    }

    /// The failure in words, with the path from the converted value to the
    /// place of a refusal that has one.
    fn into_error(self) -> mlua::Error {
        match self {
            Self::Whole(error) => error,
            Self::Placed { reason, steps } => {
                let shown = steps.iter().rev().fold(ROOT.to_owned(), |mut shown, step| {
                    shown.push_str(step);
                    shown
                });
                fault(format!("{shown} {reason}"))
            }
        }
    }
}

/// The walk that says why a value has no JSON form, made once the
/// conversion has failed: the list marker, what is left of the budget, and
/// the data tables on the path to the current value.
///
/// It visits what [`PlainWalk`] visits and counts each value the same
/// way, and it ends early for nothing. So a value past the budget of
/// [`PlainWalk`] is past this one's, and a value is past this one's in
/// every order of Lua's own walk or in none. What it visits does
/// not follow `live` either, which differs from one run to the next under
/// a table with a gap: a table past the depth limit, a table on the path,
/// a weak table, and a table whose metatable is weak are left unwalked,
/// read or not.
///
/// It reads every string the conversion reads, each time the string
/// occurs, where the conversion stops at its first fault. It counts the
/// bytes of every string it visits, a key or a value, read or not, as
/// [`PlainWalk`] does, before it reads one, and reads each once: the text
/// it reads is at most the budget's bytes, and a value past them is too
/// large in every order. It does not convert a record view, which always
/// converts: both walks count the bytes of the record's JSON form as they
/// count those of a string.
struct Faults<'lua> {
    lua: &'lua Lua,
    names: MetaNames,
    marker: Table,
    budget: WalkBudget,
    tables: Vec<*const c_void>,
}

impl<'lua> Faults<'lua> {
    fn new(lua: &'lua Lua) -> Self {
        Self {
            lua,
            names: MetaNames::default(),
            marker: lua.array_metatable(),
            budget: WalkBudget::for_vm(lua),
            tables: Vec::new(),
        }
    }

    /// Why `value` has no JSON form, or `None` when it has one. `live`
    /// says that `from_value` reads `value`: what it does not read has
    /// nothing to refuse.
    fn first(&mut self, value: &Value, live: bool) -> mlua::Result<Option<Fault>> {
        if let Some(size) = charge_value(&mut self.budget, value)? {
            let deep = live && nests_too_deep(self.tables.len(), &size);
            return Ok(deep.then(|| Fault::here(too_deep())));
        }
        let table = match value {
            Value::Table(table) => table,
            Value::Nil | Value::Boolean(_) | Value::Integer(_) | Value::Number(_) => {
                return Ok(None);
            }
            Value::String(text) => return Ok(self.text_fault(text, live)?.map(Fault::Whole)),
            other if live => {
                let refused = json_of(self.lua, other.clone()).err();
                return Ok(refused.map(Fault::Whole));
            }
            _ => return Ok(None),
        };
        let budget = &mut self.budget;
        let data = view_data_within(self.lua, &self.names, table, |cost| {
            budget.charge_read(cost).map_err(too_large)
        })?;
        let data = match data {
            Ok(data) => data,
            Err(weakness) => return Ok(live.then(|| Fault::here(format!("is {weakness}")))),
        };
        let pointer = data.to_pointer();
        if self.tables.contains(&pointer) {
            return Ok(live.then(|| Fault::Whole(fault(RECURSIVE.to_owned()))));
        }
        if self.tables.len() >= DEPTH_LIMIT {
            return Ok(live.then(|| Fault::here(too_deep())));
        }
        self.tables.push(pointer);
        let found = self.entries(&data, live);
        self.tables.pop();
        found
    }

    /// Why the table `data` has no JSON form: a gap in its list, or else
    /// the fault of the entry whose key comes first in the shared order of
    /// keys.
    fn entries(&mut self, data: &Table, live: bool) -> mlua::Result<Option<Fault>> {
        let reads = live.then(|| Reads::of(data, &self.marker));
        let mut items = Items::default();
        let mut first = FirstRefusal::default();
        for pair in data.pairs::<Value, Value>() {
            let (key, value) = pair?;
            items.add(&key);
            if let Value::String(name) = &key {
                charge_text(&mut self.budget, name)?;
            }
            let read = reads.as_ref().is_some_and(|reads| reads.takes(&key));
            let inside = self.first(&value, read)?.map(|inside| inside.under(&key));
            let found = match reads {
                Some(Reads::All) => self.key_fault(&key)?.or(inside),
                _ => inside,
            };
            if let Some(found) = found {
                first.refuse(&key, found);
            }
        }
        if live && items.has_gap() {
            let missing = i64::try_from(listed(data)?)
                .unwrap_or(i64::MAX)
                .saturating_add(1);
            return Ok(Some(Fault::here(missing_list_item(missing))));
        }
        Ok(first.into_result().err())
    }

    /// What `from_value` says of the string `text` as a value, or `None`
    /// for one it takes or does not read. The bytes are counted first.
    fn text_fault(&mut self, text: &LuaString, live: bool) -> mlua::Result<Option<mlua::Error>> {
        charge_text(&mut self.budget, text)?;
        if !live || text.to_str().is_ok() {
            return Ok(None);
        }
        let probe = self.lua.create_string(NOT_UTF8)?;
        Ok(json_of(self.lua, Value::String(probe)).err())
    }

    /// Why `key` cannot be the key of a map, or `None` for a key that
    /// can. The caller has counted the bytes of a string key.
    ///
    /// For a boolean, a number, or a string that is not UTF-8 it is what
    /// `from_value` says of a table that holds that key alone.
    ///
    /// A table, a function, a userdata, or a thread has no place in the
    /// shared order of keys, so [`FirstRefusal`] keeps the first of two of
    /// one type that Lua's own walk meets. `from_value` reads such a key,
    /// and says one thing of a key table with a list and another of one
    /// without. So the refusal of such a key is the Engine's own, and
    /// says the key's type and no more.
    fn key_fault(&self, key: &Value) -> mlua::Result<Option<Fault>> {
        let alone = match key {
            Value::String(name) if name.to_str().is_ok() => return Ok(None),
            Value::String(_) => Value::String(self.lua.create_string(NOT_UTF8)?),
            other => match position(other) {
                Ok(_) => other.clone(),
                Err(kind) => return Ok(Some(Fault::here(key_type(kind)))),
            },
        };
        let probe = self.lua.create_table()?;
        probe.raw_set(alone, true)?;
        let refused = json_of(self.lua, Value::Table(probe)).err();
        Ok(refused.map(Fault::Whole))
    }
}

/// The entries of one table that `from_value` reads.
enum Reads {
    /// A table it reads as a list: the items from 1 to this length.
    Items(usize),
    /// A table it reads as a map: every entry.
    All,
}

impl Reads {
    /// What `from_value` reads of `data`: the items of a table that is
    /// marked as a list or has a positive length, and else every entry.
    ///
    /// The length of a table with a gap differs from one run to the next,
    /// and so does this. Both walks refuse such a table whatever it is.
    fn of(data: &Table, marker: &Table) -> Self {
        let len = data.raw_len();
        let is_list = data
            .metatable()
            .is_some_and(|metatable| metatable == *marker);
        if is_list || len > 0 {
            Self::Items(len)
        } else {
            Self::All
        }
    }

    /// Whether `from_value` reads the entry under `key`.
    fn takes(&self, key: &Value) -> bool {
        match (self, key) {
            (Self::All, _) => true,
            (Self::Items(len), Value::Integer(index)) => {
                usize::try_from(*index).is_ok_and(|index| (1..=*len).contains(&index))
            }
            (Self::Items(_), _) => false,
        }
    }
}

/// The keys of one table that are integers from 1 up: how many there are,
/// and the largest.
#[derive(Default)]
struct Items {
    count: i64,
    last: i64,
}

impl Items {
    /// Counts `key`.
    fn add(&mut self, key: &Value) {
        if let Value::Integer(index) = key
            && *index >= 1
        {
            self.count += 1;
            self.last = self.last.max(*index);
        }
    }

    /// Whether the list has a gap: the keys are distinct, so they are 1
    /// to the largest exactly when there are that many.
    fn has_gap(&self) -> bool {
        self.count != self.last
    }
}

/// The size of the JSON form of the record `value` is a view of, or
/// `None` when `value` is no such view.
fn record_size(value: &Value) -> Option<JsonSize> {
    match value {
        Value::UserData(data) => viewed_size(data),
        _ => None,
    }
}

/// Spends from `budget` the values `value` itself is, for both walks: one
/// for a value that is no view of a record, and for a view the values of
/// the record's JSON form, which count the record's own object, and the
/// bytes of that form's text. Returns the size of that form, or `None`
/// when `value` is no such view.
fn charge_value(budget: &mut WalkBudget, value: &Value) -> mlua::Result<Option<JsonSize>> {
    let size = record_size(value);
    let values = size.as_ref().map_or(1, |size| size.values);
    budget.charge_nodes(values).map_err(too_large)?;
    if let Some(size) = &size {
        budget.charge_bytes(size.bytes).map_err(too_large)?;
    }
    Ok(size)
}

/// Spends from `budget` the bytes of `text`, for both walks.
fn charge_text(budget: &mut WalkBudget, text: &LuaString) -> mlua::Result<()> {
    budget
        .charge_bytes(text.as_bytes().len())
        .map_err(too_large)
}

/// Whether data of `size` that sits under `enclosing` tables nests past
/// [`DEPTH_LIMIT`].
fn nests_too_deep(enclosing: usize, size: &JsonSize) -> bool {
    enclosing + size.depth > DEPTH_LIMIT
}

/// The refusal of a walk past its budget.
fn too_large(_: TooLarge) -> mlua::Error {
    fault(TOO_LARGE_TO_CONVERT.to_owned())
}

/// The refusal of a table by [`plain`].
fn refused() -> mlua::Error {
    fault(REFUSED.to_owned())
}

/// A value with no JSON form, for the reason `reason`.
fn fault(reason: String) -> mlua::Error {
    mlua::Error::DeserializeError(reason)
}

#[cfg(test)]
#[path = "data-tests.rs"]
mod tests;
