//! The names of the metatable fields a read asks for, as strings of the
//! VM.
//!
//! `mlua` puts a name given as Rust text on Lua's stack under a protected
//! call when the VM has a memory limit, and a string the VM already holds
//! with none. A read through views asks every table that has a metatable
//! for the same three fields, so [`MetaNames`] makes each name once, at
//! the first metatable asked for it: a walk keeps one for all its tables,
//! and a function installed in a VM keeps one for all its calls.

use std::cell::OnceCell;

use mlua::{Lua, LuaString, Table, Value};

/// The names one walk, or one installed function, reads metatables by.
/// Every table it reads is of one VM, the `lua` each read is given.
#[derive(Default)]
pub(crate) struct MetaNames {
    mode: OnceCell<LuaString>,
    label: OnceCell<LuaString>,
    index: OnceCell<LuaString>,
}

impl MetaNames {
    /// The raw `__mode` of `metatable`.
    ///
    /// # Errors
    /// Returns the `mlua` error of making the name or of the read.
    pub(crate) fn mode(&self, lua: &Lua, metatable: &Table) -> mlua::Result<Value> {
        field(lua, metatable, &self.mode, "__mode")
    }

    /// The raw `__metatable` of `metatable`.
    ///
    /// # Errors
    /// Returns the `mlua` error of making the name or of the read.
    pub(crate) fn label(&self, lua: &Lua, metatable: &Table) -> mlua::Result<Value> {
        field(lua, metatable, &self.label, "__metatable")
    }

    /// The raw `__index` of `metatable`.
    ///
    /// # Errors
    /// Returns the `mlua` error of making the name or of the read.
    pub(crate) fn index(&self, lua: &Lua, metatable: &Table) -> mlua::Result<Value> {
        field(lua, metatable, &self.index, "__index")
    }
}

/// The raw field `name` of `metatable`, read by the string in `held`,
/// which is made of `name` when it is not there yet.
fn field(
    lua: &Lua,
    metatable: &Table,
    held: &OnceCell<LuaString>,
    name: &str,
) -> mlua::Result<Value> {
    let name = if let Some(name) = held.get() {
        name
    } else {
        let made = lua.create_string(name)?;
        held.get_or_init(|| made)
    };
    metatable.raw_get(name)
}
