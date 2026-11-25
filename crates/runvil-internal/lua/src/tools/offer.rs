//! The offer verbs, `tools.offer` and `tools.offer_dynamic`: one
//! installer, parameterized by name and mode, and its decode.
//!
//! A verb takes one argument: a string with a `/`, a tool id; a string
//! without one, a Pack id, since Pack ids are one segment and tool ids
//! two or more; a tool object; a Pack object; or an array mixing them.
//! An array is the entries its `__len` gives, read through `__index`, or
//! without a `__len` the entries the table holds from index 1 to the
//! first it lacks. It has no gap, no nested table, and no raw key outside
//! those entries. The rendered-list refusal comes first, and every entry
//! is checked before any is recorded.

use std::sync::{Arc, Mutex};

use mlua::{Lua, Table, Value, Variadic};
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use super::userdata::LuaToolHandle;
use crate::adhoc::AdhocTools;
use crate::bindings::{RosterEntry, ToolSet, wire_name_of};
use crate::error::{self, Result};
use crate::messages::MessageList;
use crate::order::{FirstRefusal, listed};
use crate::packs::{LuaPackHandle, is_adhoc, pack_object};
use crate::proxy::key_text;
use crate::scope::{OFFERS_POISONED, OfferMode, OfferTarget, Offers};
use crate::walk_budget::HELD_KEY_LIMIT;

/// The refusal of an offer verb once the section's sets are frozen,
/// after the verb's name.
const TOOL_LIST_FIXED: &str = "the section's tool list is fixed at its first models.loop round; \
                               make every offer before the first loop, or offer in another \
                               section";

/// What an offer verb checks its entries against and records into: the
/// run's tool set, the section's offers, the VM's adhoc tools, and the
/// section's conversation, whose frozen sets fix the offers.
#[derive(Clone)]
pub(super) struct Offering {
    pub(super) set: Arc<ToolSet>,
    pub(super) offers: Arc<Mutex<Offers>>,
    pub(super) adhoc: AdhocTools,
    pub(super) list: MessageList,
}

/// Installs the offer verb `tools.<key>` on `tools`, recording under
/// `mode`.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the function cannot be created or
/// installed.
pub(super) fn install_offer(
    lua: &Lua,
    tools: &Table,
    key: &'static str,
    mode: OfferMode,
    offering: &Offering,
) -> Result<()> {
    let verb = format!("tools.{key}");
    let offering = offering.clone();
    let offer = lua
        .create_function(move |lua, args: Variadic<Value>| {
            refuse_after_render(&verb, &offering.list)?;
            let targets = offering.targets(lua, &verb, args)?;
            let mut offers = offering
                .offers
                .lock()
                .map_err(|_| mlua::Error::external(error::make_internal_error(OFFERS_POISONED)))?;
            for target in targets {
                match target {
                    OfferTarget::Tool(id) => offers.add_tool(id, mode),
                    OfferTarget::Pack(pack) => offers.add_pack(pack, mode),
                }
            }
            Ok(())
        })
        .map_err(error::make_lua_error)?;
    tools.set(key, offer).map_err(error::make_lua_error)
}

/// Refuses the offer verb `verb` once `list` holds the section's frozen sets.
fn refuse_after_render(verb: &str, list: &MessageList) -> mlua::Result<()> {
    if list.rendered_tools().is_some() {
        return Err(mlua::Error::external(format!("{verb}: {TOOL_LIST_FIXED}")));
    }
    Ok(())
}

impl Offering {
    /// Reads `verb`'s arguments as the targets they name, checking every
    /// entry before returning any. No argument names nothing.
    fn targets(
        &self,
        lua: &Lua,
        verb: &str,
        args: Variadic<Value>,
    ) -> mlua::Result<Vec<OfferTarget>> {
        let mut args = args.into_iter();
        let Some(target) = args.next() else {
            return Ok(Vec::new());
        };
        if let Some(extra) = args.next() {
            return Err(mlua::Error::external(format!(
                "{verb} takes one tool or Pack, or an array of them, got extra {}",
                extra.type_name()
            )));
        }
        match target {
            Value::Table(table) => array(verb, &table)?
                .iter()
                .enumerate()
                .map(|(index, entry)| self.target(lua, verb, entry, Some(index + 1)))
                .collect(),
            single => Ok(vec![self.target(lua, verb, &single, None)?]),
        }
    }

    /// Reads one entry, at `index` of an array or alone, as the tool or
    /// Pack it names. A tool object under the `adhoc` Pack must be a
    /// defined adhoc tool, since only those have a binding to render.
    fn target(
        &self,
        lua: &Lua,
        verb: &str,
        entry: &Value,
        index: Option<usize>,
    ) -> mlua::Result<OfferTarget> {
        match entry {
            Value::String(text) => self.named(lua, verb, &text.to_string_lossy()),
            Value::UserData(object) => {
                if let Ok(tool) = object.borrow::<LuaToolHandle>() {
                    let id = tool.id();
                    if is_adhoc(&id.pack()) {
                        return self.named(lua, verb, &id.to_string());
                    }
                    return Ok(OfferTarget::Tool(id.clone()));
                }
                if let Ok(pack) = object.borrow::<LuaPackHandle>() {
                    return Ok(OfferTarget::Pack(pack.name().clone()));
                }
                Err(make_wrong_type_error(verb, entry, index))
            }
            other => Err(make_wrong_type_error(verb, other, index)),
        }
    }

    /// Reads a string entry: with a `/`, the id of a tool in the run
    /// catalog or a defined adhoc tool; without one, the id of a Pack with
    /// an object.
    fn named(&self, lua: &Lua, verb: &str, name: &str) -> mlua::Result<OfferTarget> {
        if name.contains('/') {
            let known = match ToolId::parse(name) {
                Ok(id) if is_adhoc(&id.pack()) => self
                    .adhoc
                    .by_id(&id)
                    .map_err(mlua::Error::external)?
                    .is_some()
                    .then_some(id),
                Ok(id) => self.set.catalog().get(&id).is_some().then_some(id),
                Err(_) => None,
            };
            return known.map(OfferTarget::Tool).ok_or_else(|| {
                mlua::Error::external(format!("{verb}: {name:?} is not a tool in this run"))
            });
        }
        if let Ok(pack) = PackId::parse(name) {
            if let Some(RosterEntry::Unavailable { reason }) = self.set.roster().get(&pack) {
                return Err(mlua::Error::external(format!(
                    "{verb}: the {pack} Pack is unavailable in this run: {reason}"
                )));
            }
            if pack_object(lua, name)?.is_some() {
                return Ok(OfferTarget::Pack(pack));
            }
        }
        let hint = match self.wire_named(name)? {
            Some(id) => format!(" (did you mean '{id}'?)"),
            None => String::new(),
        };
        Err(mlua::Error::external(format!(
            "{verb}: no Pack named {name:?} in this run; declare it under uses: to require \
             it, or offer it only when tools.packs.get({name:?}) returns it{hint}"
        )))
    }

    /// The id of the catalog tool, or else the defined adhoc tool, whose
    /// wire name is `name`.
    fn wire_named(&self, name: &str) -> mlua::Result<Option<ToolId>> {
        if let Some(info) = self
            .set
            .catalog()
            .tools()
            .iter()
            .find(|info| wire_name_of(info) == name)
        {
            return Ok(Some(info.id.clone()));
        }
        Ok(self
            .adhoc
            .by_wire_name(name)
            .map_err(mlua::Error::external)?
            .map(|binding| binding.id().clone()))
    }
}

/// The entries of the array `table`. With a `__len` the array has the
/// entries that gives, each read through `__index`. Without one it has
/// the entries the table itself holds from index 1 to the first it lacks,
/// which [`listed`] counts. A count above [`HELD_KEY_LIMIT`] is refused
/// before any entry is kept, since each entry holds a Lua reference until
/// the offer is recorded.
///
/// The array ends at the first nil the read meets. A table before that
/// nil is refused, then a raw key outside the array, then a `__len` past
/// its end: of several entries the first, and of several keys the one that
/// comes first in the shared order of keys.
///
/// Not `#table` without a `__len`: for a table with a gap that differs
/// from one run to the next, as [`listed`] tells, and an `__index` that
/// gives a value at the gap would let the read go on as far as it allowed.
/// So one run would accept `{ [1] = a, [2] = a, [3] = a, [5] = a }` with an
/// `__index` of `{ [4] = a }` and another refuse it for the key 5. Counted
/// from the table itself, every run refuses it for that key.
fn array(verb: &str, table: &Table) -> mlua::Result<Vec<Value>> {
    let has_len = match table.metatable() {
        Some(metatable) => !metatable.raw_get::<Value>("__len")?.is_nil(),
        None => false,
    };
    let len = if has_len {
        table.len()?
    } else {
        i64::try_from(listed(table)?).unwrap_or(i64::MAX)
    };
    let count = usize::try_from(len).unwrap_or(if len > 0 { usize::MAX } else { 0 });
    if count > HELD_KEY_LIMIT {
        return Err(mlua::Error::external(format!(
            "{verb} takes an array of at most {HELD_KEY_LIMIT} entries, got {len}"
        )));
    }
    let mut entries = Vec::with_capacity(count);
    for index in 1..=count {
        let entry: Value = table.get(index)?;
        match entry {
            Value::Nil => break,
            Value::Table(_) => return Err(make_wrong_type_error(verb, &entry, Some(index))),
            _ => entries.push(entry),
        }
    }
    let end = entries.len();
    let mut outside = FirstRefusal::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let inside = match key {
            Value::Integer(index) => {
                usize::try_from(index).is_ok_and(|index| (1..=end).contains(&index))
            }
            _ => false,
        };
        if !inside {
            outside.refuse(&key, key_text(&key));
        }
    }
    outside.into_result().map_err(|key| {
        mlua::Error::external(format!(
            "{verb} takes an array, got a table with the key {key}"
        ))
    })?;
    if end < count {
        return Err(make_wrong_type_error(verb, &Value::Nil, Some(end + 1)));
    }
    Ok(entries)
}

/// The refusal of `entry`, at `index` of an array or alone, as a value
/// that names no tool or Pack.
fn make_wrong_type_error(verb: &str, entry: &Value, index: Option<usize>) -> mlua::Error {
    let kind = entry.type_name();
    mlua::Error::external(match index {
        Some(index) => format!(
            "{verb} takes tool ids, Pack ids, tool objects, or Pack objects in an array, \
             got {kind} at index {index}"
        ),
        None => format!(
            "{verb} takes tool ids, Pack ids, tool objects, Pack objects, or an array of \
             them, got {kind}"
        ),
    })
}
