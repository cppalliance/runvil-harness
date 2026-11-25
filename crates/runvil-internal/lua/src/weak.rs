//! The tables a read refuses because they are weak.
//!
//! Lua's collector takes an entry out of a weak table once nothing else
//! uses the entry's key or value, and when it runs follows no input of
//! the run. So what a read finds in a weak table, or in a table whose
//! metatable is one, differs from one run to the next, and each read that
//! must give the same result in two runs refuses both. [`is_weak`] and
//! [`weakness_of`] tell such a table, [`refuse_weak`] refuses one, and
//! the constants are the words a refusal says of it. [`ModeBudget`]
//! counts the bytes of `__mode` that one call scans, and gives the words
//! for a table that is refused for their number.

use mlua::{Lua, Table, Value};

use crate::meta_names::MetaNames;
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

/// The bytes of `__mode` that one Rust call may scan: as many as the
/// byte budget of the VM, which is its memory limit.
///
/// The budget is made at the first `__mode` that has bytes, so a call
/// that scans none pays for no budget.
pub(crate) struct ModeBudget<'lua> {
    lua: &'lua Lua,
    left: Option<WalkBudget>,
}

impl<'lua> ModeBudget<'lua> {
    /// The budget of one call in `lua`.
    pub(crate) fn new(lua: &'lua Lua) -> Self {
        Self { lua, left: None }
    }

    /// Counts a scan of `bytes`, as [`weakness_of`] reports one.
    ///
    /// # Errors
    /// Past the budget, returns what a refusal says of the table, after
    /// the words that name the table: the bytes the call came to with
    /// this scan, and the most that one call scans.
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), String> {
        if bytes == 0 {
            return Ok(());
        }
        let left = self
            .left
            .get_or_insert_with(|| WalkBudget::for_vm(self.lua));
        left.charge_bytes(bytes).map_err(|TooLarge| {
            let limit = WalkBudget::for_vm(self.lua).bytes();
            let scanned = limit.saturating_sub(left.bytes()).saturating_add(bytes);
            format!(
                "a table with {scanned} bytes of __mode to scan: one call scans at most {limit} \
                 bytes"
            )
        })
    }
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
