//! Line-mapping tests: a compile error and a runtime error in a Lua block
//! report its file-absolute line, and each prose block records the file
//! line its text starts on.

use super::*;
use runvil_lua::LuaProgram;

#[test]
fn each_prose_block_records_the_file_line_its_trimmed_text_starts_on() {
    //  6: Plan.          <- the preamble's prose, which the fence on line 8 consumes
    // 12: ## S
    // 14: Notes above.   <- dropped by the break below it
    // 16: ---
    // 18: Pending.       <- the prose the fence on line 21 consumes
    // 19: Second line.
    // 25: Trailing.      <- after the last fence
    let src = "---\nname: x\ndescription: d\n---\n\nPlan.\n\n```lua\ny = 0\n```\n\n\
               ## S\n\nNotes above.\n\n---\n\n\
               Pending.\nSecond line.\n\n```lua\nx = 1\n```\n\nTrailing.\n";
    let prompt = parse_ok(src);
    let prose = |blocks: &[Block]| -> Vec<(String, u32)> {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Prose { text, line } => Some((text.as_str().to_owned(), line.get())),
                Block::Lua(_) => None,
            })
            .collect()
    };
    assert_eq!(prompt.sections[0].name, "", "the preamble comes first");
    assert_eq!(
        prose(prompt.sections[0].blocks()),
        [("Plan.".to_owned(), 6)]
    );
    assert_eq!(
        prose(prompt.sections[1].blocks()),
        [
            ("Pending.\nSecond line.".to_owned(), 18),
            ("Trailing.".to_owned(), 25)
        ]
    );
}

fn assert_runtime_error_line(program: &LuaProgram, absolute_line: u32) {
    let lua = mlua::Lua::new();
    let function = program.load(&lua).expect("bytecode must load");
    let raw_error = function
        .call::<()>(())
        .expect_err("assert(false) must fail");
    let mapped = program.map_runtime_error(&raw_error);
    let msg = mapped.to_string();
    assert!(
        msg.contains(&format!(":{absolute_line}:")),
        "error must contain absolute line {absolute_line}: {msg}"
    );
}

#[test]
fn epilog_source_line_maps_runtime_error_to_absolute_line() {
    // Lines:
    //  1: ---
    //  2: name: x
    //  3: description: d
    //  4: ---
    //  5: (empty)
    //  6: ## Check
    //  7: (empty)
    //  8: Ask the model.
    //  9: (empty)
    // 10: ```lua       <- epilog opens
    // 11: local a = 1  <- epilog line 1 (source_line = 11)
    // 12: assert(false) <- epilog line 2 (absolute = 12)
    // 13: ```
    let src = "---\nname: x\ndescription: d\n---\n\n## Check\n\nAsk the model.\n\n```lua\nlocal a = 1\nassert(false)\n```\n";
    let prompt = parse_ok(src);
    let epilog = prompt
        .entry()
        .expect("has sections")
        .epilog()
        .expect("epilog must exist");

    assert_eq!(
        epilog.source_line().get(),
        11,
        "epilog Lua starts on line 11"
    );
    assert_eq!(epilog.source(), "local a = 1\nassert(false)");

    // Simulate a runtime error: assert(false) is on chunk line 2.
    // Absolute line = 11 + 2 - 1 = 12.
    assert_runtime_error_line(epilog, 12);
}

#[test]
fn prologue_source_line_maps_correctly() {
    // Lines:
    //  1: ---
    //  2: name: x
    //  3: description: d
    //  4: ---
    //  5: (empty)
    //  6: ## Work
    //  7: (empty)
    //  8: ```lua       <- prologue opens
    //  9: assert(false) <- prologue line 1 (source_line = 9, absolute = 9)
    // 10: ```
    // 11: (empty)
    // 12: Do the work.
    let src = "---\nname: x\ndescription: d\n---\n\n## Work\n\n```lua\nassert(false)\n```\n\nDo the work.\n";
    let prompt = parse_ok(src);
    let prologue = prompt
        .entry()
        .expect("has sections")
        .prologue()
        .expect("prologue must exist");

    assert_eq!(
        prologue.source_line().get(),
        9,
        "prologue Lua starts on line 9"
    );

    assert_runtime_error_line(prologue, 9);
}

#[test]
fn multi_line_chunk_maps_inner_line_correctly() {
    // Epilog with assert on line 3 of the fence.
    //  1-4: frontmatter
    //  5: empty
    //  6: ## S
    //  7: empty
    //  8: Prose.
    //  9: empty
    // 10: ```lua
    // 11: local x = 1    <- source_line = 11
    // 12: local y = 2
    // 13: assert(false)  <- chunk line 3, absolute = 11 + 3 - 1 = 13
    // 14: ```
    let src = "---\nname: x\ndescription: d\n---\n\n## S\n\nProse.\n\n```lua\nlocal x = 1\nlocal y = 2\nassert(false)\n```\n";
    let prompt = parse_ok(src);
    let epilog = prompt
        .entry()
        .expect("has sections")
        .epilog()
        .expect("epilog must exist");

    assert_eq!(epilog.source_line().get(), 11);

    assert_runtime_error_line(epilog, 13);
}

#[test]
fn every_block_of_a_section_maps_only_its_own_errors() {
    //  6: ## S
    //  8: ```lua         <- prologue
    // 14: ```lua         <- second block
    // 15: assert(false)
    // 20: ```lua         <- third block (source_line = 21)
    // 21: local y = 2
    // 22: assert(false)  <- chunk line 2, absolute = 22
    // 27: ```lua         <- epilog
    let src = "---\nname: x\ndescription: d\n---\n\n\
         ## S\n\n```lua\nx = 1\n```\n\nOne.\n\n```lua\nassert(false)\n```\n\nTwo.\n\n\
         ```lua\nlocal y = 2\nassert(false)\n```\n\nThree.\n\n```lua\nz = 3\n```\n";
    let prompt = parse_ok(src);
    let section = prompt.entry().expect("has sections");
    let programs: Vec<&LuaProgram> = section
        .blocks()
        .iter()
        .filter_map(|block| match block {
            Block::Lua(program) => Some(program),
            Block::Prose { .. } => None,
        })
        .collect();
    let locations: std::collections::BTreeSet<&str> =
        programs.iter().map(|program| program.location()).collect();
    assert_eq!(
        locations.len(),
        4,
        "each of the four blocks needs its own chunk name, got {locations:?}"
    );
    let (second, third) = (programs[1], programs[2]);
    assert_eq!(third.source_line().get(), 21);
    assert_runtime_error_line(third, 22);

    let lua = mlua::Lua::new();
    let raw_error = third
        .load(&lua)
        .expect("bytecode must load")
        .call::<()>(())
        .expect_err("assert(false) must fail");
    assert_eq!(
        second.map_runtime_error(&raw_error).to_string(),
        raw_error.to_string(),
        "another block's program must leave the third block's error unmapped"
    );
}

#[test]
fn sections_sharing_a_title_get_locations_named_by_heading_line() {
    //  8: ### Helpers    <- under `A`
    // 10: ```lua         <- source_line = 11
    // 16: ### Helpers    <- under `B`
    // 18: ```lua         <- source_line = 19
    // 20: assert(false)  <- chunk line 2, absolute = 20
    // 23: ## C           <- a title no other section uses
    let src = "---\nname: x\ndescription: d\n---\n\n\
         ## A\n\n### Helpers\n\n```lua\nassert(false)\n```\n\n\
         ## B\n\n### Helpers\n\n```lua\nlocal pad = 1\nassert(false)\n```\n\n\
         ## C\n\n```lua\nx = 1\n```\n";
    let prompt = parse_ok(src);
    let only_program = |section: &Section| match section.blocks() {
        [Block::Lua(program)] => program.clone(),
        other => panic!("one Lua block expected, got {other:?}"),
    };
    let under_a = only_program(&prompt.sections[0].children()[0]);
    let under_b = only_program(&prompt.sections[1].children()[0]);
    assert_eq!(under_a.location(), "section `Helpers` (line 8) prologue");
    assert_eq!(under_b.location(), "section `Helpers` (line 16) prologue");
    assert_eq!(
        only_program(&prompt.sections[2]).location(),
        "section `C` prologue"
    );
    assert_runtime_error_line(&under_b, 20);

    let lua = mlua::Lua::new();
    let raw_error = under_b
        .load(&lua)
        .expect("bytecode must load")
        .call::<()>(())
        .expect_err("assert(false) must fail");
    assert_eq!(
        under_a.map_runtime_error(&raw_error).to_string(),
        raw_error.to_string(),
        "the other `Helpers` must leave this one's error unmapped"
    );
}

#[test]
fn every_preamble_block_maps_only_its_own_errors() {
    //  6: ```lua         <- first block (source_line = 7)
    //  7: assert(false)
    // 10: Plan.
    // 12: ```lua         <- second block (source_line = 13)
    let src = "---\nname: x\ndescription: d\n---\n\n```lua\nassert(false)\n```\n\nPlan.\n\n```lua\nx = 1\n```\n\n## S\n\np\n";
    let prompt = parse_ok(src);
    let preamble = prompt.entry().expect("has sections");
    assert_eq!(preamble.name, "", "the preamble comes first");
    let [Block::Lua(first), Block::Prose { .. }, Block::Lua(second)] = preamble.blocks() else {
        panic!("Lua, prose, Lua expected, got {:?}", preamble.blocks());
    };
    assert_eq!(first.location(), "preamble prologue");
    assert_eq!(second.location(), "preamble epilog");
    assert_eq!(second.source_line().get(), 13);
    assert_runtime_error_line(first, 7);

    let lua = mlua::Lua::new();
    let raw_error = first
        .load(&lua)
        .expect("bytecode must load")
        .call::<()>(())
        .expect_err("assert(false) must fail");
    assert_eq!(
        second.map_runtime_error(&raw_error).to_string(),
        raw_error.to_string(),
        "the second preamble block must leave the first block's error unmapped"
    );
}

#[test]
fn a_preamble_compile_error_reports_its_file_line() {
    //  6: Plan.
    //  8: ```lua         <- the preamble's epilog (source_line = 9)
    //  9: local a = 1
    // 10: local b =      <- chunk line 2, absolute = 10
    let src = "---\nname: x\ndescription: d\n---\n\nPlan.\n\n```lua\nlocal a = 1\nlocal b =\n```\n\n## S\n\np\n";
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::LuaCompile, "{error:?}");
    assert!(
        error
            .message
            .starts_with("lua compilation error at preamble epilog (line 9): "),
        "{error:?}"
    );
    assert_eq!(
        location(&error),
        Some(("prompt", 10, 1, 11, 1)),
        "the whole line the compiler names: {error:?}"
    );
}

#[test]
fn a_long_unique_section_title_maps_its_prologue_line() {
    //  6: ## Gather Background Research Notes
    //  8: ```lua         <- prologue opens
    //  9: assert(false)  <- source_line = 9, absolute = 9
    let src = "---\nname: x\ndescription: d\n---\n\n## Gather Background Research Notes\n\n```lua\nassert(false)\n```\n\nGo.\n";
    let prompt = parse_ok(src);
    let prologue = prompt
        .entry()
        .expect("has sections")
        .prologue()
        .expect("prologue must exist");
    assert_eq!(
        prologue.location(),
        "section `Gather Background Research Notes` prologue"
    );
    assert_runtime_error_line(prologue, 9);
}

#[test]
fn a_shared_title_whose_heading_line_reaches_100_maps_its_prologue_line() {
    //   8: ### Research Worker Notes  <- under `A`
    //  11: assert(false)              <- under `A` source_line = 11
    //  14-103: Padding.
    // 107: ### Research Worker Notes  <- under `B`
    // 110: local pad = 1              <- under `B` source_line = 110
    // 111: assert(false)              <- chunk line 2, absolute = 111
    let src = format!(
        "---\nname: x\ndescription: d\n---\n\n\
         ## A\n\n### Research Worker Notes\n\n```lua\nassert(false)\n```\n\n{}\n\
         ## B\n\n### Research Worker Notes\n\n```lua\nlocal pad = 1\nassert(false)\n```\n",
        "Padding.\n".repeat(90),
    );
    let prompt = parse_ok(&src);
    let prologue = |section: &Section| section.prologue().expect("prologue must exist").clone();
    let under_a = prologue(&prompt.sections[0].children()[0]);
    let under_b = prologue(&prompt.sections[1].children()[0]);
    assert_eq!(
        under_b.location(),
        "section `Research Worker Notes` (line 107) prologue"
    );
    assert_eq!(under_b.source_line().get(), 110);
    assert_runtime_error_line(&under_b, 111);

    let lua = mlua::Lua::new();
    let raw_error = under_b
        .load(&lua)
        .expect("bytecode must load")
        .call::<()>(())
        .expect_err("assert(false) must fail");
    assert_eq!(
        under_a.map_runtime_error(&raw_error).to_string(),
        raw_error.to_string(),
        "the other `Research Worker Notes` must leave this one's error unmapped"
    );
}

#[test]
fn unrecognized_lua_label_error_reports_the_fence_opening_line() {
    // Lines:
    //  6: ## S
    //  8: Prose.
    // 10: ```lua extra <- the error points here
    // 11: function f()
    // 12: end
    // 13: ```
    // 15: ```Lua       <- a later offender, not reported
    let src = "---\nname: x\ndescription: d\n---\n\n\
         ## S\n\nProse.\n\n```lua extra\nfunction f()\nend\n```\n\n```Lua\nreturn 1\n```\n";
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
    assert_eq!(
        location(&error),
        Some(("prompt", 10, 1, 10, 13)),
        "the opening line without its newline: {error}"
    );
}
