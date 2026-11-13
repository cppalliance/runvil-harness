//! Heading tests: the top level, section nesting, container headings, and
//! heading refusals.

use super::*;

/// The names of `sections`, in file order.
fn names(sections: &[Section]) -> Vec<&str> {
    sections.iter().map(Section::name).collect()
}

#[test]
fn the_top_level_is_the_shallowest_heading_level() {
    // Whatever level the body's shallowest headings have, each of them is a
    // top-level section, and a heading one level deeper is a child of the
    // section above it.
    for top in 1..=4_u8 {
        let heading = |depth: u8, name: &str| {
            let hashes = "#".repeat(usize::from(top + depth));
            format!("{hashes} {name}")
        };
        let src = PromptSource::new()
            .prose(&heading(0, "A"), "a")
            .prose(&heading(1, "B"), "b")
            .prose(&heading(2, "C"), "c")
            .prose(&heading(0, "D"), "d")
            .prose(&heading(1, "E"), "e")
            .build();
        let prompt = parse_ok(&src);
        assert_eq!(names(&prompt.sections), ["A", "D"], "top level {top}");
        let (a, d) = (&prompt.sections[0], &prompt.sections[1]);
        assert_eq!((a.level, d.level), (top, top));
        assert_eq!(names(&a.children), ["B"], "top level {top}");
        let b = &a.children[0];
        assert_eq!(b.level, top + 1);
        assert_eq!(names(&b.children), ["C"], "top level {top}");
        assert_eq!(b.children[0].level, top + 2);
        assert_eq!(names(&d.children), ["E"], "top level {top}");
        assert_eq!(d.children[0].level, top + 1);
    }
}

#[test]
fn several_h1_headings_are_sibling_top_level_sections() {
    let src = PromptSource::new()
        .prose("# A", "a")
        .prose("## B", "b")
        .prose("# C", "c")
        .prose("# D", "d")
        .build();
    let prompt = parse_ok(&src);
    assert_eq!(names(&prompt.sections), ["A", "C", "D"]);
    assert!(prompt.sections.iter().all(|section| section.level == 1));
    assert_eq!(names(&prompt.sections[0].children), ["B"]);
    assert!(prompt.sections[1].children.is_empty());
}

#[test]
fn a_first_heading_deeper_than_a_later_one_is_refused_as_an_orphan() {
    // The top level is the shallowest heading level in the body, so a first
    // heading deeper than a later one has no parent. The frontmatter takes
    // lines 1-4, so that first heading is on line 6, and the span is its
    // line through its newline.
    for (body, message) in [
        (
            "## A\n\na\n\n# B\n\nb\n",
            "section `A` is an orphan H2 heading with no parent H1",
        ),
        (
            "### A\n\na\n\n## B\n\nb\n",
            "section `A` is an orphan H3 heading with no parent H2",
        ),
        (
            "### A\n\na\n\n# B\n\nb\n",
            "section `A` is an orphan H3 heading with no parent H1",
        ),
    ] {
        let error = parse_err(&format!("---\nname: x\ndescription: d\n---\n\n{body}"));
        assert_eq!(error.code, ErrorCode::StructureInvalid, "{error}");
        assert_eq!(error.message, message);
        assert_eq!(location(&error), Some(("prompt", 6, 1, 7, 1)), "{error}");
    }
}

#[test]
fn recursive_nesting_h2_h3_h4() {
    let src = PromptSource::new()
        .prose("A", "a")
        .prose("### B", "b")
        .prose("#### C", "c")
        .build();
    let p = parse_ok(&src);
    let a = &p.sections[0];
    assert_eq!(a.name, "A");
    let b = &a.children[0];
    assert_eq!(b.name, "B");
    assert_eq!(b.level, 3);
    let c = &b.children[0];
    assert_eq!(c.name, "C");
    assert_eq!(c.level, 4);
}

#[test]
fn headings_inside_block_quotes_and_list_items_stay_in_the_section_prose() {
    let src = PromptSource::new()
        .prose("S", "> ## Quoted\n> kept\n\n- ## Listed\n\ntail")
        .prose("Next", "next")
        .build();
    let prompt = parse_ok(&src);
    assert_eq!(
        names(&prompt.sections),
        ["S", "Next"],
        "container headings are not sections, and a heading after the containers is"
    );
    let section = &prompt.sections[0];
    assert!(section.children.is_empty());
    let prose = section.prose();
    assert!(
        prose.contains("> ## Quoted") && prose.contains("- ## Listed") && prose.contains("tail"),
        "container headings stay in the enclosing prose: {prose}"
    );
}

#[test]
fn skipped_heading_level_is_rejected_as_orphan() {
    // H4 directly under H2 (no intervening H3) is an orphan deep heading:
    // it has no parent H3, so it must be rejected, not reparented to the H2.
    let src = PromptSource::new()
        .prose("A", "a")
        .prose("#### D", "d")
        .build();
    assert_parse_error(&src, ErrorCode::StructureInvalid, "orphan");
}

#[test]
fn an_empty_heading_is_refused_at_every_level() {
    // A heading alone in the body is a top-level section whatever its
    // level, so each of these reaches the name check. A heading is empty
    // with or without spaces after its marker.
    for level in 1..=6_usize {
        let marker = "#".repeat(level);
        for heading in [marker.clone(), format!("{marker}  ")] {
            let error = parse_err(&PromptSource::new().prose(&heading, "a").build());
            assert_eq!(error.code, ErrorCode::StructureInvalid, "{error}");
            assert_eq!(
                error.message,
                format!("an H{level} section heading must not be empty")
            );
        }
    }

    // An empty child heading is refused the same way.
    let src = PromptSource::new().prose("A", "a").prose("###", "b");
    assert_parse_error(
        &src.build(),
        ErrorCode::StructureInvalid,
        "an H3 section heading must not be empty",
    );
}

#[test]
fn duplicate_sibling_section_names_are_rejected() {
    // Two H2 siblings named `S` are ambiguous section targets.
    let src = "---\nname: x\ndescription: d\n---\n\n## S\n\na\n\n## S\n\nb\n";
    let err = parse_err(src);
    let message = err.to_string();
    assert!(
        message.contains("duplicate sibling section name"),
        "expected a duplicate-sibling error, got: {err}"
    );
    // Both duplicate locations are named. The frontmatter takes lines 1-4,
    // so the first `## S` is on line 6 and the second on line 10.
    assert!(
        message.contains("first declared at line 6") && message.contains("again at line 10"),
        "both duplicate locations must be reported, got: {message}"
    );
    // A structured parse error reports a stable code and the location of
    // the second heading rather than inferring them from the message.
    assert_eq!(err.code, ErrorCode::StructureInvalid);
    assert_eq!(
        location(&err),
        Some(("prompt", 10, 1, 11, 1)),
        "the second `## S` heading line, through its newline: {err:?}"
    );

    // Two H1 siblings named `S` are refused the same way.
    assert_parse_error(
        "---\nname: x\ndescription: d\n---\n\n# S\n\na\n\n# S\n\nb\n",
        ErrorCode::StructureInvalid,
        "duplicate sibling section name `S`: first declared at line 6, again at line 10",
    );

    // The same name under DIFFERENT parents (not siblings) is allowed.
    parse_ok(
        "---\nname: x\ndescription: d\n---\n\n## A\n\na\n\n### S\n\nx\n\n## B\n\nb\n\n### S\n\ny\n",
    );
}

#[test]
fn section_markdown_is_the_sections_own_content() {
    let own = "  indented first line\n\n```lua\nvar.n = 1\n```\n\ntext\n\n---\n\n{{ x }}";
    let src = format!(
        "---\nname: x\ndescription: d\n---\n\n## S\n\n\n{own}\n\n### Child\n\nchild text\n\n## Empty\n\n\n"
    );
    let prompt = parse_ok(&src);
    let section = &prompt.sections[0];
    assert_eq!(
        section.markdown(),
        own,
        "a section's Markdown is its own content byte for byte, stopping at its child"
    );
    assert_eq!(section.children[0].markdown(), "child text");
    assert_eq!(prompt.sections[1].markdown(), "");
    // The Markdown starts on a line of the file, after the frontmatter,
    // the heading, and the blank lines under it.
    assert_eq!(section.markdown_line().get(), 9);
    assert_eq!(section.children[0].markdown_line().get(), 23);
    let (shared, range) = section.markdown_text().shared();
    assert_eq!(&shared[range], own, "the stretch reads in place");
}

#[test]
fn the_first_top_level_section_is_the_entry_regardless_of_name() {
    let src = PromptSource::new()
        .prose("Zebra", "first")
        .prose("Main", "second")
        .build();
    let p = parse_ok(&src);
    assert_eq!(p.entry().expect("has sections").name, "Zebra");
}
