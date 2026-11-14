//! The store error value: each failure's reason tag, fields, and message,
//! as rendered through the Lua boundary and as a block catches it from a
//! store call.

use super::*;

/// A store failure a block catches with `pcall` is an error table of kind
/// `store`: its `code` and `reason` tags and the failure's own fields read
/// from Lua, an anchor's `count` as an integer, and `tostring` gives the
/// code's prose after the `file:line` of the store call, not of the
/// `pcall` that caught it.
#[test]
fn a_store_failure_caught_by_pcall_carries_the_reason_tag() {
    let vm = TestVm::new().shims().build();
    let missing = drive_scalar(
        &vm,
        "local ok, err = pcall(function()\n\
           local text = store.read('missing.txt')\n\
           return text\n\
         end)\n\
         return tostring(ok) .. '|' .. err.kind .. '|' .. err.code .. '|' .. err.reason\n\
           .. '|' .. err.path .. '|' .. tostring(err)",
    )
    .expect("the catching block must run");
    assert_eq!(
        missing,
        Some(format!(
            "false|store|not_found|not_found|missing.txt|prompt:2: {}",
            ErrorCode::NotFound.prose()
        )),
        "a caught missing read must carry the documented shape and its call's line"
    );

    let ambiguous = drive_scalar(
        &vm,
        "store.write('a.txt', 'na na na')\n\
         local ok, err = pcall(store.str_replace, 'a.txt', 'na', 'la')\n\
         return tostring(ok) .. '|' .. err.kind .. '|' .. err.reason .. '|' .. err.path\n\
           .. '|' .. err.anchor .. '|' .. math.type(err.count) .. '|' .. err.count\n\
           .. '|' .. tostring(err)",
    )
    .expect("the catching block must run");
    assert_eq!(
        ambiguous,
        Some(format!(
            "false|store|anchor|a.txt|na|integer|3|prompt:2: {}",
            ErrorCode::Anchor.prose()
        )),
        "a caught ambiguous anchor must carry its fields, the count as an integer"
    );
}

/// One store failure's rendering: the reason tag, the fields (with the
/// anchor's `count` checked as an integer), and the model-facing message.
#[expect(
    clippy::too_many_lines,
    reason = "one table holds every reason's rendering beside its fields and message"
)]
#[test]
fn store_error_values_carry_reason_fields_and_message_for_each_reason() {
    use runvil_vfs::{PathReason, VfsError};

    struct Case {
        error: VfsError,
        reason: &'static str,
        fields: Vec<(String, crate::ErrorField)>,
        message: &'static str,
    }

    fn path_field(path: &str) -> Vec<(String, crate::ErrorField)> {
        vec![(
            "path".to_owned(),
            crate::ErrorField::String(path.to_owned()),
        )]
    }

    let cases = [
        Case {
            error: VfsError::NotFound {
                path: "notes.md".to_owned(),
            },
            reason: "not_found",
            fields: path_field("notes.md"),
            message: "file not found in store: notes.md",
        },
        Case {
            error: VfsError::Anchor {
                path: "notes.md".to_owned(),
                anchor: "TODO".to_owned(),
                count: 3,
            },
            reason: "anchor",
            fields: vec![
                (
                    "path".to_owned(),
                    crate::ErrorField::String("notes.md".to_owned()),
                ),
                (
                    "anchor".to_owned(),
                    crate::ErrorField::String("TODO".to_owned()),
                ),
                ("count".to_owned(), crate::ErrorField::Integer(3)),
            ],
            message: "anchor \"TODO\" occurs 3 times in notes.md, expected exactly one; \
                      include more surrounding text so it matches once",
        },
        Case {
            error: VfsError::InvalidPath {
                path: "../x".to_owned(),
                reason: PathReason::Traversal,
            },
            reason: "invalid_path",
            fields: vec![
                (
                    "path".to_owned(),
                    crate::ErrorField::String("../x".to_owned()),
                ),
                (
                    "rule".to_owned(),
                    crate::ErrorField::String("traversal".to_owned()),
                ),
            ],
            message: "invalid path \"../x\": path contains a traversal segment",
        },
        Case {
            error: VfsError::InvalidRange {
                path: "notes.md".to_owned(),
                reason: "start must be at least 1",
            },
            reason: "invalid_range",
            fields: path_field("notes.md"),
            message: "invalid line range for notes.md: start must be at least 1",
        },
        Case {
            error: VfsError::NotUtf8 {
                path: "a.bin".to_owned(),
                valid_up_to: 4,
            },
            reason: "not_utf8",
            fields: vec![
                (
                    "path".to_owned(),
                    crate::ErrorField::String("a.bin".to_owned()),
                ),
                ("valid_up_to".to_owned(), crate::ErrorField::Integer(4)),
            ],
            message: "file in store is not UTF-8 after byte 4: a.bin",
        },
        Case {
            error: VfsError::IsADirectory {
                path: "d".to_owned(),
            },
            reason: "is_a_directory",
            fields: path_field("d"),
            message: "is a directory in store: d",
        },
        Case {
            error: VfsError::NotADirectory {
                path: "d".to_owned(),
            },
            reason: "not_a_directory",
            fields: path_field("d"),
            message: "not a directory in store: d",
        },
        Case {
            error: VfsError::DirectoryNotEmpty {
                path: "d".to_owned(),
            },
            reason: "directory_not_empty",
            fields: path_field("d"),
            message: "directory not empty in store: d",
        },
        Case {
            error: VfsError::AlreadyExists {
                path: "d".to_owned(),
            },
            reason: "already_exists",
            fields: path_field("d"),
            message: "file already exists in store: d",
        },
        Case {
            error: VfsError::PermissionDenied {
                path: "d".to_owned(),
                reason: "read-only mount".to_owned(),
            },
            reason: "permission_denied",
            fields: path_field("d"),
            message: "permission denied for store path d: read-only mount",
        },
        Case {
            error: VfsError::ApprovalRequired {
                path: "d".to_owned(),
                reason: "ask first".to_owned(),
            },
            reason: "approval_required",
            fields: path_field("d"),
            message: "store path d needs approval, which this run cannot give: ask first",
        },
        Case {
            error: VfsError::ScopeEnded {
                path: "d".to_owned(),
            },
            reason: "scope_ended",
            fields: path_field("d"),
            message: "store access ended: the run or tool call that owned it has ended, so d \
                      can no longer be touched through it",
        },
        Case {
            error: VfsError::ReadOnly {
                path: "d".to_owned(),
            },
            reason: "read_only",
            fields: path_field("d"),
            message: "store path is read-only: d",
        },
        Case {
            error: VfsError::OutsideRoot {
                path: "d".to_owned(),
                detail: "d escapes the mounted root".to_owned(),
            },
            reason: "outside_root",
            fields: path_field("d"),
            message: "store path d is outside the mounted root: d escapes the mounted root",
        },
        Case {
            error: VfsError::RootProtected {
                path: "d".to_owned(),
            },
            reason: "root_protected",
            fields: path_field("d"),
            message: "the store's mounted root cannot be removed, renamed, or replaced: d",
        },
        Case {
            error: VfsError::CrossMount {
                path: "d".to_owned(),
                detail: "rename needs one mount".to_owned(),
            },
            reason: "cross_mount",
            fields: path_field("d"),
            message: "store operation on d spans two mounts: rename needs one mount",
        },
        Case {
            error: VfsError::Io {
                path: "d".to_owned(),
                kind: std::io::ErrorKind::PermissionDenied,
                raw_os_error: Some(13),
                message: "access is denied".to_owned(),
            },
            reason: "io",
            fields: path_field("d"),
            message: "store I/O failure on d: access is denied",
        },
        Case {
            error: VfsError::Unsupported {
                path: "d".to_owned(),
                detail: "no rename".to_owned(),
            },
            reason: "unsupported",
            fields: path_field("d"),
            message: "unsupported store operation on d: no rename",
        },
        Case {
            error: VfsError::Backend {
                message: "disk gone".to_owned(),
            },
            reason: "backend",
            fields: Vec::new(),
            message: "store backend failure: disk gone",
        },
    ];

    for case in cases {
        let fields = crate::store_error_value_fields(&case.error);
        let Some((reason, fields)) = fields.split_first() else {
            panic!(
                "the reason leads every store error's fields: {:?}",
                case.error
            );
        };
        let op = crate::VfsOp::Read {
            path: "x".to_owned(),
            start: None,
            end: None,
        };
        let message = crate::store_error_message(&op, &case.error);
        assert_eq!(
            reason,
            &(
                "reason".to_owned(),
                crate::ErrorField::String(case.reason.to_owned())
            ),
            "wrong reason for {:?}",
            case.error
        );
        assert_eq!(fields, case.fields, "wrong fields for {:?}", case.error);
        assert_eq!(message, case.message, "wrong message for {:?}", case.error);
    }
}

/// A glob failure renders as an invalid glob pattern - the call surface's
/// promised wording - while a path failure renders as an invalid path.
#[test]
fn store_error_messages_use_the_operations_wording() {
    use runvil_vfs::{PathReason, VfsError};
    let error = VfsError::InvalidPath {
        path: "a**b".to_owned(),
        reason: PathReason::Wildcard,
    };
    let glob = crate::VfsOp::Glob {
        pattern: "a**b".to_owned(),
    };
    let read = crate::VfsOp::Read {
        path: "a**b".to_owned(),
        start: None,
        end: None,
    };
    assert_eq!(
        crate::store_error_message(&glob, &error),
        "invalid glob pattern \"a**b\": pattern contains invalid wildcard grammar"
    );
    assert_eq!(
        crate::store_error_message(&read, &error),
        "invalid path \"a**b\": pattern contains invalid wildcard grammar"
    );
}

/// A store failure raised through the Lua boundary renders as an error
/// table of kind `store` whose `count` field is a Lua integer, and
/// `tostring` of the unlocated failure returns its prose.
#[test]
fn a_raised_store_error_renders_an_integer_count_and_the_prose() {
    let anchor = runvil_vfs::VfsError::Anchor {
        path: "notes.md".to_owned(),
        anchor: "TODO".to_owned(),
        count: 3,
    };
    let op = crate::VfsOp::StrReplace {
        path: "notes.md".to_owned(),
        old: "TODO".to_owned(),
        new: "DONE".to_owned(),
    };
    let mut error = crate::make_store_error(&anchor);
    error.message = crate::store_error_message(&op, &anchor);
    assert!(
        error.message.contains("include more surrounding text"),
        "the message carries the recovery hint: {error}"
    );
    let lua = mlua::Lua::new();
    let table = crate::error_value::error_table(&lua, &error).expect("the table builds");
    assert_eq!(
        table.raw_get::<String>("kind").expect("the kind reads"),
        "store"
    );
    assert_eq!(
        table.raw_get::<Value>("count").expect("the count reads"),
        Value::Integer(3)
    );
    assert_eq!(
        table
            .raw_get::<String>("message")
            .expect("the message reads"),
        error.prose
    );
    let raised = crate::error_value::raised_from(&lua, &Value::Table(table))
        .expect("the table reads back")
        .expect("the table is a structured error table");
    assert_eq!(
        (raised.code, &raised.facts),
        (error.code, &error.facts),
        "the count stays an integer through the boundary"
    );
}
