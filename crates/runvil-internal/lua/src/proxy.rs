//! The read-only proxy every seal builds on - `sys`, the `args` table of
//! the chain's arguments and each table inside it, a sealed prelude
//! global, and a prelude's `var` view - the key
//! formatter their refusals and field paths share, and [`unseal`], which
//! reads a table through such views without calling into Lua.
//! [`view_len`] and [`view_data`] make a view measure and walk as the
//! table it stands for, and [`plain`] gives a value's plain form to a
//! conversion that reads a view as the empty table it is.
//! `tools.stats` and `tools.packs` build on the walkable [`read_only_walk`].
//! Tool and Pack objects share [`registry_object`] and [`add_frozen_guard`].

use std::collections::HashSet;
use std::ffi::c_void;

use mlua::{
    AnyUserData, Function, Lua, LuaSerdeExt, MaybeSend, MetaMethod, MultiValue, Table,
    UserDataMethods, Value,
};

use crate::walk_budget::{TooLarge, WalkBudget};

/// Renders a table key for a refusal: a string as its text, an integer as
/// its digits, and any other key by its type name.
pub(crate) fn key_text(key: &Value) -> String {
    match key {
        Value::String(name) => name.to_string_lossy(),
        Value::Integer(index) => index.to_string(),
        other => other.type_name().to_owned(),
    }
}

/// Renders the path of the field `key` under `path`: `path.name` for a
/// string key, `path[key]` for any other, its key rendered by [`key_text`].
pub(crate) fn key_path(path: &str, key: &Value) -> String {
    match key {
        Value::String(name) => format!("{path}.{}", name.to_string_lossy()),
        other => format!("{path}[{}]", key_text(other)),
    }
}

/// Builds an empty proxy whose `__index` is `index`, whose `__newindex`
/// raises the text `refusal` builds from the refused key, and whose
/// `__metatable` is `label`, so `getmetatable` returns only the label.
///
/// A proxy whose `index` is a table is a view of that table: its `__len`
/// is the table's length, and `pairs`, `next`, and [`plain`] read the
/// table through it. A proxy whose `index` is a function holds no table
/// to measure or walk, so `#` gives 0 and `pairs` sees nothing.
pub(crate) fn read_only_proxy(
    lua: &Lua,
    index: Value,
    refusal: impl Fn(&Value) -> String + MaybeSend + 'static,
    label: &str,
) -> mlua::Result<Table> {
    let newindex = lua.create_function(
        move |_lua, (_proxy, key, _value): (Value, Value, Value)| -> mlua::Result<()> {
            Err(mlua::Error::runtime(refusal(&key)))
        },
    )?;
    let metatable = lua.create_table()?;
    if let Value::Table(data) = &index {
        metatable.raw_set("__len", view_len(lua, data)?)?;
    }
    metatable.raw_set("__index", index)?;
    metatable.raw_set("__newindex", newindex)?;
    metatable.raw_set("__metatable", label)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

/// What a table stands for once sealed views are read through.
pub(crate) enum Unsealed {
    /// The table that holds the data.
    Data(Table),
    /// A view whose data only a function can read, with the view's label
    /// when it has one.
    Unreadable(Option<String>),
    /// A chain of views that leads back to one of its own views.
    Loop,
}

/// Reads `table` through sealed views, never calling into Lua.
///
/// A sealed view is a table with no entries of its own whose metatable has
/// both `__index` and `__metatable`: a table `__index` is its data, read
/// by these same rules, and a function `__index` cannot be read, labelled
/// by a string `__metatable`. Any other table is its own data. `hop` runs
/// once for each view passed, so the caller can bound the work, and the
/// views already passed sit in a set, so a long chain costs time in
/// proportion to its length.
///
/// # Errors
/// Returns the failure of `hop`, or of reading a metatable.
pub(crate) fn unseal(
    table: &Table,
    mut hop: impl FnMut() -> mlua::Result<()>,
) -> mlua::Result<Unsealed> {
    let mut current = table.clone();
    let mut views = HashSet::new();
    loop {
        let Some(metatable) = current.metatable() else {
            return Ok(Unsealed::Data(current));
        };
        let label: Value = metatable.raw_get("__metatable")?;
        if label.is_nil()
            || current
                .pairs::<Value, Value>()
                .next()
                .transpose()?
                .is_some()
        {
            return Ok(Unsealed::Data(current));
        }
        match metatable.raw_get::<Value>("__index")? {
            Value::Table(data) => {
                hop()?;
                views.insert(current.to_pointer());
                if views.contains(&data.to_pointer()) {
                    return Ok(Unsealed::Loop);
                }
                current = data;
            }
            Value::Function(_) => {
                let label = match label {
                    Value::String(label) => Some(label.to_string_lossy()),
                    _ => None,
                };
                return Ok(Unsealed::Unreadable(label));
            }
            _ => return Ok(Unsealed::Data(current)),
        }
    }
}

/// The `__len` of a view whose data table is `data`: the length of `data`
/// itself, so `#`, `table.concat`, `table.unpack`, and `table.insert`
/// measure the view as the table it stands for.
pub(crate) fn view_len(lua: &Lua, data: &Table) -> mlua::Result<Function> {
    let data = data.clone();
    lua.create_function(move |_, _: MultiValue| Ok(data.raw_len()))
}

/// The table `table` stands for: the data table of a sealed view, read by
/// [`unseal`], and `table` itself when it is no view or its data cannot be
/// read as a table.
pub(crate) fn view_data(table: &Table) -> mlua::Result<Table> {
    match unseal(table, || Ok(()))? {
        Unsealed::Data(data) => Ok(data),
        Unsealed::Unreadable(_) | Unsealed::Loop => Ok(table.clone()),
    }
}

/// The most table levels [`plain`] descends, the top-level table being
/// level 1. A table nested past it stays as it is.
const PLAIN_DEPTH_LIMIT: usize = 200;

/// What [`plain`] says when a value holds more values than its budget.
const TOO_LARGE_TO_CONVERT: &str = "the value is too large to convert";

/// `value` with a plain copy of the view's data in place of every sealed
/// view inside it, for a caller that converts the value with
/// `from_value`, which reads a view as the empty table it is.
///
/// A value that holds no view is returned as it is, so its conversion is
/// the one the caller always made. A table that holds a view becomes a
/// fresh table whose other entries are the table's own, and a copy keeps
/// the list marker of the table it copies, so an empty list still converts
/// to `[]`. The walk counts every value it visits against a
/// [`WalkBudget`]. It does not descend into a table already on the path to
/// it, or one at a level past [`PLAIN_DEPTH_LIMIT`], and it leaves a view
/// whose data only a function can read as it is: the conversion that
/// follows keeps its own behavior for each.
///
/// # Errors
/// Returns a deserialize error once the walk passes its budget, and the
/// failure of reading a table or of building a copy.
pub(crate) fn plain(lua: &Lua, value: Value) -> mlua::Result<Value> {
    let mut walk = PlainWalk {
        lua,
        marker: lua.array_metatable(),
        budget: WalkBudget::for_vm(lua),
        path: Vec::new(),
    };
    Ok(walk.value(&value)?.unwrap_or(value))
}

/// One [`plain`] walk: the list marker a copy keeps, what is left of the
/// budget, and the data tables on the path to the current value.
struct PlainWalk<'lua> {
    lua: &'lua Lua,
    marker: Table,
    budget: WalkBudget,
    path: Vec<*const c_void>,
}

impl PlainWalk<'_> {
    /// The plain form of `value`, or `None` when `value` holds no view and
    /// stays as it is.
    fn value(&mut self, value: &Value) -> mlua::Result<Option<Value>> {
        self.budget.charge_node().map_err(too_large)?;
        let Value::Table(table) = value else {
            return Ok(None);
        };
        let budget = &mut self.budget;
        let data = match unseal(table, || budget.charge_node().map_err(too_large))? {
            Unsealed::Data(data) => data,
            Unsealed::Unreadable(_) | Unsealed::Loop => return Ok(None),
        };
        let pointer = data.to_pointer();
        if self.path.len() >= PLAIN_DEPTH_LIMIT || self.path.contains(&pointer) {
            return Ok(None);
        }
        let is_view = pointer != table.to_pointer();
        self.path.push(pointer);
        let copy = self.entries(&data, is_view);
        self.path.pop();
        Ok(copy?.map(Value::Table))
    }

    /// A copy of `data` with the plain form of each value, made when
    /// `data` is a view's data or holds a view, and `None` otherwise. The
    /// copy starts at the first entry that needs it and takes the entries
    /// before that one as they are, so a table that needs none is read
    /// once and never copied.
    fn entries(&mut self, data: &Table, is_view: bool) -> mlua::Result<Option<Table>> {
        let mut copy = if is_view {
            Some(self.like(data)?)
        } else {
            None
        };
        for (seen, pair) in data.pairs::<Value, Value>().enumerate() {
            let (key, value) = pair?;
            let plain = self.value(&value)?;
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

/// The refusal of a [`plain`] walk past its budget.
fn too_large(_: TooLarge) -> mlua::Error {
    mlua::Error::DeserializeError(TOO_LARGE_TO_CONVERT.to_owned())
}

/// An empty proxy whose metatable holds `index`, a `__pairs` that walks
/// with `step`, a `__newindex` that raises `<path> is read-only`, and that
/// text as its `__metatable`, which protects it.
pub(crate) fn read_only_walk(
    lua: &Lua,
    index: Function,
    step: Function,
    path: &str,
) -> mlua::Result<Table> {
    let label = format!("{path} is read-only");
    let pairs =
        lua.create_function(move |_, proxy: Table| Ok((step.clone(), proxy, Value::Nil)))?;
    let refusal = label.clone();
    let newindex = lua.create_function(move |_, _: MultiValue| -> mlua::Result<()> {
        Err(mlua::Error::external(refusal.clone()))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", index)?;
    meta.raw_set("__pairs", pairs)?;
    meta.raw_set("__newindex", newindex)?;
    meta.raw_set("__metatable", label)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    Ok(proxy)
}

/// The userdata under `key` in the registry table `registry`, or `None`
/// when the table is absent or the entry is no userdata.
///
/// # Errors
/// Returns an `mlua` error when the registry cannot be read.
pub(crate) fn registry_object(
    lua: &Lua,
    registry: &str,
    key: &str,
) -> mlua::Result<Option<AnyUserData>> {
    let objects: Option<Table> = lua.named_registry_value(registry)?;
    match objects
        .map(|objects| objects.raw_get::<Value>(key))
        .transpose()?
    {
        Some(Value::UserData(object)) => Ok(Some(object)),
        _ => Ok(None),
    }
}

/// Installs the `__newindex` refusal of a frozen userdata: assigning any
/// field raises `{label} are frozen: cannot assign field {key:?}`.
pub(crate) fn add_frozen_guard<T, M: UserDataMethods<T>>(methods: &mut M, label: &'static str) {
    methods.add_meta_method(
        MetaMethod::NewIndex,
        move |_, _, (key, _): (String, Value)| -> mlua::Result<()> {
            Err(mlua::Error::external(format!(
                "{label} are frozen: cannot assign field {key:?}"
            )))
        },
    );
}
