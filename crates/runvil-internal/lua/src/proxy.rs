//! The read-only proxy every seal builds on - `sys`, the `args` table of
//! the chain's arguments and each table inside it, a sealed prelude
//! global, and a prelude's `var` view - the key
//! formatter their refusals and field paths share, and [`unseal`], which
//! reads a table through such views without calling into Lua.
//! [`view_len`] and [`view_data`] make a view measure and walk as the
//! table it stands for, and [`field_weakness`] tells whether the read of
//! one field of a table is the same in every run.
//! `tools.stats` and `tools.packs` build on the walkable [`read_only_walk`].
//! Tool and Pack objects share [`registry_object`] and [`add_frozen_guard`].

use std::collections::HashSet;
use std::fmt;

use mlua::{
    AnyUserData, Function, Lua, LuaString, MaybeSend, MetaMethod, MultiValue, Table,
    UserDataMethods, Value,
};

use crate::meta_names::MetaNames;
use crate::walk_budget::ReadCost;
use crate::weak::{ModeBudget, Spent, fallback_weakness, weakness_of};

/// Renders a table key for a refusal: a string as its text, an integer as
/// its digits, and any other key by its type name.
pub(crate) fn key_text(key: &Value) -> String {
    match key {
        Value::String(name) => name.to_string_lossy(),
        Value::Integer(index) => index.to_string(),
        other => other.type_name().to_owned(),
    }
}

/// Renders the step from a table to its field `key`, as a path shows it:
/// the [`name_step`] of a string key, and the [`index_step`] of any
/// other, its key rendered by [`key_text`].
pub(crate) fn key_step(key: &Value) -> String {
    match key {
        Value::String(name) => name_step(&name.as_bytes()),
        other => index_step(key_text(other)),
    }
}

/// Renders the step to the field `name`: `.name`, the name as [`quoted`]
/// gives it, so the step of a name of any size is small.
pub(crate) fn name_step(name: &[u8]) -> String {
    format!(".{}", quoted(name))
}

/// Renders the step to the item or the key that `shown` names: `[shown]`.
pub(crate) fn index_step(shown: impl fmt::Display) -> String {
    format!("[{shown}]")
}

/// Renders the path of the field `key` under `path`: `path` and then the
/// [`key_step`] to the field.
pub(crate) fn key_path(path: &str, key: &Value) -> String {
    format!("{path}{}", key_step(key))
}

/// `text` as a refusal quotes it: all of it up to 64 bytes, and of a
/// longer one its start, to the end of a character, and its length. Only
/// the start is copied, so the refusal of a text of any size is small.
pub(crate) fn quoted(text: &[u8]) -> String {
    /// The most bytes of one text of the author's that a refusal quotes.
    const QUOTED_BYTES: usize = 64;
    /// The most bytes that continue one character of UTF-8.
    const CONTINUATIONS: usize = 3;
    if text.len() <= QUOTED_BYTES {
        return String::from_utf8_lossy(text).into_owned();
    }
    // A byte of the form 10xxxxxx continues a character, so a character
    // starts at the last byte that has another form. Text with no such
    // byte in reach is not UTF-8 there, and is cut where it stands.
    let end = (QUOTED_BYTES - CONTINUATIONS..=QUOTED_BYTES)
        .rev()
        .find(|&at| text[at] & 0xC0 != 0x80)
        .unwrap_or(QUOTED_BYTES);
    format!(
        "{}... ({} bytes)",
        String::from_utf8_lossy(&text[..end]),
        text.len()
    )
}

/// Builds an empty proxy whose `__index` is `index`, whose `__newindex`
/// raises the text `refusal` builds from the refused key, and whose
/// `__metatable` is `label`, so `getmetatable` returns only the label.
///
/// A proxy whose `index` is a table is a view of that table: its `__len`
/// is the table's length, and `pairs`, `next`, and
/// [`data_of`](crate::data::data_of) read the table through it. A proxy
/// whose `index` is a function holds no table to measure or walk, so `#`
/// gives 0 and `pairs` sees nothing.
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
    /// when it has one. The label is the view's own string: many views can
    /// share one long label, so a caller that wants its text counts its
    /// bytes before it copies them.
    Unreadable(Option<LuaString>),
    /// A chain of views that leads back to one of its own views.
    Loop,
    /// A table the read must not look into, with what a refusal says of
    /// it, as [`weakness_of`] tells: a weak table, or a table whose
    /// metatable is one. It is the table given, a view passed on the way,
    /// or the table that holds the data.
    Weak(&'static str),
}

/// Reads `table` through sealed views, never calling into Lua.
///
/// A sealed view is a table with no entries of its own whose metatable has
/// both `__index` and `__metatable`: a table `__index` is its data, read
/// by these same rules, and a function `__index` cannot be read, labelled
/// by a string `__metatable`. Any other table is its own data. `spend`
/// runs once for each view passed, and once for each metatable the read
/// asks for a `__mode`, with the bytes scanned, as [`weakness_of`] tells.
/// So the caller can bound the work: the views already passed sit in a
/// set, and a long chain costs time in proportion to its length and to
/// the bytes of `__mode` it scans. `names` keeps the name of each field
/// the read asks a metatable for, for all the reads of one walk or of
/// one installed function, as [`MetaNames`] tells.
///
/// A weak table ends the read wherever it is met, before the read looks
/// for an entry in it. The collector takes the entries of a weak table, so
/// whether one has none, and is a view, would differ from one run to the
/// next, and so would the entries of the data. A table whose metatable is
/// a weak table ends the read too, before the read asks that metatable for
/// `__metatable` and `__index`: the collector takes those fields as it
/// takes any entry, and the table would be a view in one run and its own
/// data in the next.
///
/// # Errors
/// Returns the failure of `spend`, or of reading a metatable.
pub(crate) fn unseal(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
    mut spend: impl FnMut(ReadCost) -> mlua::Result<()>,
) -> mlua::Result<Unsealed> {
    let mut current = table.clone();
    let mut views = HashSet::new();
    loop {
        let Some(metatable) = current.metatable() else {
            return Ok(Unsealed::Data(current));
        };
        let weakness = weakness_of(lua, names, &metatable, |bytes| spend(ReadCost::Mode(bytes)))?;
        if let Some(weakness) = weakness {
            return Ok(Unsealed::Weak(weakness));
        }
        let label = names.label(lua, &metatable)?;
        if label.is_nil()
            || current
                .pairs::<Value, Value>()
                .next()
                .transpose()?
                .is_some()
        {
            return Ok(Unsealed::Data(current));
        }
        match names.index(lua, &metatable)? {
            Value::Table(data) => {
                spend(ReadCost::View)?;
                views.insert(current.to_pointer());
                if views.contains(&data.to_pointer()) {
                    return Ok(Unsealed::Loop);
                }
                current = data;
            }
            Value::Function(_) => {
                let label = match label {
                    Value::String(label) => Some(label),
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

/// The table `table` stands for, for a caller that reads one table in a
/// Rust call: the data table of a sealed view, read by [`unseal`], and
/// `table` itself when it is no view or its data cannot be read as a
/// table.
///
/// The read scans a `__mode` for each view on the way, and a chain of
/// views can share one as long as the Lua heap allows. So the bytes it
/// scans are counted against a [`ModeBudget`] of its own.
///
/// The inner `Err` holds what a refusal says of the table, and the
/// caller reads nothing: for a table [`unseal`] must not look into, and
/// for one past that budget. `names` is what [`unseal`] takes.
///
/// # Errors
/// Returns the failure of reading a metatable.
pub(crate) fn view_data(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
) -> mlua::Result<Result<Table, String>> {
    let data = view_data_counted(lua, names, table, &mut ModeBudget::new(lua))?;
    Ok(data.map(|(data, _)| data))
}

/// What a refusal says of `table`, after the words that name it, when the
/// read of `table[key]` that Lua makes, metamethods included, is one the
/// Engine must not make, or `None` when it can: the words of
/// [`view_data`] for `table` and the views it is read through, and those
/// of [`fallback_weakness`] for a table that the read looks into past the
/// data, as far as Lua follows the read. One [`ModeBudget`] counts the
/// scans of both and the tables past the data.
///
/// The caller makes the read next, with no code of the author between
/// the two.
///
/// # Errors
/// Returns the failure of reading a table, a metatable, or the registry.
pub(crate) fn field_weakness(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
    key: &Value,
) -> mlua::Result<Option<String>> {
    let mut budget = ModeBudget::new(lua);
    match view_data_counted(lua, names, table, &mut budget)? {
        Ok((data, views)) => fallback_weakness(lua, names, &data, views, key, &mut budget),
        Err(refusal) => Ok(Some(refusal)),
    }
}

/// [`view_data`] for a caller that reads many tables in one Rust call:
/// `spend` runs as it does for [`unseal`], so the caller can count the
/// views and the bytes of `__mode` of all its reads against one budget,
/// and it keeps one `names` for all of them.
/// When [`unseal`] meets a table it must not look into, the inner `Err`
/// holds what a refusal says of that table, and the caller reads nothing.
///
/// # Errors
/// Returns the failure of `spend`, or of reading a metatable.
pub(crate) fn view_data_within(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
    spend: impl FnMut(ReadCost) -> mlua::Result<()>,
) -> mlua::Result<Result<Table, &'static str>> {
    Ok(match unseal(lua, names, table, spend)? {
        Unsealed::Data(data) => Ok(data),
        Unsealed::Unreadable(_) | Unsealed::Loop => Ok(table.clone()),
        Unsealed::Weak(weakness) => Err(weakness),
    })
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

/// [`view_data`] with the bytes counted against the caller's `budget`,
/// for a caller that scans more in the same Rust call, and with the
/// number of views the read passed on the way to the data.
///
/// # Errors
/// Returns the failure of reading a metatable.
fn view_data_counted(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
    budget: &mut ModeBudget<'_>,
) -> mlua::Result<Result<(Table, usize), String>> {
    let mut views = 0_usize;
    let read = budget.counting(|spend| {
        view_data_within(lua, names, table, |cost| match cost {
            ReadCost::View => {
                views = views.saturating_add(1);
                Ok(())
            }
            ReadCost::Mode(bytes) => spend(Spent::Mode(bytes)),
        })
    })?;
    Ok(read.and_then(|data| match data {
        Ok(data) => Ok((data, views)),
        Err(weakness) => Err(weakness.to_owned()),
    }))
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};

    use super::{Unsealed, unseal};
    use crate::meta_names::MetaNames;
    use crate::walk_budget::ReadCost;
    use crate::weak::{WEAK_METATABLE, WEAK_TABLE};

    #[test]
    fn a_weak_table_ends_the_read_wherever_it_is_met() {
        // An empty table with `__index` and `__metatable` is a view. A weak
        // table is empty or not as the collector leaves it, so the read
        // refuses one before it looks for an entry: the same table is weak
        // with the entry and without. The two fields are entries of the
        // metatable, so the read refuses a table whose metatable is weak
        // before it asks for them, whether the table holds an entry or not.
        let lua = Lua::new();
        let names = MetaNames::default();
        let cases = lua
            .load(
                "local function view(of, mode)
                   return setmetatable({}, { __index = of, __metatable = false, __mode = mode })
                 end
                 local function under(table, of, mode)
                   local fields = { __index = of, __metatable = false }
                   return setmetatable(table, setmetatable(fields, { __mode = mode }))
                 end
                 local data = { x = 1 }
                 local held = view(data, 'v')
                 rawset(held, 'entry', data)
                 return {
                   view(view(data)),
                   view(data, 'v'),
                   held,
                   view(view(data, 'k')),
                   view(setmetatable({ x = 1 }, { __mode = 'kv' })),
                   view(function() end, 'v'),
                   view(data, 'x'),
                   under({}, data, 'v'),
                   under({ x = 1 }, data, 'k'),
                   view(under({}, data, 'kv')),
                   under({}, data, 'x'),
                 }",
            )
            .eval::<Vec<Table>>()
            .expect("the tables build");
        let read = cases
            .iter()
            .map(
                |table| match unseal(&lua, &names, table, |_| Ok(())).expect("the table reads") {
                    Unsealed::Data(data) => match data.raw_get::<Option<i64>>("x") {
                        Ok(Some(1)) => "the data",
                        _ => "another table",
                    },
                    Unsealed::Unreadable(_) => "unreadable",
                    Unsealed::Loop => "a loop",
                    Unsealed::Weak(WEAK_TABLE) => "weak",
                    Unsealed::Weak(WEAK_METATABLE) => "a weak metatable",
                    Unsealed::Weak(_) => "weak in another way",
                },
            )
            .collect::<Vec<_>>();
        assert_eq!(
            read,
            [
                "the data",
                "weak",
                "weak",
                "weak",
                "weak",
                "weak",
                "the data",
                "a weak metatable",
                "a weak metatable",
                "a weak metatable",
                "the data",
            ]
        );
    }

    #[test]
    fn a_read_spends_each_view_it_passes_and_the_bytes_of_each_mode_it_scans() {
        // Lua reads a `__mode` up to its first zero byte, and the answer
        // is known at the first `k` or `v`: the scan ends there, with that
        // byte. A metatable with no `__mode` costs no byte, and the
        // metatable above it is asked only when the first makes no table
        // weak.
        let lua = Lua::new();
        let names = MetaNames::default();
        let spent = |source: &str| {
            let table = lua.load(source).eval::<Table>().expect("the table builds");
            let mut spent = Vec::new();
            unseal(&lua, &names, &table, |cost| {
                spent.push(cost);
                Ok(())
            })
            .expect("the table reads");
            spent
        };
        assert_eq!(spent("return {}"), []);
        assert_eq!(
            spent("return setmetatable({}, { __mode = 'xxk' .. string.rep('x', 100) })"),
            [ReadCost::Mode(3)]
        );
        assert_eq!(
            spent("return setmetatable({}, { __mode = 'x\\0k' })"),
            [ReadCost::Mode(2)]
        );
        assert_eq!(
            spent(
                "local above = { __mode = string.rep('y', 7) .. 'v' }
                 return setmetatable({}, setmetatable({ __mode = string.rep('x', 100) }, above))"
            ),
            [ReadCost::Mode(100), ReadCost::Mode(8)]
        );
        assert_eq!(
            spent(
                "local data = setmetatable({ x = 1 }, { __mode = 'xx' })
                 return setmetatable({}, { __index = data, __metatable = false })"
            ),
            [ReadCost::Mode(0), ReadCost::View, ReadCost::Mode(2)]
        );
    }
}
