//! Tests for the conversions that need no gateway: the phrase table, the
//! message JSON, and the mapping of a client error onto the Engine's.

use gateway_client::{ErrorKind, Message as Wire, Role};
use runvil::model::{Message as Engine, phrase};

use super::*;

/// A client error with only its kind and texts set.
fn wire_error(kind: ErrorKind, message: &str, summary: &str) -> WireError {
    WireError {
        kind,
        message: message.to_owned(),
        summary: summary.to_owned(),
        status: None,
        finish_reason: None,
        prompt_tokens: None,
        context_window: None,
        reason: None,
        name: None,
    }
}

/// The client's phrase table is a copy of the Engine's. It must stay one. The two setup kinds
/// have no phrase in the Engine, because their messages are the setup text itself.
#[test]
fn each_model_kind_has_the_engine_phrase() {
    for kind in ErrorKind::ALL {
        if matches!(kind, ErrorKind::InvalidConfig | ErrorKind::InvalidRequest) {
            continue;
        }
        assert_eq!(kind.phrase(), phrase(kind.into_engine()), "{kind:?}");
    }
}

/// The two message types write the same JSON for the three cases both can build. The other
/// shapes (system, assistant with tool calls) have no public Engine constructor, and the
/// request tests cover them by stepping a real run.
#[test]
fn engine_and_client_messages_write_the_same_json() {
    let pairs = [
        (Engine::user("hi"), Wire::text(Role::User, "hi")),
        (Engine::assistant("ok"), Wire::text(Role::Assistant, "ok")),
        (Engine::tool("call_1", "42"), Wire::tool("call_1", "42")),
    ];
    for (engine, wire) in pairs {
        let engine = serde_json::to_value(&engine).expect("the Engine message serializes");
        let wire = serde_json::to_value(&wire).expect("the client message serializes");
        assert_eq!(engine, wire);
    }
}

/// The context overflow is the one error whose counts the Engine acts on.
#[test]
fn a_context_overflow_keeps_its_counts_as_engine_facts() {
    let mut wire = wire_error(ErrorKind::ContextOverflow, "full message", "the phrase");
    wire.prompt_tokens = Some(9000);
    wire.context_window = Some(8192);
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(error.message, "full message");
    assert_eq!(error.prose, "the phrase");
    assert_eq!(
        error.facts.get(&ErrorFact::Count),
        Some(&Value::from(9000_u32))
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Limit),
        Some(&Value::from(8192_u32))
    );
}

/// A setup error keeps the reason and the variable name that the Engine's callers read.
#[test]
fn a_setup_error_keeps_its_reason_and_name_as_engine_facts() {
    let mut wire = wire_error(
        ErrorKind::InvalidConfig,
        "missing environment variable: X",
        "same",
    );
    wire.reason = Some("missing_env");
    wire.name = Some("X".into());
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from("missing_env"))
    );
    assert_eq!(error.facts.get(&ErrorFact::Name), Some(&Value::from("X")));
}

/// An empty reply keeps the finish reason, as the `FinishReason` fact.
#[test]
fn an_empty_reply_keeps_its_finish_reason_as_an_engine_fact() {
    let mut wire = wire_error(ErrorKind::EmptyReply, "the phrase", "the phrase");
    wire.finish_reason = Some("stop".into());
    let error = wire.into_engine();
    assert_eq!(error.code, ErrorCode::EmptyReply);
    assert_eq!(
        error.facts.get(&ErrorFact::FinishReason),
        Some(&Value::from("stop"))
    );
}
