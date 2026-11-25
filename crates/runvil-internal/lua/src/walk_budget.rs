//! The work one conversion of a Lua value may do before it is refused.
//!
//! A walk that reads a shared table once per reference, or copies a
//! repeated string once per occurrence, can build a result far larger than
//! the Lua heap that holds its input, and a single Rust callback runs
//! without the instruction hook ever firing. The budget is sized from the
//! heap ceiling so that only expansion beyond the heap is refused.
//!
//! A read that tells whether a table is weak scans the `__mode` of its
//! metatable, a string of any length that many tables can share, once for
//! each table. So those bytes are text the walk reads, and [`ReadCost`]
//! brings them to the budget.

use mlua::Lua;

/// The smallest cost of one value in the Lua heap, rounded down: an array
/// slot takes at least about 9 bytes, so an unshared value that fits the
/// heap has at most `ceiling / 9` values, and `ceiling / 8` leaves room
/// above that.
const BYTES_PER_NODE: usize = 8;

/// What one sealed view costs a walk that can pass one view many times,
/// in values: a view is a table of its own, which takes 48 bytes of the
/// Lua heap on a 64-bit machine, so views that are passed once each and
/// fit the heap stay inside the budget.
const NODES_PER_VIEW: usize = 6;

/// The most Lua references one Rust walk may hold at once: an encode path's
/// string, table, function, thread, and userdata keys, the entries of
/// `json.encode`'s options table, or an offer verb's array entries, each of
/// which takes a slot on the auxiliary reference stack until it is dropped.
/// An encode path's integer, number, and boolean keys hold no slot. A Lua
/// stack caps at 1,000,000 slots and mlua panics when a reference cannot get
/// one, so 10 times headroom below that cap covers the references the path
/// itself keeps alive.
pub(crate) const HELD_KEY_LIMIT: usize = 100_000;

/// The most tables deep a value may nest, the top-level table being level
/// 1: the depth `json.decode` accepts, so a value that converts is one
/// that reads back.
pub(crate) const DEPTH_LIMIT: usize = 127;

/// The walk went past its budget.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TooLarge;

/// The VM's Lua heap limit, kept as Lua app data because mlua has no getter
/// for the memory limit.
pub(crate) struct HeapCeiling(pub usize);

/// The Lua heap ceiling a VM holds from construction until
/// [`apply_lua_limits`](crate::SectionVm::apply_lua_limits) sets the run's.
pub(crate) const DEFAULT_LUA_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// What the read of one table through its views asks the walk it is part
/// of to spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadCost {
    /// One view passed on the way to the data.
    View,
    /// This many bytes of a `__mode`, which the read looked at to tell
    /// whether a table is weak.
    Mode(usize),
}

/// What one walk has left to spend: values visited and bytes of text read.
pub(crate) struct WalkBudget {
    nodes: usize,
    bytes: usize,
}

impl WalkBudget {
    /// The budget for a walk on `lua`: `ceiling / 8` values and `ceiling`
    /// bytes of text, where the ceiling is the VM's Lua heap limit. At the
    /// default 64 MiB that is 8,388,608 values, which covers the most
    /// values an 8 MiB `json.decode` input can produce, so decoding and
    /// then encoding stays whole. A VM with no recorded ceiling gets the
    /// default, so no caller runs unbounded.
    pub(crate) fn for_vm(lua: &Lua) -> Self {
        let ceiling = lua
            .app_data_ref::<HeapCeiling>()
            .map_or(DEFAULT_LUA_MEMORY_BYTES, |ceiling| ceiling.0);
        Self {
            nodes: ceiling / BYTES_PER_NODE,
            bytes: ceiling,
        }
    }

    /// How many values are left to spend.
    pub(crate) fn values(&self) -> usize {
        self.nodes
    }

    /// How many bytes of text are left to spend.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Spends one value, failing once none are left.
    pub(crate) fn charge_node(&mut self) -> Result<(), TooLarge> {
        self.charge_nodes(1)
    }

    /// Spends `n` values, failing when fewer than `n` are left.
    pub(crate) fn charge_nodes(&mut self, n: usize) -> Result<(), TooLarge> {
        self.nodes = self.nodes.checked_sub(n).ok_or(TooLarge)?;
        Ok(())
    }

    /// Spends one view passed on the way to a table, for a walk that can
    /// pass one view many times, failing once too few values are left.
    pub(crate) fn charge_view(&mut self) -> Result<(), TooLarge> {
        self.charge_nodes(NODES_PER_VIEW)
    }

    /// Spends what the read of one table asks for: a value for a view
    /// passed, and the bytes of a `__mode` as text.
    pub(crate) fn charge_read(&mut self, cost: ReadCost) -> Result<(), TooLarge> {
        match cost {
            ReadCost::View => self.charge_node(),
            ReadCost::Mode(bytes) => self.charge_bytes(bytes),
        }
    }

    /// Spends `n` bytes of text, failing when fewer than `n` are left.
    pub(crate) fn charge_bytes(&mut self, n: usize) -> Result<(), TooLarge> {
        self.bytes = self.bytes.checked_sub(n).ok_or(TooLarge)?;
        Ok(())
    }
}
