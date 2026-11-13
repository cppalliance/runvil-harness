//! Compiled Lua chunks: bytecode compilation with debug info and chunk-line to prompt-line mapping.

use std::ffi::c_void;
use std::num::NonZeroU32;

use mlua::{Function, LightUserData, Lua, LuaOptions, StdLib, Value};
use runvil_types::error::ErrorCode;

use crate::chunk_line::{PROMPT_CHUNK_PREFIX, compile_line, map_chunk_line_to_absolute};
use crate::error::{
    self, Error, Result, authored_at, failure_text, make_lua_compile_error, make_lua_runtime_error,
    root_text,
};

/// The statement every prompt block compiles behind, on its first line so
/// no line moves. Lua compiles no tail call inside a to-be-closed
/// variable's scope, so a main chunk's `return f()` keeps the author's
/// frame on the stack for the call line to be read from; a nil
/// to-be-closed value is never closed. The scope covers the `do` block
/// [`RAN_OFF_OPENER`] opens, so the author's `return f()` inside it is no
/// tail call either.
const FRAME_GUARD: &str = "local __runvil_site <close> = nil; ";

/// What a prompt block's source compiles behind, after [`FRAME_GUARD`] and
/// on the same first line: the `do` that [`RAN_OFF_TRAILER`] closes. Lua
/// takes a `return` only as the last statement of its block, so the
/// trailer can follow a source that ends in one only from outside a block
/// of the source's own.
const RAN_OFF_OPENER: &str = "do ";

/// What a prompt block's source compiles in front of, on a line of its own
/// because a source can end in a line comment: the end of the `do` block
/// and a return of the chunk's arguments. The Engine calls a block with
/// the [`ran_off_marker`], so a block that runs off its end returns the
/// marker, and a block that executes a `return` of its own never reaches
/// this one.
const RAN_OFF_TRAILER: &str = "\nend return ...";

/// The byte whose address the [`ran_off_marker`] holds. Nothing reads it.
static RAN_OFF: u8 = 0;

/// The value the Engine passes to every prompt block as its one argument,
/// which the block returns only by running off its end: a light userdata
/// that holds the address of a private static, so no value a prompt can
/// build equals it.
pub(crate) fn ran_off_marker() -> Value {
    Value::LightUserData(LightUserData(
        std::ptr::from_ref(&RAN_OFF).cast_mut().cast::<c_void>(),
    ))
}

/// Whether `value` is the [`ran_off_marker`].
pub(crate) fn is_ran_off(value: &Value) -> bool {
    matches!(
        value,
        Value::LightUserData(pointer)
            if std::ptr::eq(pointer.0.cast_const().cast::<u8>(), &raw const RAN_OFF)
    )
}

/// The fixed temporary VM a chunk compiles in.
fn compiler() -> mlua::Result<Lua> {
    Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )
}

/// Compiles a named chunk in `lua` and keeps debug info.
fn compile_chunk(lua: &Lua, source: &str, chunk_name: &str) -> mlua::Result<Vec<u8>> {
    let function = lua.load(source).set_name(chunk_name).into_function()?;
    // Keep debug info so runtime errors report the chunk name and line
    // (`dump(true)` strips them and leaves `?:` in the traceback).
    Ok(function.dump(false))
}

/// Compiles a prompt block's `source` in `lua` under `chunk_name`, twice.
///
/// The first compile takes the source as written, behind [`FRAME_GUARD`]
/// alone, and its bytecode is dropped: it checks that the source is a
/// whole chunk, so its failure is the block's diagnostic, and it refuses a
/// source such as `end do`, which compiles only between
/// [`RAN_OFF_OPENER`] and [`RAN_OFF_TRAILER`]. The second compile takes
/// the source between the two, and its bytecode is the block's. Neither
/// addition holds a line break before the source, so a line of the block
/// is the same chunk line in both.
fn compile_block(lua: &Lua, source: &str, chunk_name: &str) -> mlua::Result<Vec<u8>> {
    lua.load(format!("{FRAME_GUARD}{source}"))
        .set_name(chunk_name)
        .into_function()?;
    compile_chunk(
        lua,
        &format!("{FRAME_GUARD}{RAN_OFF_OPENER}{source}{RAN_OFF_TRAILER}"),
        chunk_name,
    )
}

/// Compiled Lua 5.5 source that can be loaded into multiple process-local VMs.
///
/// A program retains its original source for diagnostics and stores bytecode
/// produced once by Lua 5.5. The bytecode is an in-memory implementation detail:
/// it is not a stable or portable serialization format and must not be persisted.
///
/// Compilation does not execute the source. Loading a program ([`load`](Self::load))
/// creates a function in the supplied VM but likewise does not call it.
///
/// # Sensitivity
/// A program retains the author's original prompt Lua source verbatim, and
/// [`source`](Self::source) exposes it. Prompt source can embed
/// author-sensitive material (system instructions, embedded credentials in a
/// poorly written prompt, private policy text), so treat the value returned by
/// [`source`](Self::source) as sensitive: it is a full-fidelity diagnostic
/// accessor, not a value to log at info level, echo to untrusted sinks, or place
/// in a model-facing message. [`location`](Self::location) and
/// [`source_line`](Self::source_line) are safe positional metadata (a display
/// name and a line number) and hold no source text. The crate itself never logs the
/// retained source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaProgram {
    source: String,
    bytecode: Vec<u8>,
    /// Name the chunk was compiled under, which Lua renders in its errors as
    /// `[string "{chunk_name}"]`: `line {source_line}` for a prompt block,
    /// the explicit name for an internal chunk. Mapping finds a chunk's
    /// lines by this name, so it must be unique among the programs one VM
    /// maps through and stay under the 45 bytes Lua keeps whole.
    chunk_name: String,
    /// Parser location string that mapped runtime errors name (for example
    /// ``section `Web Search` epilog``).
    location: String,
    /// 1-based line number in the prompt source where this Lua region begins.
    ///
    /// A [`NonZeroU32`] so line zero is unrepresentable. Used together with
    /// chunk-relative line numbers from Lua runtime errors to produce an
    /// absolute prompt-source line: `source_line + chunk_line - 1`.
    source_line: NonZeroU32,
}

impl LuaProgram {
    /// Compiles `source` as Lua 5.5 bytecode without executing it.
    ///
    /// `location` identifies the source region in diagnostics, and `file`
    /// is the prompt file the region is in. The chunk
    /// itself compiles under the name `line {source_line}`, which Lua
    /// renders as `[string "line 120"]`: no two blocks of one prompt start
    /// on the same line, and the name never reaches the length at which Lua
    /// cuts it. The chunk compiles behind a statement on its first line
    /// that keeps a main-chunk tail call from dropping the author's frame,
    /// and inside a `do` block followed by a return of the chunk's
    /// arguments, so a block called with the Engine's marker returns the
    /// marker when it runs off its end and its own values when it executes
    /// a `return`. The source is checked as written first, so a failure
    /// reads as it does for the source alone, and no line of the block
    /// moves. [`source`](Self::source) is the source as given.
    ///
    /// # Errors
    /// Returns a `LuaCompile` failure when `source` is not syntactically
    /// valid, or compiles alone and exceeds one of Lua's limits inside the
    /// `do` block: its message names the region and holds the Lua
    /// diagnostic, its `Section` fact names the region, it is located on
    /// the line of `file` the diagnostic names, and its author prose names
    /// what failed.
    /// Returns an `Internal` failure located at this crate's Rust source,
    /// whose message holds the `mlua` error's text, if the temporary
    /// compiler VM cannot be created.
    pub fn compile(
        source: &str,
        file: &str,
        location: &str,
        source_line: NonZeroU32,
    ) -> Result<Self> {
        let chunk_name = format!("{PROMPT_CHUNK_PREFIX}{source_line}");
        let bytecode = compiler()
            .map_err(|error| error::make_internal_error(error.to_string()))
            .and_then(|lua| {
                compile_block(&lua, source, &chunk_name).map_err(|error| {
                    let diagnostic = match &error {
                        mlua::Error::SyntaxError { message, .. } => message.clone(),
                        other => other.to_string(),
                    };
                    let line = compile_line(&chunk_name, source_line, &diagnostic);
                    authored_at(
                        make_lua_compile_error(location, source_line.get(), &error.to_string()),
                        file,
                        Some(line),
                        failure_text(&diagnostic),
                    )
                })
            })?;
        Ok(Self {
            source: source.to_owned(),
            bytecode,
            chunk_name,
            location: location.to_owned(),
            source_line,
        })
    }

    /// Compiles one of the crate's embedded Lua chunks.
    ///
    /// Internal chunks are crate source, not author prompt source.
    /// `chunk_name` serves as both the chunk name and the display
    /// location; an `@`-prefixed name renders as a file path in errors,
    /// which the line mapper never rewrites. Every embedded chunk is
    /// included with `include_str!`, so chunk line 1 is file line 1, and
    /// compiles once in a `LazyLock`; a compile failure is a crate bug
    /// that each install clones.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the temporary compiler VM cannot be
    /// created or the source fails to compile.
    pub fn compile_internal(source: &str, chunk_name: &str) -> Result<Self> {
        let bytecode = compiler()
            .and_then(|lua| compile_chunk(&lua, source, chunk_name))
            .map_err(error::make_lua_error)?;
        Ok(Self {
            source: source.to_owned(),
            bytecode,
            chunk_name: chunk_name.to_owned(),
            location: chunk_name.to_owned(),
            source_line: NonZeroU32::MIN,
        })
    }

    /// Returns the original Lua source retained for diagnostics.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the 1-based prompt-source line where this Lua region begins.
    #[must_use]
    pub fn source_line(&self) -> NonZeroU32 {
        self.source_line
    }

    /// Loads the compiled function into `lua` without executing it.
    ///
    /// The bytecode is loaded only into a VM in the same process and is never
    /// exposed as a persistence format. A prompt block's function returns
    /// its arguments when it runs off its end, so a caller that needs to
    /// tell that from a `return` calls it with the Engine's marker, as
    /// [`SectionVm::start_block_coro`](crate::SectionVm::start_block_coro)
    /// does.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the VM rejects the internally
    /// compiled bytecode.
    pub fn load(&self, lua: &Lua) -> Result<Function> {
        lua.load(self.bytecode.as_slice())
            .into_function()
            .map_err(error::make_lua_error)
    }

    /// Loads the compiled function into `lua` with `env` as its
    /// environment: the table every global the chunk names is read from
    /// and written to, in the chunk and in every function it defines.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the VM rejects the internally
    /// compiled bytecode.
    pub(crate) fn load_in(&self, lua: &Lua, env: mlua::Table) -> Result<Function> {
        lua.load(self.bytecode.as_slice())
            .set_environment(env)
            .into_function()
            .map_err(error::make_lua_error)
    }

    /// Maps a Lua runtime failure to its ordered core outcome, for a chunk
    /// that ran beside `loaded`, the programs of the libraries loaded into
    /// its VM, from the prompt file `file`.
    ///
    /// A typed internal or substitution failure that already has a
    /// location returns as it was raised, so an internal failure keeps its
    /// Rust line. A typed context exhaustion, such as an uncaught direct
    /// `compactors.fail` call's, and an Engine quota refusal, recognized by
    /// its typed value, keep their own failure, and every other failure,
    /// other typed failures included, returns a `LuaRuntime` failure with
    /// each chunk-relative line rewritten to an absolute prompt-source line
    /// by the program that owns that chunk, this one first when two share a
    /// chunk name. Nested errors from other chunks (for example a fanout
    /// arm) are left unchanged. Each is placed on the first line the
    /// mapping rewrote, of `file`, when it has no location yet, and the
    /// quota and the runtime failure get the author prose of the failure's
    /// own text. Cancellation is the VM's to classify: a chunk the
    /// instruction hook aborted under the run's cancel flag is a
    /// `Canceled` failure before the raw error reaches here.
    pub(crate) fn map_runtime_error_among(
        &self,
        error: &mlua::Error,
        loaded: &[Self],
        file: &str,
    ) -> Error {
        let typed = error.downcast_ref::<Error>();
        if let Some(located) = typed.filter(|typed| {
            matches!(typed.code, ErrorCode::Internal | ErrorCode::Substitution)
                && typed.at.is_some()
        }) {
            return located.clone();
        }
        let raw = error.to_string();
        let owners: Vec<(&str, &str, NonZeroU32)> = std::iter::once(self)
            .chain(loaded)
            .map(|program| {
                (
                    program.chunk_name.as_str(),
                    program.location(),
                    program.source_line,
                )
            })
            .collect();
        let (mapped, line) = map_chunk_line_to_absolute(&raw, &owners);
        let kept = typed.filter(|typed| {
            matches!(
                typed.code,
                ErrorCode::ContextExhausted | ErrorCode::LuaQuota
            )
        });
        let (failure, what) = match kept {
            Some(kept) => (kept.clone(), kept.message.clone()),
            None => (make_lua_runtime_error(mapped), root_text(error)),
        };
        authored_at(failure, file, line, failure_text(&what))
    }

    /// Display location recorded at compile time (parser location string),
    /// the name mapped runtime errors give this chunk.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the compile path's location, the runtime mapping's location
    //! and author prose, a quota refusal's among them, the typed failures the
    //! mapping keeps, the prompt line a block's chunk position names, and
    //! what a block called with the ran-off marker returns: the marker when
    //! it runs off its end, and its own values when it executes a `return`.

    use std::num::NonZeroU32;

    use mlua::{Lua, MultiValue};
    use runvil_types::error::{ErrorCode, ErrorFact, SourceLoc};
    use serde_json::Value;

    use super::{FRAME_GUARD, LuaProgram, is_ran_off, ran_off_marker};
    use crate::chunk_line::{
        compile_line, first_prompt_line, map_chunk_line_to_absolute, prompt_line,
    };
    use crate::error::{
        QuotaResource, make_context_exhausted_error, make_internal_error, make_lua_quota_error,
    };

    /// The file every program here comes from.
    const FILE: &str = "agents/a.md";

    /// The region every program here is.
    const REGION: &str = "section `S` prologue";

    /// The line `n`.
    fn line(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("tests pass non-zero lines")
    }

    /// `source` compiled as the block of [`REGION`] in [`FILE`] that starts on
    /// `start`.
    fn compiled(source: &str, start: u32) -> LuaProgram {
        LuaProgram::compile(source, FILE, REGION, line(start)).expect("the block compiles")
    }

    /// The failure running `program` in `lua` raises.
    fn raised(program: &LuaProgram, lua: &Lua) -> mlua::Error {
        program
            .load(lua)
            .expect("the bytecode loads")
            .call::<()>(())
            .expect_err("the block fails")
    }

    #[test]
    fn a_compile_line_maps_the_chunk_line_or_keeps_the_block_line() {
        let diagnostic = "[string \"line 40\"]:3: unexpected symbol near <eof>";
        assert_eq!(compile_line("line 40", line(40), diagnostic), line(42));
        assert_eq!(
            compile_line("line 41", line(41), diagnostic),
            line(41),
            "another chunk's line is not this block's"
        );
        assert_eq!(compile_line("line 40", line(40), "no chunk line"), line(40));
        let chunk = format!("line {}", u32::MAX);
        assert_eq!(
            compile_line(
                &chunk,
                NonZeroU32::MAX,
                &format!("[string \"{chunk}\"]:2: x")
            ),
            NonZeroU32::MAX,
            "an overflowing mapping keeps the block line"
        );
    }

    #[test]
    fn a_compile_failure_is_located_on_its_diagnostics_line_with_author_prose() {
        let error = LuaProgram::compile("local a = 1\nlocal b =", FILE, REGION, line(11))
            .expect_err("the block must not compile");
        let diagnostic = "[string \"line 11\"]:2: unexpected symbol near <eof>";
        assert_eq!(error.code, ErrorCode::LuaCompile);
        assert_eq!(
            error.message,
            format!("lua compilation error at {REGION} (line 11): syntax error: {diagnostic}")
        );
        assert_eq!(error.at, Some(SourceLoc::line(FILE, line(12))));
        assert_eq!(
            error.facts.get(&ErrorFact::Section),
            Some(&Value::from(REGION))
        );
        assert_eq!(
            error.prose,
            format!(
                "unexpected symbol near <eof>. {}",
                ErrorCode::LuaCompile.prose()
            )
        );
    }

    #[test]
    fn a_block_keeps_its_source_and_its_lines_behind_the_frame_guard() {
        let source = "local x = 1\nreturn x";
        let program = compiled(source, 30);
        assert_eq!(program.source(), source);
        let lua = Lua::new();
        let raw = raised(&compiled("local x = 1\nerror('boom')", 30), &lua);
        assert!(
            raw.to_string().contains("[string \"line 30\"]:2: boom"),
            "the guard adds no line: {raw}"
        );
    }

    /// What `source`, compiled as a block, returns when it is called in
    /// `lua` with the ran-off marker, as the Engine calls a block.
    fn called(lua: &Lua, source: &str) -> MultiValue {
        compiled(source, 1)
            .load(lua)
            .expect("the bytecode loads")
            .call(ran_off_marker())
            .expect("the block runs")
    }

    /// Whether `source`, compiled as a block, runs off its end: called with
    /// the marker, it returns the marker and nothing else.
    fn runs_off(source: &str) -> bool {
        let returned = called(&Lua::new(), source);
        returned.len() == 1 && returned.iter().all(is_ran_off)
    }

    #[test]
    fn only_the_marker_itself_is_the_ran_off_marker() {
        assert!(is_ran_off(&ran_off_marker()));
        assert!(
            !is_ran_off(&mlua::Value::NULL),
            "`json.null` is another light userdata"
        );
        assert!(!is_ran_off(&mlua::Value::Nil));
        assert!(!is_ran_off(&mlua::Value::Boolean(true)));
    }

    #[test]
    fn a_source_that_compiles_only_inside_the_do_block_is_refused_as_written() {
        // Between `do` and `end` each of these is balanced Lua. Alone, each
        // closes a block it never opened, and that is the diagnostic.
        for source in ["end do", "end\ndo", "local x = 1 end do x = 2"] {
            let error = LuaProgram::compile(source, FILE, REGION, line(11))
                .expect_err("the block must not compile");
            assert_eq!(error.code, ErrorCode::LuaCompile, "{source:?}");
            assert!(
                error
                    .message
                    .contains("[string \"line 11\"]:1: <eof> expected near 'end'"),
                "{source:?} fails as the source alone does: {}",
                error.message
            );
            assert_eq!(
                error.at,
                Some(SourceLoc::line(FILE, line(11))),
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_block_with_no_return_runs_off_its_end_whatever_it_ends_in() {
        for source in [
            "",
            "\n",
            "-- only a comment",
            "local x = 1",
            "local x = 1 -- a comment on the last line",
            "local x = 1\n-- a comment line last\n",
            "local x = 1;",
            "--[[ a long comment ]]",
            "if false then return end",
            "if false then return 'never' end",
            "local function f() return 'inside' end\nf()",
            "for i = 1, 3 do local _ = i end",
        ] {
            assert!(runs_off(source), "{source:?} returns the marker");
        }
    }

    #[test]
    fn a_label_at_a_blocks_end_still_takes_a_goto_from_before_a_local() {
        // The label is a void statement at the end of its block, so the
        // local's scope ends before it, in the `do` block as in the chunk.
        assert!(runs_off("goto done\nlocal x = 1\n::done::"));
        assert!(runs_off("goto done\nlocal x = 1\n::done:: ;"));
    }

    #[test]
    fn an_executed_return_never_reaches_the_marker() {
        let lua = Lua::new();
        for source in [
            "return",
            "return;",
            "do return end",
            "if true then return end",
            "local function f() end\nreturn f()",
            "for _ = 1, 3 do return end",
        ] {
            assert!(
                called(&lua, source).is_empty(),
                "{source:?} returns no value at all"
            );
        }
        let nil = called(&lua, "return nil");
        assert!(
            nil.len() == 1 && nil.iter().all(mlua::Value::is_nil),
            "`return nil` returns one nil"
        );
        let pair = called(&lua, "return nil, 'text'");
        assert_eq!(pair.len(), 2, "`return nil, 'text'` returns both values");
        for (source, expected) in [
            ("return 'text'", "text"),
            (
                "local x = 'early'\nif x then return x end\nreturn 'late'",
                "early",
            ),
            ("return 7 -- a comment after the return", "7"),
        ] {
            let returned = called(&lua, source);
            let text = returned
                .iter()
                .next()
                .and_then(|value| value.to_string().ok());
            assert_eq!(text.as_deref(), Some(expected), "{source:?}");
            assert!(!returned.iter().any(is_ran_off), "{source:?}");
        }
    }

    #[test]
    fn a_block_that_returns_its_argument_reads_as_one_that_ran_off() {
        // The marker reaches the block as its one argument, which is the
        // only way a block can name it.
        assert!(runs_off("return ..."));
    }

    #[test]
    fn a_failure_on_a_blocks_last_line_keeps_its_line_before_the_trailer() {
        let lua = Lua::new();
        for (source, expected) in [
            ("error('boom')", "[string \"line 30\"]:1: boom"),
            ("local x = 1\nerror('boom')", "[string \"line 30\"]:2: boom"),
            (
                "local x = 1\n\nerror('boom') -- last",
                "[string \"line 30\"]:3: boom",
            ),
        ] {
            let raw = raised(&compiled(source, 30), &lua);
            assert!(raw.to_string().contains(expected), "{source:?}: {raw}");
        }
    }

    /// `depth` nested `do` blocks around one statement.
    fn nested(depth: usize) -> String {
        format!(
            "{}local x = 1 {}",
            "do ".repeat(depth),
            "end ".repeat(depth)
        )
    }

    #[test]
    fn a_source_at_luas_nesting_limit_fails_to_compile_inside_the_do_block() {
        // The `do` block is one more level, so the deepest source Lua
        // compiles alone is one level too deep as a block.
        let alone = |depth: usize| {
            Lua::new()
                .load(format!("{FRAME_GUARD}{}", nested(depth)))
                .into_function()
                .is_ok()
        };
        let deepest = (1..1_000)
            .take_while(|&depth| alone(depth))
            .last()
            .expect("Lua compiles one `do` block");
        assert!(deepest < 999, "Lua limits how deep blocks nest");
        assert!(
            LuaProgram::compile(&nested(deepest - 1), FILE, REGION, line(11)).is_ok(),
            "one level under the limit compiles as a block"
        );
        let error = LuaProgram::compile(&nested(deepest), FILE, REGION, line(11))
            .expect_err("the deepest source alone is too deep as a block");
        assert_eq!(error.code, ErrorCode::LuaCompile);
        assert_eq!(
            error.at.as_ref().map(|at| at.file.as_str()),
            Some(FILE),
            "the failure is placed in the prompt file"
        );
    }

    #[test]
    fn a_runtime_failure_is_located_on_its_mapped_line_with_author_prose() {
        let program = compiled("local x = 1\nerror('boom')", 50);
        let lua = Lua::new();
        let raw = raised(&program, &lua);
        let mapped = program.map_runtime_error_among(&raw, &[], FILE);
        let (message, _) =
            map_chunk_line_to_absolute(&raw.to_string(), &[("line 50", REGION, line(50))]);
        assert_eq!(mapped.code, ErrorCode::LuaRuntime);
        assert_eq!(mapped.message, message, "the message keeps its mapped text");
        assert_eq!(mapped.at, Some(SourceLoc::line(FILE, line(51))));
        assert_eq!(
            mapped.prose,
            format!("boom. {}", ErrorCode::LuaRuntime.prose())
        );
    }

    /// The failure running `program` raises in a Lua VM whose global `fail`
    /// raises `error` from Rust.
    fn raised_through(program: &LuaProgram, error: &runvil_types::error::Error) -> mlua::Error {
        let lua = Lua::new();
        let raise = error.clone();
        let fail = lua
            .create_function(move |_, ()| -> mlua::Result<()> {
                Err(mlua::Error::external(raise.clone()))
            })
            .expect("the callback builds");
        lua.globals()
            .set("fail", fail)
            .expect("the global installs");
        raised(program, &lua)
    }

    #[test]
    fn a_raised_internal_failure_keeps_its_rust_location_through_the_mapping() {
        let fault = make_internal_error("a test invariant broke");
        assert!(fault.at.is_some(), "an internal failure is located in Rust");
        let program = compiled("local x = 1\nfail()", 5);
        let mapped = program.map_runtime_error_among(&raised_through(&program, &fault), &[], FILE);
        assert_eq!(
            mapped, fault,
            "it is not recoded at the line that raised it"
        );
    }

    #[test]
    fn a_raised_context_exhaustion_is_placed_on_its_mapped_line() {
        let exhausted = make_context_exhausted_error("precheck");
        let program = compiled("local x = 1\nfail()", 5);
        let mapped =
            program.map_runtime_error_among(&raised_through(&program, &exhausted), &[], FILE);
        assert_eq!(
            (mapped.code, &mapped.message, &mapped.prose),
            (exhausted.code, &exhausted.message, &exhausted.prose),
            "it keeps its own failure and its prose"
        );
        assert_eq!(mapped.at, Some(SourceLoc::line(FILE, line(6))));
    }

    #[test]
    fn a_prompt_line_is_named_only_by_a_prompt_blocks_chunk() {
        assert_eq!(prompt_line("line 40", 3), Some(line(42)));
        assert_eq!(prompt_line("line 40", 0), Some(line(39)));
        for other in [
            "line 0",
            "line +4",
            "line ",
            "@pack:web",
            "probe",
            "lines 4",
        ] {
            assert_eq!(prompt_line(other, 3), None, "{other:?}");
        }
        assert_eq!(
            prompt_line(&format!("line {}", u32::MAX), 2),
            None,
            "an overflowing line is none"
        );
    }

    #[test]
    fn the_first_prompt_position_skips_function_markers_and_other_chunks() {
        let traceback = "stack traceback:\n\t[C]: in ?\n\
                     \t@crates/runvil-internal/lua/src/__impl_coro.lua:91: in function 'pcall'\n\
                     \t[string \"probe\"]:3: in function <[string \"line 7\"]:2>\n\
                     \t[string \"line 7\"]:4: in main chunk";
        assert_eq!(first_prompt_line(traceback), Some(line(10)));
        assert_eq!(
            first_prompt_line("in function <[string \"line 7\"]:2>"),
            None,
            "a function marker is no position"
        );
        assert_eq!(first_prompt_line("[string \"line 7\"]:: x"), None);
    }

    #[test]
    fn a_quota_refusal_raised_through_lua_is_recognized_at_the_boundary() {
        let lua = Lua::new();
        let log = lua
            .create_function(|_, ()| -> mlua::Result<()> {
                Err(mlua::Error::external(make_lua_quota_error(
                    QuotaResource::LogEvent,
                )))
            })
            .expect("the callback builds");
        lua.globals().set("log", log).expect("the global installs");
        let program = compiled("local x = 1\nlog()", 7);
        let mapped = program.map_runtime_error_among(&raised(&program, &lua), &[], FILE);
        let quota = make_lua_quota_error(QuotaResource::LogEvent);
        assert_eq!(
            (mapped.code, &mapped.message, &mapped.facts),
            (ErrorCode::LuaQuota, &quota.message, &quota.facts)
        );
        assert_eq!(mapped.at, Some(SourceLoc::line(FILE, line(8))));
        assert_eq!(
            mapped.prose,
            format!(
                "lua log event quota exceeded. {}",
                ErrorCode::LuaQuota.prose()
            )
        );
        let mimic = compiled("error('lua log event quota exceeded')", 7);
        assert_eq!(
            mimic
                .map_runtime_error_among(&raised(&mimic, &lua), &[], FILE)
                .code,
            ErrorCode::LuaRuntime,
            "an author who raises the same text fails as a runtime error"
        );
    }
}
