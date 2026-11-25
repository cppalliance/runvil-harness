//! The three failure helpers the adapter's conversions and catalog rules
//! use. Each builds the Engine's [`Error`] from a model code and that code's
//! fixed phrase, as the client's own helpers do on its side.

use std::fmt::Display;

use runvil::model::{make_completion_error, phrase};
use runvil::{Error, ErrorCode};

/// A `MalformedResponse` failure: the code's fixed phrase extended with
/// `: ` and `specific`, which this crate's own code wrote. Provider text
/// never goes in `specific`.
pub(crate) fn make_malformed_error(specific: impl Display) -> Error {
    let code = ErrorCode::MalformedResponse;
    make_completion_error(code, format!("{}: {specific}", phrase(code)))
}

/// Appends provider `text`, already bounded and control-escaped, to
/// `error`'s message after `: `. The prose stays as it was, so provider
/// text never reaches the model.
pub(crate) fn with_detail(mut error: Error, text: &str) -> Error {
    error.message.push_str(": ");
    error.message.push_str(text);
    error
}

/// A validating constructor's refusal of parts this crate decoded from a
/// reply: an `InvalidParts` refusal becomes the `MalformedResponse` the
/// backend's reply earned, keeping its specific, since another reply may
/// hold whole parts. Any other failure passes through.
pub(crate) fn decoded(error: Error) -> Error {
    let code = ErrorCode::InvalidParts;
    if error.code != code {
        return error;
    }
    let prose = error.prose.as_str();
    let specific = prose
        .strip_prefix(phrase(code))
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(prose);
    make_malformed_error(specific)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_helper_uses_the_fixed_phrase_for_its_code() {
        let unreadable = make_malformed_error("model list body exceeds the 8-byte limit");
        assert_eq!(unreadable.code, ErrorCode::MalformedResponse);
        assert_eq!(
            unreadable.to_string(),
            "the model backend sent a reply that could not be understood: \
             model list body exceeds the 8-byte limit"
        );
        assert_eq!(unreadable.message, unreadable.prose);
    }

    #[test]
    fn a_decoded_parts_refusal_is_a_malformed_reply_with_its_specific() {
        use runvil::model::ToolCall;
        let refusal = ToolCall::from_parts(" ", "fetch", serde_json::json!({}))
            .expect_err("a blank id is refused");
        assert_eq!(refusal.code, ErrorCode::InvalidParts);
        let reply = decoded(refusal);
        assert_eq!(reply.code, ErrorCode::MalformedResponse);
        assert_eq!(
            reply.to_string(),
            "the model backend sent a reply that could not be understood: tool call id was blank"
        );

        let unavailable = ErrorCode::Unavailable;
        let passed = decoded(make_completion_error(unavailable, phrase(unavailable)));
        assert_eq!(passed.code, ErrorCode::Unavailable);
    }
}
