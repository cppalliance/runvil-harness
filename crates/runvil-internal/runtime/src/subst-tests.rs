//! Regression coverage for `{{ }}` prose substitution: the resolution
//! of the `args`, `item`, `var`, `sys`, and bare-global sources, the
//! escape grammar, path segment validation, and the failure reasons.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use runvil_types::error::SourceLoc;

use super::*;
use serde_json::json;

/// The lookup for prose with no section VM behind it: every bare-global
/// name comes back unset.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature must match the globals-lookup callback type"
)]
fn no_globals(_: &str) -> Result<Option<Value>> {
    Ok(None)
}

/// The prompt line the stock sources' prose starts on.
const PROSE_LINE: NonZeroU32 = NonZeroU32::new(7).expect("7 is non-zero");

/// The stock args object: a string, an integer, a nested object, a list,
/// and a null.
static STOCK_ARGS: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "name": "Acme Corp",
        "limit": 3,
        "row": { "a": 1 },
        "tags": ["x", "y"],
        "none": null,
    })
});

/// Sources with the stock args object and no item, over the given
/// `var`/`sys`, for prose that starts on [`PROSE_LINE`] of `prompt`; tests
/// override fields with struct-update syntax.
fn test_sources<'a>(var: &'a Value, sys: &'a Value) -> Sources<'a> {
    Sources {
        args: &STOCK_ARGS,
        item: None,
        var,
        sys,
        globals: &no_globals,
        file: "prompt",
        line: Some(PROSE_LINE),
    }
}

fn run(prose: &str) -> Result<String> {
    let var = json!({ "kind": "library", "count": 3, "row": { "a": 1 } });
    let sys = json!({ "when": "2026-07-29T00:00:00Z", "id": 1 });
    substitute(prose, &test_sources(&var, &sys))
}

fn err_of(prose: &str) -> Error {
    let var = json!({ "kind": "library", "row": { "a": 1 }, "arr": [1, 2] });
    let sys = json!({ "id": 1 });
    let sources = Sources {
        item: Some(&json!("i")),
        ..test_sources(&var, &sys)
    };
    substitute(prose, &sources).expect_err("expected substitution failure")
}

/// The `Reason` fact of a substitution failure.
fn reason(error: &Error) -> Option<&str> {
    error.facts.get(&ErrorFact::Reason)?.as_str()
}

#[test]
fn substitution_diagnostics_escape_and_bound_the_placeholder() {
    let hostile = format!("var.{}", "x".repeat(500));
    let preview = path_preview(&hostile);
    assert!(
        preview.chars().count() <= 83,
        "preview must be bounded, got {} chars",
        preview.chars().count()
    );
    assert!(preview.ends_with("..."), "over-long preview must be elided");

    let with_controls = path_preview("var.a\nb\tc");
    assert!(
        !with_controls.contains('\n') && !with_controls.contains('\t'),
        "control characters must be escaped, got: {with_controls}"
    );
    assert!(with_controls.contains("\\n") && with_controls.contains("\\t"));
}

#[test]
fn resolves_an_args_field() {
    assert_eq!(run("hi {{ args.name }}!").unwrap(), "hi Acme Corp!");
    assert_eq!(run("at most {{ args.limit }}").unwrap(), "at most 3");
}

#[test]
fn resolves_an_args_dotted_path() {
    assert_eq!(
        run("cell={{ args.row.a }} row={{ args.row }} tags={{ args.tags }}").unwrap(),
        "cell=1 row={\"a\":1} tags=[\"x\",\"y\"]"
    );
}

#[test]
fn bare_args_renders_the_whole_object_as_compact_json() {
    assert_eq!(
        run("got {{ args }}").unwrap(),
        "got {\"limit\":3,\"name\":\"Acme Corp\",\"none\":null,\
         \"row\":{\"a\":1},\"tags\":[\"x\",\"y\"]}"
    );
    let var = json!({});
    let sys = json!({});
    let empty = json!({});
    let sources = Sources {
        args: &empty,
        ..test_sources(&var, &sys)
    };
    assert_eq!(substitute("got {{ args }}", &sources).unwrap(), "got {}");
}

#[test]
fn an_args_field_the_object_lacks_is_a_missing_key() {
    for prose in ["{{ args.query }}", "{{ args.row.b }}", "{{ args.name.x }}"] {
        let e = err_of(prose);
        assert_eq!(reason(&e), Some("missing_key"), "path {prose:?}");
        assert_eq!(e.message, format!("missing {prose} [MissingKey at byte 0]"));
    }
}

#[test]
fn a_null_args_field_is_a_null_value() {
    let var = json!({});
    let sys = json!({});
    let e = substitute("{{ args.none }}", &test_sources(&var, &sys)).unwrap_err();
    assert_eq!(reason(&e), Some("null_value"));
    assert_eq!(e.message, "missing {{ args.none }} [NullValue at byte 0]");
}

#[test]
fn resolves_var_scalar() {
    assert_eq!(run("a {{ var.kind }} paper").unwrap(), "a library paper");
    assert_eq!(run("{{ var.count }}").unwrap(), "3");
}

#[test]
fn resolves_sys() {
    assert_eq!(run("id {{ sys.id }}").unwrap(), "id 1");
    assert_eq!(run("at {{ sys.when }}").unwrap(), "at 2026-07-29T00:00:00Z");
}

#[test]
fn table_renders_as_json() {
    assert_eq!(run("{{ var.row }}").unwrap(), "{\"a\":1}");
}

#[test]
fn missing_key_is_error() {
    assert!(run("{{ var.nope }}").is_err());
    assert!(run("{{ ghost.x }}").is_err());
    let sys_error = run("{{ sys.bogus }}").expect_err("unknown sys field must fail");
    assert!(
        sys_error.to_string().contains("missing {{ sys.bogus }}"),
        "error was {sys_error}"
    );
}

#[test]
fn no_placeholders_passthrough() {
    assert_eq!(run("plain text").unwrap(), "plain text");
}

#[test]
fn unclosed_is_error() {
    assert_eq!(reason(&err_of("open {{ args")), Some("unclosed"));
}

// --- Escape grammar ------------------------------------------------------

#[test]
fn escaped_delimiters_are_literal() {
    assert_eq!(
        run(r"literal \{{ args }} here").unwrap(),
        "literal {{ args }} here"
    );
    assert_eq!(run(r"close \}} brace").unwrap(), "close }} brace");
    assert_eq!(run(r"back \\ slash").unwrap(), r"back \ slash");
}

#[test]
fn escape_then_real_placeholder_adjacent() {
    // First delimiter escaped, second one live and resolved.
    assert_eq!(run(r"\{{x}}{{ args.name }}").unwrap(), "{{x}}Acme Corp");
}

#[test]
fn lone_backslash_is_literal() {
    assert_eq!(run(r"a\b").unwrap(), r"a\b");
    assert_eq!(run("trailing\\").unwrap(), "trailing\\");
}

#[test]
fn replacement_produced_delimiters_are_not_resubstituted() {
    let var = json!({ "payload": "{{ args.name }}" });
    let sys = json!({});
    // `var.payload` renders text that looks like a placeholder; it must be
    // emitted verbatim, never resolved against args.
    let args = json!({ "name": "SECRET" });
    let sources = Sources {
        args: &args,
        ..test_sources(&var, &sys)
    };
    let out = substitute("value: {{ var.payload }}", &sources).unwrap();
    assert_eq!(out, "value: {{ args.name }}");
}

// --- Path segment grammar ------------------------------------------------

#[test]
fn empty_or_padded_segments_are_rejected() {
    for bad in ["var.", "var..x", "var. .x", "var.x.", "var. .x .y"] {
        let prose = format!("{{{{ {bad} }}}}");
        let e = err_of(&prose);
        assert_eq!(
            reason(&e),
            Some("empty_segment"),
            "path {bad:?} must be an empty-segment error, got {e:?}"
        );
    }
}

#[test]
fn valid_nested_segment_still_resolves() {
    assert_eq!(run("{{ var.row.a }}").unwrap(), "1");
}

// --- Failure reason/offset/cause -----------------------------------------

#[test]
fn error_has_reason_and_offset() {
    let e = err_of("prefix {{ ghost.x }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
    assert!(
        e.message.ends_with("[UnknownNamespace at byte 7]"),
        "offset must point at the '{{{{': {e}"
    );
    assert!(e.message.contains("ghost.x"));
}

#[test]
fn a_failure_is_a_substitution_error_with_its_reason_fact() {
    let e = err_of("see {{ var.nope }}");
    let detail = "missing {{ var.nope }}";
    assert_eq!(e.code, ErrorCode::Substitution);
    assert_eq!(e.message, format!("{detail} [MissingKey at byte 4]"));
    assert_eq!(
        e.prose,
        format!("{detail}. {}", ErrorCode::Substitution.prose())
    );
    assert_eq!(
        e.facts,
        BTreeMap::from([(ErrorFact::Reason, Value::from("missing_key"))])
    );
    assert_eq!(
        e.at,
        Some(SourceLoc::line("prompt", PROSE_LINE)),
        "a placeholder on the prose's first line fails there"
    );
}

#[test]
fn a_failure_is_placed_on_its_placeholders_line_of_the_prose() {
    let var = json!({});
    let sys = json!({});
    let sources = test_sources(&var, &sys);
    for (prose, newlines) in [
        ("one\ntwo {{ var.nope }}", 1),
        ("one\n\nthree\n{{ unclosed", 3),
    ] {
        let e = substitute(prose, &sources).expect_err("the placeholder fails");
        assert_eq!(
            e.at.map(|at| at.start_line.get()),
            Some(PROSE_LINE.get() + newlines),
            "{prose:?}"
        );
    }
    let unplaced = Sources {
        line: None,
        ..test_sources(&var, &sys)
    };
    assert_eq!(
        substitute("{{ var.nope }}", &unplaced)
            .expect_err("the placeholder fails")
            .at,
        None,
        "prose whose line is unknown places nothing"
    );
}

#[test]
fn a_failure_is_a_substitution_error_that_lua_reads_as_lua() {
    let error = run("{{ var.nope }}").expect_err("a missing key fails the substitution");
    assert_eq!(error.code, ErrorCode::Substitution);
    assert_eq!(reason(&error), Some("missing_key"));
    assert_eq!(runvil_lua::error_kind(&error), runvil_lua::ErrorKind::Lua);
    assert_eq!(runvil_lua::error_fields(&error), Vec::new());
}

// --- bare globals (section-local Lua globals in prose) -------------------

#[test]
fn bare_global_resolves_a_scalar() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "answer").then(|| json!(42)));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("the answer is {{ answer }}", &sources).unwrap();
    assert_eq!(out, "the answer is 42");
}

#[test]
fn bare_global_dotted_path_indexes_the_json() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "row").then(|| json!({ "a": { "b": 2 } })));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("cell {{ row.a.b }}", &sources).unwrap();
    assert_eq!(out, "cell 2");
}

#[test]
fn bare_global_table_renders_as_json() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "row").then(|| json!({ "a": 1 })));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("{{ row }}", &sources).unwrap();
    assert_eq!(out, "{\"a\":1}");
}

#[test]
fn missing_bare_global_is_unknown_namespace() {
    let e = err_of("{{ ghost }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
    assert!(e.to_string().contains("global 'ghost'"));
}

#[test]
fn non_json_bare_global_is_an_error() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| {
        assert_eq!(name, "f");
        Err(crate::error::make_lua_runtime_error(
            "global `f` is a function",
        ))
    };
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let e = substitute("{{ f }}", &sources).unwrap_err();
    assert_eq!(reason(&e), Some("serialize"));
    assert_eq!(
        e.message,
        "global 'f' in {{ f }} is not JSON data [Serialize at byte 0]: global `f` is a function",
        "the lookup failure must be preserved in the message"
    );
    assert!(
        !e.prose.contains("is a function"),
        "the lookup failure must stay out of the prose: {e:?}"
    );
}

#[test]
fn bare_namespaces_still_require_a_key() {
    // `{{ var }}` and `{{ sys }}` are `bad_path`; only `args` and a bare
    // global render whole values.
    for prose in ["{{ var }}", "{{ sys }}"] {
        let e = err_of(prose);
        assert_eq!(reason(&e), Some("bad_path"), "path {prose:?}");
    }
}

#[test]
fn reply_is_no_longer_a_namespace() {
    // `{{ reply }}` resolves as a bare global like any other name, and is
    // unset here.
    let e = err_of("{{ reply }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
}

#[test]
fn null_value_and_nil_item_reasons() {
    let var = json!({ "n": Value::Null });
    let sys = json!({});
    let e = substitute("{{ var.n }}", &test_sources(&var, &sys)).unwrap_err();
    assert_eq!(reason(&e), Some("null_value"));

    let e = substitute("{{ item }}", &test_sources(&var, &sys)).unwrap_err();
    assert_eq!(reason(&e), Some("nil_item"));
}

#[test]
fn not_a_table_reason() {
    let e = err_of("{{ item.x }}");
    assert_eq!(reason(&e), Some("not_a_table"));
    assert_eq!(
        e.message,
        "item takes no key path; {{ item }} renders the whole item [NotATable at byte 0]"
    );
}

// --- Null, arrays, trust-neutral passthrough -----------------------------

#[test]
fn array_renders_as_json() {
    let var = json!({ "arr": [1, 2, 3] });
    let sys = json!({});
    let out = substitute("{{ var.arr }}", &test_sources(&var, &sys)).unwrap();
    assert_eq!(out, "[1,2,3]");
}

#[test]
fn resolves_item_when_present() {
    let var = json!({});
    let sys = json!({});
    let sources = Sources {
        item: Some(&json!("the angle")),
        ..test_sources(&var, &sys)
    };
    let out = substitute("topic: {{ item }}", &sources).unwrap();
    assert_eq!(out, "topic: the angle");
}

#[test]
fn item_nil_is_error() {
    let var = json!({});
    let sys = json!({});
    let err = substitute("{{ item }}", &test_sources(&var, &sys)).expect_err("nil item must fail");
    assert!(
        err.to_string()
            .contains("{{ item }} is nil (no item is seeded into this section)"),
        "error must say no item is seeded: {err}"
    );
}

#[test]
fn item_dot_path_is_error() {
    let var = json!({});
    let sys = json!({});
    let sources = Sources {
        item: Some(&json!("text")),
        ..test_sources(&var, &sys)
    };
    let err = substitute("{{ item.x }}", &sources).expect_err("item takes no key path");
    assert!(
        err.to_string()
            .contains("item takes no key path; {{ item }} renders the whole item"),
        "error must say item renders whole: {err}"
    );
}
