//! The standard-library functions a section VM replaces.
//!
//! A run with the same seed, start time, and answers must produce the same
//! result. Two stock functions read something no run input fixes:
//! `math.randomseed` with no argument and `table.sort` read the clock and a
//! memory address. [`install`] replaces each with a function whose result
//! depends only on its arguments and on the VM's generator, which starts
//! from a fixed seed until
//! [`SectionVm::seed_random`](crate::SectionVm::seed_random) gives it the
//! run's.
//!
//! The Lua side lives in `__impl_stdlib.lua`.

use std::sync::LazyLock;

use mlua::Lua;

use crate::error::{self, Error, Result};
use crate::program::LuaProgram;

/// Replaces `math.randomseed` and `table.sort` on a fresh VM, and seeds its
/// generator with a fixed value.
///
/// Must run after the `_G` guard is installed and before any other chunk
/// captures a function this one replaces.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the chunk cannot load or run.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    STDLIB_PROGRAM
        .as_ref()
        .map_err(Error::clone)?
        .load(lua)?
        .call::<()>(())
        .map_err(error::make_lua_error)
}

/// The replacement chunk's name.
const STDLIB_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_stdlib.lua";

/// The replacement source.
const STDLIB_SOURCE: &str = include_str!("__impl_stdlib.lua");

/// The replacement program.
static STDLIB_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(STDLIB_SOURCE, STDLIB_CHUNK_NAME));

#[cfg(test)]
mod tests {
    use super::{STDLIB_CHUNK_NAME, STDLIB_SOURCE};
    use crate::tests::assert_chunk_name_resolves;

    #[test]
    fn the_chunk_name_resolves_to_the_embedded_file() {
        assert_chunk_name_resolves("STDLIB_CHUNK_NAME", STDLIB_CHUNK_NAME, STDLIB_SOURCE);
    }
}
