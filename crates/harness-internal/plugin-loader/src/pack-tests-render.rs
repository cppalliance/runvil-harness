//! Tests for RV_PACK_RENDER: what the loader lends a Plugin's render (the
//! section entry, the model as JSON text, and the offered tools), what it
//! makes of what the Plugin lists, and how each way a render can fail
//! reads. A Plugin that answers RV_ERR_UNSUPPORTED shows the model the
//! offered tools as they are.

use std::num::NonZeroU32;

use runvil_plugin::{Error, ModelId, ModelInfo, Pack, RenderContext, ThinkingMode, ToolInfo};
use serde_json::json;

use super::*;

/// What a render was lent: the entry, the model text, the tools, whether
/// `offered` was NULL, and whether the call carried the contract's fields.
type Lent = (String, String, Vec<OfferedTool>, bool, bool);

/// The model a render here is asked for, with the provider `acme`.
fn model() -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway("m").expect("a valid model id"),
        "The round's model",
        NonZeroU32::new(4096).expect("a non-zero context window"),
        ThinkingMode::Never,
    )
    .with_provider("acme")
}

/// The Pack's catalog tools as its `RV_PACK_NEW` lists them, which are
/// the tools a section offers.
fn offered(pack: &Arc<dyn Pack>) -> Vec<ToolInfo> {
    pack.tools()
}

/// Renders `offered` on `pack` for the entry `0.1` and `model`.
fn render(
    pack: &Arc<dyn Pack>,
    model: &ModelInfo,
    offered: Vec<ToolInfo>,
) -> Result<Vec<ToolInfo>, Error> {
    block_on(pack.render(RenderContext::new("0.1", model), offered))
}

/// The tools of a Pack whose `RV_PACK_NEW` adds `echo`, which changes
/// files, has the wire name `Echo`, and takes a `text`, and then `plain`.
macro_rules! two_tools {
    ($call:ident) => {{
        set_pack_run($call, instance(PACK_TAG));
        add_tool_info(
            $call,
            &RvToolInfo {
                flags: RV_TOOL_CHANGES_FILES,
                wire_name: rv_str("Echo"),
                description: rv_str("Echoes a text."),
                parameters: rv_str(r#"{"type":"object","properties":{"text":{"type":"string"}}}"#),
                ..tool_info("echo")
            },
        );
        add_tool($call, "plain")
    }};
}

pack_fake!(
    keeping,
    {
        /// What the last render was lent.
        pub(super) static SEEN: Mutex<Option<super::Lent>> = Mutex::new(None);
    },
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| {
        let args = render_args(call);
        let tools = offered_tools(call);
        if let Ok(mut seen) = SEEN.lock() {
            *seen = Some((
                text(args.entry),
                text(args.model),
                tools.clone(),
                args.offered.is_null(),
                contract_holds(call, RV_PACK_RENDER),
            ));
        }
        for tool in &tools {
            list_as_offered(call, tool);
        }
        RV_OK
    }
);

#[test]
fn a_render_is_lent_the_entry_the_model_as_sorted_json_and_the_tools_as_the_catalog_has_them() {
    let pack = made(keeping::entry);
    let shown = render(&pack, &model(), offered(&pack)).expect("the render succeeds");
    let seen = keeping::SEEN
        .lock()
        .expect("the lock holds")
        .take()
        .expect("the Plugin was asked to render");
    let (entry, model_text, tools, null, well_formed) = seen;
    assert_eq!(entry, "0.1");
    assert_eq!(
        model_text,
        r#"{"context":4096,"description":"The round's model","name":"m","provider":"acme","server":"gateway","thinking":"never"}"#
    );
    assert_eq!(
        tools,
        [
            OfferedTool {
                name: "echo".to_owned(),
                wire_name: "Echo".to_owned(),
                description: "Echoes a text.".to_owned(),
                parameters:
                    json!({ "type": "object", "properties": { "text": { "type": "string" } } })
                        .to_string(),
                flags: RV_TOOL_CHANGES_FILES,
            },
            OfferedTool {
                name: "plain".to_owned(),
                wire_name: String::new(),
                description: "A fake tool.".to_owned(),
                parameters: json!({ "type": "object", "properties": {} }).to_string(),
                flags: 0,
            },
        ],
        "each name is without the Pack id, an absent wire name is empty, and the flag is the \
         catalog's"
    );
    assert!(!null, "two tools are offered through a pointer");
    assert!(well_formed, "the call carried the contract's fields");
    assert_eq!(
        shown,
        offered(&pack),
        "a Plugin that lists each offered tool as lent shows the model the offered tools"
    );
    assert_eq!(keeping::DELETES.load(Ordering::SeqCst), 0);
    drop(pack);
    assert_eq!(
        keeping::DELETES.load(Ordering::SeqCst),
        1,
        "a render leaves the instance in place, and it is deleted once"
    );
}

#[test]
fn a_model_with_no_provider_reads_each_thinking_mode_as_such_in_the_json() {
    let pack = made(keeping::entry);
    for (mode, word) in [
        (ThinkingMode::Never, "never"),
        (ThinkingMode::Always, "always"),
        (ThinkingMode::Switchable, "switchable"),
    ] {
        let model = ModelInfo::new(
            ModelId::new("local", "q").expect("a valid model id"),
            "A \"quoted\" model",
            NonZeroU32::new(8).expect("a non-zero context window"),
            mode,
        );
        render(&pack, &model, Vec::new()).expect("the render succeeds");
        let (_entry, model_text, ..) = keeping::SEEN
            .lock()
            .expect("the lock holds")
            .take()
            .expect("the Plugin was asked to render");
        assert_eq!(
            model_text,
            format!(
                r#"{{"context":8,"description":"A \"quoted\" model","name":"q","server":"local","thinking":"{word}"}}"#
            ),
            "{word}"
        );
    }
}

pack_fake!(
    shaping,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| {
        let offered = offered_tools(call);
        for tool in offered.iter().filter(|tool| tool.name == "echo") {
            render_add_tool_info(
                call,
                &RvToolInfo {
                    flags: tool.flags,
                    wire_name: rv_str("Look"),
                    description: rv_str("Look at a text."),
                    parameters: rv_str(&tool.parameters),
                    ..tool_info("echo")
                },
            );
        }
        render_add_tool_info(
            call,
            &RvToolInfo {
                flags: RV_TOOL_CHANGES_FILES,
                description: rv_str("Edits with a patch."),
                parameters: rv_str(r#"{"type":"object","properties":{"patch":{"type":"string"}}}"#),
                ..tool_info("lite")
            },
        )
    }
);

#[test]
fn a_render_may_rename_leave_out_and_list_a_tool_only_the_model_sees_in_order() {
    let pack = made(shaping::entry);
    let shown = render(&pack, &model(), offered(&pack)).expect("the render succeeds");
    let listed: Vec<(String, Option<&str>, &str, bool)> = shown
        .iter()
        .map(|tool| {
            (
                tool.id.to_string(),
                tool.wire_name.as_deref(),
                tool.description.as_str(),
                tool.changes_files,
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                "fake/echo".to_owned(),
                Some("Look"),
                "Look at a text.",
                true
            ),
            ("fake/lite".to_owned(), None, "Edits with a patch.", true),
        ],
        "echo is renamed and re-described, plain is left out, and lite is only the model's"
    );
    assert_eq!(
        shown[0].parameters_schema,
        offered(&pack)[0].parameters_schema,
        "the renamed tool keeps the parameters it was offered with"
    );
    assert_eq!(
        shown[1].parameters_schema,
        json!({ "type": "object", "properties": { "patch": { "type": "string" } } })
    );
}

pack_fake!(
    nothing_offered,
    {
        /// The pointer and the length of the last render's `offered`.
        pub(super) static LENT: Mutex<Option<(bool, usize)>> = Mutex::new(None);
    },
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| {
        let args = render_args(call);
        if let Ok(mut lent) = LENT.lock() {
            *lent = Some((args.offered.is_null(), args.offered_len));
        }
        RV_OK
    }
);

#[test]
fn a_render_with_no_offered_tool_is_lent_a_null_pointer_and_a_zero_length() {
    let pack = made(nothing_offered::entry);
    let shown = render(&pack, &model(), Vec::new()).expect("the render succeeds");
    assert_eq!(shown, Vec::<ToolInfo>::new());
    assert_eq!(
        *nothing_offered::LENT.lock().expect("the lock holds"),
        Some((true, 0))
    );
}

#[test]
fn a_plugin_that_answers_unsupported_shows_the_model_the_offered_tools_as_they_are() {
    let pack = made(good::entry);
    let tools = offered(&pack);
    let shown = render(&pack, &model(), tools.clone()).expect("an unsupported render passes");
    assert_eq!(shown, tools);
}

pack_fake!(
    refusing,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| render_refuse(call, "no tools for this model")
);

#[test]
fn a_refusal_fails_the_render_and_names_the_plugin_the_pack_and_the_reason() {
    let pack = made(refusing::entry);
    let error = render(&pack, &model(), offered(&pack)).expect_err("the render is refused");
    assert_eq!(error.code, ErrorCode::ToolOther);
    assert_eq!(
        error.message,
        "the Plugin fake declined to render the Pack fake: no tools for this model"
    );
}

pack_fake!(
    odd,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |_call| ODD_STATUS
);

#[test]
fn a_status_other_than_ok_or_unsupported_fails_the_render_and_names_it() {
    let pack = made(odd::entry);
    let error = render(&pack, &model(), offered(&pack)).expect_err("the render fails");
    assert_eq!(
        error.message,
        "the Plugin fake answered RV_PACK_RENDER for the Pack fake with status 77"
    );
}

pack_fake!(
    twice,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| {
        render_add_tool(call, "same");
        render_add_tool(call, "same")
    }
);

pack_fake!(
    badly_named,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| render_add_tool(call, "Not A Segment")
);

pack_fake!(
    wrong_ctx,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| render_add_tool_as(call, instance(1), &tool_info("lite"))
);

pack_fake!(
    refusing_twice,
    {},
    new: |call| two_tools!(call),
    step: |_call| RV_ERR_UNSUPPORTED,
    render: |call| {
        render_refuse(call, "first");
        render_refuse(call, "second")
    }
);

/// The message of a render that breaks a rule of the render functions.
macro_rules! render_misuse {
    ($detail:expr) => {
        concat!(
            "the Plugin fake misused the render functions for the Pack fake: ",
            $detail
        )
    };
}

#[test]
fn each_breach_of_a_render_function_fails_the_render_with_the_first_misuse() {
    let cases: [(&str, PluginEntry, &str); 3] = [
        (
            "a repeated name",
            twice::entry,
            render_misuse!("two tools of one render share a name"),
        ),
        (
            "a name that is not a segment",
            badly_named::entry,
            render_misuse!("the tool's name is not one name segment, such as `fetch`"),
        ),
        (
            "a ctx that is not the call's",
            wrong_ctx::entry,
            render_misuse!(
                "a render function was handed a ctx other than the open RV_PACK_RENDER call's"
            ),
        ),
    ];
    for (case, entry, message) in cases {
        let pack = made(entry);
        let error = render(&pack, &model(), offered(&pack)).expect_err(case);
        assert_eq!(error.message, message, "{case}");
    }
}

#[test]
fn a_second_refusal_of_a_render_is_a_misuse_and_the_first_stays_the_reason() {
    let pack = made(refusing_twice::entry);
    let error = render(&pack, &model(), offered(&pack)).expect_err("the render is refused");
    assert_eq!(
        error.message, "the Plugin fake declined to render the Pack fake: first",
        "the refusal is checked before the misuse, as RV_PACK_NEW checks them"
    );
}
