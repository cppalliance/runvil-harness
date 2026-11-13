//! Preamble tests: when the content before the first heading is a section,
//! its live blocks, and the fence rules in it.

use super::*;

const LABEL_NEAR_MISSES: [&str; 4] = ["lua extra", "Lua", "LUA", "luau"];

/// The preamble of `prompt`: its first top-level section, whose name is
/// empty.
#[track_caller]
fn preamble_of(prompt: &Prompt) -> &Section {
    let first = prompt.entry().expect("the prompt has a section");
    assert_eq!(first.name, "", "the first section is the preamble");
    first
}

/// The names of the top-level sections of `prompt`, in file order.
fn top_level_names(prompt: &Prompt) -> Vec<&str> {
    prompt.sections.iter().map(Section::name).collect()
}

#[test]
fn a_body_with_no_heading_and_a_lua_block_is_one_preamble_section() {
    let prompt = parse_ok(&PromptSource::new().preamble("return 'done'").build());
    assert_eq!(prompt.sections.len(), 1);
    let preamble = preamble_of(&prompt);
    assert_eq!(preamble.level, 1, "with no heading the top level is 1");
    assert!(preamble.children.is_empty());
    assert!(matches!(
        preamble.blocks(),
        [Block::Lua(program)] if program.source() == "return 'done'"
    ));
}

#[test]
fn a_body_with_no_heading_and_no_lua_block_has_no_section() {
    let empty = PromptSource::new();
    let prose = PromptSource::new().preamble_prose("Text.\n\n```text\nnot Lua\n```");
    for src in [empty, prose] {
        let prompt = parse_ok(&src.build());
        assert!(prompt.sections.is_empty(), "{src:?}");
        assert!(prompt.entry().is_none(), "{src:?}");
    }
}

#[test]
fn a_prose_only_preamble_is_commentary() {
    // With no `lua` fence the content before the first heading is
    // commentary, whatever else it holds: the prompt has no preamble, and
    // its first section is the first heading's.
    let commentary = "Commentary.\n\n```text\nnot Lua\n```\n\n---\n\nMore commentary.";
    let src = PromptSource::new().preamble_prose(commentary);
    let prompt = parse_ok(&src.prose("S", "p").prose("Next", "q").build());
    assert_eq!(top_level_names(&prompt), ["S", "Next"]);
    let entry = prompt.entry().expect("has sections");
    assert_eq!(entry.name, "S");
    assert_eq!(entry.prose(), "p", "no commentary reaches the section");
}

#[test]
fn a_lua_fence_shown_inside_a_longer_fence_does_not_make_a_preamble() {
    let shown = "````markdown\n```lua\nreturn 1\n```\n````";
    let src = PromptSource::new().preamble_prose(shown);
    let prompt = parse_ok(&src.prose("S", "p").build());
    assert_eq!(top_level_names(&prompt), ["S"]);
}

#[test]
fn a_preamble_with_lua_is_the_first_top_level_section() {
    // The preamble has an empty name, no children, and the top level as its
    // level, and the top-level headings follow it as its siblings.
    for top in 1..=3_u8 {
        let marker = "#".repeat(usize::from(top));
        let src = PromptSource::new()
            .preamble("local ready = true")
            .prose(&format!("{marker} A"), "a")
            .prose(&format!("{marker}# B"), "b")
            .prose(&format!("{marker} C"), "c")
            .build();
        let prompt = parse_ok(&src);
        assert_eq!(top_level_names(&prompt), ["", "A", "C"], "top level {top}");
        let preamble = preamble_of(&prompt);
        assert_eq!(preamble.level, top);
        assert!(preamble.children.is_empty(), "top level {top}");
        let a = &prompt.sections[1];
        assert_eq!((a.level, a.children[0].name()), (top, "B"));
    }
}

#[test]
fn the_preamble_markdown_is_its_own_content() {
    let own = "Intro.\n\n```lua\nvar.n = 1\n```\n\ntext";
    let src = format!("---\nname: x\ndescription: d\n---\n\n\n{own}\n\n## S\n\np\n");
    let prompt = parse_ok(&src);
    let preamble = preamble_of(&prompt);
    assert_eq!(
        preamble.markdown(),
        own,
        "the preamble's Markdown is its own content, stopping at the first heading"
    );
    // The Markdown starts on a line of the file, after the frontmatter and
    // the blank lines under it.
    assert_eq!(preamble.markdown_line().get(), 7);
}

#[test]
fn preamble_lua_and_prose_are_live_blocks() {
    let preamble = "```lua\nlocal first = 1\n```\n\nPlan {{ args }}.\n\n```lua\nstore.write('done', reply)\n```";
    let src = PromptSource::new().preamble_prose(preamble);
    let prompt = parse_ok(&src.prose("S", "p").build());
    let blocks = preamble_of(&prompt).blocks();
    assert_eq!(blocks.len(), 3);
    assert!(matches!(
        &blocks[0],
        Block::Lua(program) if program.source() == "local first = 1"
    ));
    assert!(matches!(
        &blocks[1],
        Block::Prose { text, .. } if text == "Plan {{ args }}."
    ));
    assert!(matches!(
        &blocks[2],
        Block::Lua(program) if program.source() == "store.write('done', reply)"
    ));
}

#[test]
fn a_lone_preamble_lua_fence_is_a_live_block() {
    let lua = "local live = true";
    let prompt = parse_ok(&PromptSource::new().preamble(lua).prose("S", "p").build());
    assert!(matches!(
        preamble_of(&prompt).blocks(),
        [Block::Lua(program)] if program.source() == "local live = true"
    ));
}

#[test]
fn preamble_lua_blocks_are_labeled_by_their_place() {
    let src = PromptSource::new()
        .preamble("var.a = 1")
        .preamble("var.b = 2")
        .preamble("var.c = 3");
    let prompt = parse_ok(&src.prose("S", "p").build());
    let labels: Vec<&str> = preamble_of(&prompt)
        .blocks()
        .iter()
        .filter_map(|block| match block {
            Block::Lua(program) => Some(program.location()),
            Block::Prose { .. } => None,
        })
        .collect();
    assert_eq!(
        labels,
        ["preamble prologue", "preamble lua 2", "preamble epilog"]
    );
}

#[test]
fn a_preamble_compile_error_names_the_block_by_its_label() {
    let sources = ["var.a = 1", "var.b = 2", "var.c = 3"];
    for (broken, label) in [
        (0, "preamble prologue"),
        (1, "preamble lua 2"),
        (2, "preamble epilog"),
    ] {
        let mut src = PromptSource::new();
        for (index, lua) in sources.into_iter().enumerate() {
            src = src.preamble(if index == broken { "broken =" } else { lua });
        }
        let error = parse_err(&src.prose("S", "p").build());
        assert_eq!(error.code, ErrorCode::LuaCompile, "{label}: {error:?}");
        assert!(
            error
                .message
                .starts_with(&format!("lua compilation error at {label} (line ")),
            "{error:?}"
        );
    }
}

#[test]
fn an_unclosed_preamble_lua_fence_is_refused_naming_the_preamble() {
    // The frontmatter takes lines 1-4, so the body's content starts on
    // line 6. The failure is on the fence's opening line. An unclosed fence
    // runs to the end of the body, so a heading below it is Lua source, not
    // a section.
    for (body, role, line) in [
        ("```lua\nreturn 1\n", "prologue", 6),
        ("```lua\nreturn 1\n\n## S\n\np\n", "prologue", 6),
        ("Prose.\n\n```lua\nreturn 1\n", "epilog", 8),
        ("```lua\nvar.a = 1\n```\n\n```lua\nreturn 1\n", "epilog", 10),
    ] {
        let error = parse_err(&format!("---\nname: x\ndescription: d\n---\n\n{body}"));
        assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
        assert_eq!(
            error.message,
            format!("preamble {role} `lua` fence is not closed")
        );
        assert_eq!(
            location(&error),
            Some(("prompt", line, 1, line + 1, 1)),
            "{error}"
        );
    }
}

#[track_caller]
fn assert_unrecognized_label(src: &str, label: &str) {
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
    assert_eq!(
        error.to_string(),
        format!("unrecognized fence label `{label}`; the only Lua fence label is `lua`")
    );
}

#[test]
fn unrecognized_lua_label_in_the_preamble_fails_the_parse() {
    let src = "---\r\nname: x\r\ndescription: d\r\n---\r\n\r\n \t\r\n```lua extra\r\nfunction answer() return 42 end\r\n```\r\n\r\nDescription.\r\n\r\n## S\r\n\r\np\r\n";
    assert_unrecognized_label(src, "lua extra");
}

#[test]
fn unrecognized_lua_label_in_a_section_fails_the_parse() {
    let body = "Prose.\n\n```lua extra\nlocal a = 1\n```";
    assert_unrecognized_label(&PromptSource::new().prose("S", body).build(), "lua extra");
}

#[test]
fn the_first_unrecognized_label_is_the_one_reported() {
    let preamble = "```Lua\nlocal a = 1\n```\n\n```lua extra\nlocal b = 2\n```";
    let src = PromptSource::new().preamble_prose(preamble);
    assert_unrecognized_label(&src.prose("S", "p").build(), "Lua");
}

#[test]
fn preamble_label_near_misses_fail_whatever_the_opener_shape() {
    // Each opener shape here is one pulldown-cmark reports as a fence, so
    // the label alone decides, leading or after prose.
    for label in LABEL_NEAR_MISSES {
        for opener in [
            format!("```{label}\nreturn 1\n```"),
            format!("````{label}\nreturn 1\n````"),
            format!(" ```{label}\nreturn 1\n ```"),
        ] {
            for intro in ["", "Intro.\n\n"] {
                let src =
                    format!("---\nname: x\ndescription: d\n---\n\n{intro}{opener}\n\n## S\n\np\n");
                assert_unrecognized_label(&src, label);
            }
        }
    }

    let unclosed =
        "---\nname: x\ndescription: d\n---\n\n```lua extra\nreturn 1\n````\n\n## S\n\np\n";
    assert_unrecognized_label(unclosed, "lua extra");
}

#[track_caller]
fn assert_opener_violation(src: &str) {
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
    assert_eq!(error.to_string(), OPENER_MESSAGE, "{src:?}");
}

#[test]
fn non_exact_lua_openers_fail_in_the_preamble() {
    for fence in NON_EXACT_LUA_OPENERS {
        for intro in ["", "Intro.\n\n"] {
            assert_opener_violation(&format!(
                "---\nname: x\ndescription: d\n---\n\n{intro}{fence}\n\n## S\n\np\n"
            ));
        }
    }
}

#[test]
fn label_near_misses_inside_longer_fences_remain_prose() {
    let src = "---\nname: x\ndescription: d\n---\n\n````markdown\n```lua extra\nreturn 1\n```\n````\n\nIntro.\n\n```lua\nlocal live = 1\n```\n\n## S\n\n````markdown\n```lua extra\nreturn 2\n```\n````\n";
    let prompt = parse_ok(src);

    assert!(matches!(
        preamble_of(&prompt).blocks(),
        [Block::Prose { text, .. }, Block::Lua(_)] if text.as_str().contains("```lua extra")
    ));
    let section = &prompt.sections[1];
    assert!(section.prologue().is_none());
    assert!(section.prose().contains("```lua extra"));
}

#[test]
fn unrecognized_lua_label_fails_before_any_lua_compiles() {
    // The preamble block ahead of the near-miss does not compile, so a
    // parse that compiled any Lua first would fail with `LuaCompile`.
    let source = "private_payload =";
    let body = format!("```lua extra\n{source}\n```");
    let src = PromptSource::new().preamble("local broken =");
    let error = parse_err(&src.prose("S", &body).build());
    assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
    assert!(!error.to_string().contains(source), "{error}");
}

#[test]
fn non_exact_lua_opener_fails_before_any_lua_compiles() {
    // The preamble block ahead of the opener does not compile, so a parse
    // that compiled any Lua first would fail with `LuaCompile`.
    let source = "private_payload =";
    let body = format!("~~~lua\n{source}\n~~~");
    let src = PromptSource::new().preamble("local broken =");
    let error = parse_err(&src.prose("S", &body).build());
    assert_eq!(error.code, ErrorCode::FenceInvalid, "{error}");
    assert!(!error.to_string().contains(source), "{error}");
}
