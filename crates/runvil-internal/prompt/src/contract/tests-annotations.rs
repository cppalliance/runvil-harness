//! Tests for the `annotations` key: the four behavior hints, unset versus
//! written, and each refused map with its message and position.

use super::*;
use crate::Annotations;

/// The hints the YAML block `front` declares.
#[track_caller]
fn declared(front: &str) -> Annotations {
    *parse_ok(&block_prompt(front)).frontmatter().annotations()
}

/// The four hints of `annotations`, in the order MCP lists them.
fn hints(annotations: Annotations) -> [Option<bool>; 4] {
    [
        annotations.read_only_hint(),
        annotations.destructive_hint(),
        annotations.idempotent_hint(),
        annotations.open_world_hint(),
    ]
}

/// Asserts that the YAML block `front` fails the parse with the message
/// `invalid frontmatter: ` then `refusal`, located at the file line and
/// column `at`.
#[track_caller]
fn assert_refused(front: &str, refusal: &str, at: (u32, u32)) {
    let error = parse_err(&block_prompt(front));
    let (line, column) = at;
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error:?}");
    assert_eq!(
        error.message,
        format!("invalid frontmatter: {refusal}"),
        "{front}"
    );
    assert_eq!(
        location(&error),
        Some(("prompt", line, column, line, column)),
        "{refusal}"
    );
}

#[test]
fn no_key_an_empty_map_and_null_each_leave_every_hint_unset() {
    let unset = parse_ok(&prompt("", &[]));
    assert_eq!(*unset.frontmatter().annotations(), Annotations::default());
    for front in ["annotations: {}", "annotations: ~"] {
        assert_eq!(declared(front), Annotations::default(), "{front}");
    }
    assert_eq!(hints(Annotations::default()), [None; 4]);
}

#[test]
fn each_hint_written_alone_reads_back_and_leaves_the_others_unset() {
    let keys = [
        "readOnlyHint",
        "destructiveHint",
        "idempotentHint",
        "openWorldHint",
    ];
    for (at, key) in keys.iter().enumerate() {
        for value in [true, false] {
            let mut expected = [None; 4];
            expected[at] = Some(value);
            let front = format!("annotations: {{ {key}: {value} }}");
            assert_eq!(hints(declared(&front)), expected, "{front}");
        }
    }
}

#[test]
fn all_four_hints_together_read_back() {
    let annotations = declared(
        "
        annotations:
          readOnlyHint: false
          destructiveHint: true
          idempotentHint: false
          openWorldHint: true
        ",
    );
    assert_eq!(
        hints(annotations),
        [Some(false), Some(true), Some(false), Some(true)]
    );
}

#[test]
fn a_hint_written_null_is_unset() {
    let annotations = declared("annotations: { readOnlyHint: null, openWorldHint: false }");
    assert_eq!(hints(annotations), [None, None, None, Some(false)]);
}

#[test]
fn an_unknown_hint_is_refused_naming_the_four_hints() {
    for key in ["readonlyHint", "read_only_hint", "title"] {
        assert_refused(
            &format!("annotations:\n  {key}: true\n"),
            &format!(
                "annotations: unknown field `{key}`, expected one of `readOnlyHint`, \
                 `destructiveHint`, `idempotentHint`, `openWorldHint` at line 5 column 3"
            ),
            (6, 3),
        );
    }
}

#[test]
fn a_write_hint_written_null_beside_read_only_is_unset_and_parses() {
    let annotations =
        declared("annotations: { readOnlyHint: true, destructiveHint: ~, idempotentHint: null }");
    assert_eq!(hints(annotations), [Some(true), None, None, None]);
}

#[test]
fn a_hint_written_twice_is_refused_even_when_its_first_value_is_null() {
    for value in ["true", "false", "null"] {
        assert_refused(
            &format!(
                "annotations:\n  readOnlyHint: {value}\n  openWorldHint: true\n  readOnlyHint: true\n"
            ),
            "annotations: duplicate field `readOnlyHint` at line 5 column 3",
            (6, 3),
        );
    }
}

#[test]
fn a_hint_that_is_not_a_boolean_is_refused() {
    // YAML 1.2 reads `yes` as a string.
    assert_refused(
        "annotations:\n  readOnlyHint: yes\n",
        "annotations.readOnlyHint: invalid type: string \"yes\", expected a boolean \
         at line 5 column 17",
        (6, 17),
    );
    assert_refused(
        "annotations:\n  readOnlyHint: 1\n",
        "annotations.readOnlyHint: invalid type: integer `1`, expected a boolean \
         at line 5 column 17",
        (6, 17),
    );
}

#[test]
fn annotations_that_are_not_a_map_are_refused() {
    assert_refused(
        "annotations: [readOnlyHint]\n",
        "annotations: invalid type: sequence, expected a map of behavior hints at line 4 column 14",
        (5, 14),
    );
}

#[test]
fn read_only_preserves_write_hints_regardless_of_their_order() {
    for value in [true, false] {
        for written in [
            format!("readOnlyHint: true, destructiveHint: {value}, idempotentHint: {value}"),
            format!("idempotentHint: {value}, destructiveHint: {value}, readOnlyHint: true"),
        ] {
            let annotations = declared(&format!("annotations: {{ {written} }}"));
            assert_eq!(
                hints(annotations),
                [Some(true), Some(value), Some(value), None],
                "{written}"
            );
        }
    }
}
