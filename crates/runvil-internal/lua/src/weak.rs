//! The tables a read refuses because they are weak.
//!
//! Lua's collector takes an entry out of a weak table once nothing else
//! uses the entry's key or value, and when it runs follows no input of
//! the run. So what a read finds in a weak table, or in a table whose
//! metatable is one, differs from one run to the next, and each read that
//! must give the same result in two runs refuses both. [`is_weak`] and
//! [`weakness_of`] tell such a table, [`refuse_weak`] refuses one, and
//! the constants are the words a refusal says of it. [`ModeBudget`]
//! counts the bytes of `__mode` that one call scans and the tables that
//! it follows its reads through, and gives the words for a table that is
//! refused for the number of either.
//!
//! A read of one field by its key looks into more tables than the one it
//! is written for: Lua asks the `__index` of the metatable for a field
//! that the table lacks, and that of the next table for one that it lacks
//! too. [`fallback_weakness`] and [`index_weakness`] tell a weak table on
//! that way.

use mlua::{Lua, Table, Value};

use crate::meta_names::MetaNames;
use crate::stdlib::strings_metatable;
use crate::walk_budget::{TooLarge, WalkBudget};

/// What a refusal says of a weak table, after the words that name the
/// table.
pub(crate) const WEAK_TABLE: &str = "a weak table, one whose metatable sets __mode: the entries the \
                                     collector leaves in it are not the same in two runs";

/// Whether `table` is weak: its metatable has a `__mode` that is a string
/// with `k` or `v` in it before its first zero byte, which is how Lua reads
/// the field. Lua reads it each time it collects, so a table can become
/// weak after it has its metatable.
///
/// A walk that keeps no reference to each entry refuses a weak table.
/// Lua's collector takes an entry out of one once nothing else uses the
/// entry's key or value, whichever the mode names. Such a walk writes to
/// Lua's heap as it reads, and the collector can run at each write: it
/// then takes the entries the walk has not read yet, and keeps the one
/// Rust holds. Which entries those are follows Lua's own order, which
/// differs from one VM to the next, so what the walk found would too.
///
/// # Errors
/// Returns the `mlua` error of the metatable read.
pub(crate) fn is_weak(table: &Table) -> mlua::Result<bool> {
    match table.metatable() {
        Some(metatable) => Ok(scan(&metatable.raw_get::<Value>("__mode")?).0),
        None => Ok(false),
    }
}

/// What a refusal says of a table whose metatable is a weak table, after
/// the words that name the table.
pub(crate) const WEAK_METATABLE: &str = "a table whose metatable is a weak table: the fields the \
                                         collector leaves in that metatable are not the same in \
                                         two runs";

/// Why a read must look at neither the entries of a table with the
/// metatable `metatable` nor the fields of `metatable`, as a refusal
/// words it, or `None` when it can read both: [`WEAK_TABLE`] for a
/// metatable that makes its table weak, and [`WEAK_METATABLE`] for one
/// that is weak itself.
///
/// The fields of a metatable are its entries, so the collector takes them
/// out of a weak one: an `__index` or a `__len` is there in one run and
/// gone in the next. A read that asks a metatable what its table is, a
/// view or a list with a length of its own, asks this first.
///
/// The answer is the same in every run, whatever else is weak: it comes
/// from a `__mode`, which counts only when it is a string, and the
/// collector takes no string out of a weak table.
///
/// `scanned` runs once for each metatable the answer asks for a
/// `__mode`, with the bytes [`scan`] looked at. A caller that asks
/// about many tables in one Rust call, or about one table many times,
/// counts them against a budget: a `__mode` can be as long as the Lua
/// heap allows, and every table can share one. Such a caller asks every
/// metatable by the one name `names` keeps, where [`is_weak`] makes the
/// name anew for its one table.
///
/// # Errors
/// Returns the failure of `scanned`, or the `mlua` error of a metatable
/// read.
pub(crate) fn weakness_of(
    lua: &Lua,
    names: &MetaNames,
    metatable: &Table,
    mut scanned: impl FnMut(usize) -> mlua::Result<()>,
) -> mlua::Result<Option<&'static str>> {
    let (weak, bytes) = scan(&names.mode(lua, metatable)?);
    scanned(bytes)?;
    if weak {
        return Ok(Some(WEAK_TABLE));
    }
    let Some(above) = metatable.metatable() else {
        return Ok(None);
    };
    let (weak, bytes) = scan(&names.mode(lua, &above)?);
    scanned(bytes)?;
    Ok(weak.then_some(WEAK_METATABLE))
}

/// What a refusal says of `table`, after the words that name it, when the
/// read of `table[key]` looks into a table past `table` that it must not
/// look into, or `None` when it meets none: the way to that table, and
/// what [`weakness_of`] says of it. Past `budget`, the words are those
/// `budget` gives.
///
/// The read is the one Lua makes for `table[key]` (`luaV_finishget` in
/// `lvm.c`), followed here with no call into Lua. A table that holds the
/// field ends it. A table that lacks the field hands the read to the
/// `__index` of its metatable: a table is asked next, a function is
/// called, and with no `__index` the read ends with nil. A string hands
/// it to the `__index` of the one metatable all strings share. Any other
/// value is a userdata, whose `__index` is the Engine's own, or a value
/// that Lua refuses to index.
///
/// The caller has asked [`weakness_of`] about `table` itself, or has read
/// it through its views, and makes the read next, with no code of the
/// author between the two. Each table past it is asked before the read
/// looks for the field in it, so the way up to a refusal is through
/// tables that the collector takes nothing out of, and it is the same in
/// every run. A function at its end is the author's code, and what that
/// reads is the author's own read.
///
/// Lua gives up the read [`INDEX_CHAIN`] values past the table it is
/// written for. `passed` is how many of those the read asked before
/// `table`, the views that `table` is the data of, so the way is at most
/// the rest of them past `table`.
///
/// Each table on the way costs `budget` what [`Spent::Table`] does. A
/// caller that reads many fields in one Rust call asks this for each, and
/// every read can go the same way, so it counts them all against one
/// budget.
///
/// # Errors
/// Returns the `mlua` error of a read of a table, of a metatable, or of
/// the registry.
pub(crate) fn fallback_weakness(
    lua: &Lua,
    names: &MetaNames,
    table: &Table,
    passed: usize,
    key: &Value,
    budget: &mut ModeBudget<'_>,
) -> mlua::Result<Option<String>> {
    if !table.raw_get::<Value>(key)?.is_nil() {
        return Ok(None);
    }
    let Some(metatable) = table.metatable() else {
        return Ok(None);
    };
    let index = names.index(lua, &metatable)?;
    let values = INDEX_CHAIN.saturating_sub(passed);
    leads_to(lua, names, &index, values, key, budget)
}

/// [`fallback_weakness`] for a caller that reads `index[key]` in place of
/// an `__index` that is no function, as the guard of the globals does for
/// the metatable the author gave them: `index` is asked first, and the
/// way is the whole of that read, `index` and [`INDEX_CHAIN`] values past
/// it.
///
/// # Errors
/// Returns the `mlua` error of a read of a table, of a metatable, or of
/// the registry.
pub(crate) fn index_weakness(
    lua: &Lua,
    names: &MetaNames,
    index: &Value,
    key: &Value,
    budget: &mut ModeBudget<'_>,
) -> mlua::Result<Option<String>> {
    leads_to(lua, names, index, INDEX_CHAIN + 1, key, budget)
}

/// What one Rust call may spend to tell the weak tables that its reads
/// would look into, as much as one walk of a value on the VM: as many
/// bytes of `__mode` scanned as the byte budget, which is the memory
/// limit, and as many tables followed on the way of a read by key as the
/// budget has values.
///
/// No instruction hook interrupts one Rust call, and its work can be far
/// more than the Lua heap holds: many tables can share one `__mode`, and
/// a call that reads many fields follows the way of each anew, through
/// tables that all of them can share.
///
/// The budget is made at the first table followed or `__mode` that has
/// bytes, so a call that meets neither pays for no budget.
pub(crate) struct ModeBudget<'lua> {
    lua: &'lua Lua,
    left: Option<WalkBudget>,
}

impl<'lua> ModeBudget<'lua> {
    /// The budget of one call in `lua`.
    pub(crate) fn new(lua: &'lua Lua) -> Self {
        Self { lua, left: None }
    }

    /// Counts what a read spent.
    ///
    /// # Errors
    /// Past the budget, returns what a refusal says of the table, after
    /// the words that name the table: for a scan, the bytes the call
    /// came to with it and the most that one call scans, and for a table
    /// followed, the most of them that one call follows.
    pub(crate) fn spend(&mut self, spent: Spent) -> Result<(), String> {
        if spent == Spent::Mode(0) {
            return Ok(());
        }
        let left = self
            .left
            .get_or_insert_with(|| WalkBudget::for_vm(self.lua));
        match spent {
            Spent::Table => left.charge_node().map_err(|TooLarge| {
                let limit = WalkBudget::for_vm(self.lua).values();
                format!(
                    "a table whose __index chain is followed through more than {limit} tables: \
                     one call follows at most {limit}"
                )
            }),
            Spent::Mode(bytes) => left.charge_bytes(bytes).map_err(|TooLarge| {
                let limit = WalkBudget::for_vm(self.lua).bytes();
                let scanned = limit.saturating_sub(left.bytes()).saturating_add(bytes);
                format!(
                    "a table with {scanned} bytes of __mode to scan: one call scans at most \
                     {limit} bytes"
                )
            }),
        }
    }

    /// Runs `read`, which reports what it spends to the function it is
    /// given, and counts that. Past the budget, the inner `Err` holds the
    /// words of [`spend`](Self::spend), and `read` has ended there.
    ///
    /// # Errors
    /// Returns the failure of `read` that is nothing spent past the
    /// budget.
    pub(crate) fn counting<T>(
        &mut self,
        read: impl FnOnce(&mut dyn FnMut(Spent) -> mlua::Result<()>) -> mlua::Result<T>,
    ) -> mlua::Result<Result<T, String>> {
        let mut past_budget = None;
        let read = read(&mut |spent| {
            self.spend(spent).map_err(|refusal| {
                past_budget = Some(refusal.clone());
                mlua::Error::external(refusal)
            })
        });
        match past_budget {
            Some(refusal) => Ok(Err(refusal)),
            None => read.map(Ok),
        }
    }
}

/// What a read spends of the [`ModeBudget`] of its call.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spent {
    /// One table that the read of a field goes on to, past the table it
    /// is written for.
    Table,
    /// This many bytes of a `__mode`, which the read looked at to tell
    /// whether a table is weak, as [`weakness_of`] reports them.
    Mode(usize),
}

/// Refuses a weak `table`, which the words `what` name, as in
/// `tools.offer takes an array, got`.
///
/// # Errors
/// Returns that refusal, or the `mlua` error of the metatable read.
pub(crate) fn refuse_weak(table: &Table, what: &str) -> mlua::Result<()> {
    if is_weak(table)? {
        return Err(mlua::Error::external(format!("{what} {WEAK_TABLE}")));
    }
    Ok(())
}

/// What a refusal says of a string that a read asks for a field, after the
/// words that name the way to it, when the metatable of strings is a weak
/// table.
const WEAK_STRINGS: &str = "a string, whose metatable is a weak table: the fields the collector \
                            leaves in that metatable are not the same in two runs";

/// How many times Lua goes on from one value to its `__index` in the read
/// of one field, before it refuses the read with `'__index' chain too long;
/// possible loop`: `MAXTAGLOOP` in Lua's `lvm.c`.
const INDEX_CHAIN: usize = 2000;

/// What a refusal says of a table whose `__index` is `index`, when the
/// read of `index[key]` looks into a table that it must not look into
/// among the first `values` it asks, with what the read spends counted
/// against `budget`.
fn leads_to(
    lua: &Lua,
    names: &MetaNames,
    index: &Value,
    values: usize,
    key: &Value,
    budget: &mut ModeBudget<'_>,
) -> mlua::Result<Option<String>> {
    let met = budget.counting(|spend| weak_behind(lua, names, index, values, key, spend))?;
    Ok(match met {
        Ok(weakness) => {
            weakness.map(|weakness| format!("a table whose __index leads to {weakness}"))
        }
        Err(refusal) => Some(refusal),
    })
}

/// What [`weakness_of`] says of the first table that the read of
/// `index[key]` must not look into, among the first `values` values it
/// asks, with each table it goes on to and each scan of a `__mode`
/// reported to `spend`.
fn weak_behind(
    lua: &Lua,
    names: &MetaNames,
    index: &Value,
    values: usize,
    key: &Value,
    mut spend: impl FnMut(Spent) -> mlua::Result<()>,
) -> mlua::Result<Option<&'static str>> {
    let mut asked = index.clone();
    for _ in 0..values {
        asked = match &asked {
            Value::Table(table) => {
                spend(Spent::Table)?;
                let metatable = table.metatable();
                let scanned = |bytes| spend(Spent::Mode(bytes));
                if let Some(metatable) = &metatable
                    && let Some(weakness) = weakness_of(lua, names, metatable, scanned)?
                {
                    return Ok(Some(weakness));
                }
                if !table.raw_get::<Value>(key)?.is_nil() {
                    return Ok(None);
                }
                match metatable {
                    Some(metatable) => names.index(lua, &metatable)?,
                    None => return Ok(None),
                }
            }
            Value::String(_) => {
                let Some(strings) = strings_metatable(lua)? else {
                    return Ok(None);
                };
                if let Some(above) = strings.metatable() {
                    let (weak, bytes) = scan(&names.mode(lua, &above)?);
                    spend(Spent::Mode(bytes))?;
                    if weak {
                        return Ok(Some(WEAK_STRINGS));
                    }
                }
                names.index(lua, &strings)?
            }
            _ => return Ok(None),
        };
    }
    Ok(None)
}

/// Whether a table is weak whose metatable has the `__mode` field `mode`,
/// as [`is_weak`] tells, and how many bytes of a string `mode` the answer
/// took: those up to the first `k`, `v`, or zero byte, with that byte, or
/// all of them.
fn scan(mode: &Value) -> (bool, usize) {
    let Value::String(mode) = mode else {
        return (false, 0);
    };
    let bytes = mode.as_bytes();
    let end = bytes
        .iter()
        .position(|byte| *byte == 0 || *byte == b'k' || *byte == b'v');
    match end {
        Some(end) => (bytes[end] != 0, end + 1),
        None => (false, bytes.len()),
    }
}
