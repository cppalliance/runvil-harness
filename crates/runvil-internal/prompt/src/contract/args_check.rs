//! Checks a caller's arguments against a prompt's `args` declaration.
//!
//! [`accept`] reads the argument text as one JSON object, gives each
//! missing field its declared default, and refuses the first value that
//! does not match. The order is fixed: text that is not JSON, then JSON
//! that is not an object, then each object's declared fields in name
//! order, then its undeclared fields in name order. A declared field is
//! checked for a missing required value, then its type, then its `enum`,
//! then its own contents, and a list's items are checked in index order.
//! A missing field takes its default before that check, so a required
//! field that declares a default is never missing.
//!
//! A `null` sent for a field that is not required counts as the field
//! left out, so the field takes its default or stays absent. A `null`
//! sent for a required field is refused, with or without a default. A
//! `null` anywhere else is a value of no declared type, except in an
//! object that takes undeclared fields, where an undeclared field keeps
//! it. An `integer` takes any JSON number with no fractional part that
//! fits in 64 bits, and the accepted object holds it as an integer.

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use serde_json::{Map, Value as Json};

use super::args::{ArgsSchema, Kind, Node, ObjectNode};

/// The most characters of a refused string a refusal shows.
const SHOWN_CHARACTERS: usize = 40;

/// 2^63, one past the largest integer 64 bits hold.
const ABOVE_INTEGERS: f64 = 9_223_372_036_854_775_808.0;

/// One value that does not match its node.
pub(super) struct Mismatch {
    /// The tag of the rule the value broke, the failure's `Reason` fact.
    reason: &'static str,
    /// The argument the value belongs to as a path from the root, such as
    /// `opts.depth` or `focus[1]`, where a list index counts from 0. It is
    /// empty for the checked value itself.
    name: String,
    /// What the rule requires and what the value is, as the words that
    /// follow the argument's name.
    detail: String,
}

impl Mismatch {
    /// The refusal of a declared default this mismatch was found in: the
    /// text after the node's path.
    pub(super) fn of_default(&self) -> String {
        if self.name.is_empty() {
            format!("the default {}", self.detail)
        } else {
            format!("the default's `{}` {}", self.name, self.detail)
        }
    }

    /// The failure of a run whose arguments hold this mismatch.
    fn into_error(self) -> Error {
        make_args_error(
            self.reason,
            Some(&self.name),
            format!("argument `{}` {}", self.name, self.detail),
        )
    }
}

/// Reads `text` as the arguments `schema` declares and returns the object
/// a run holds: the caller's fields, with each missing field's default.
///
/// # Errors
/// Returns an `ArgsInvalid` failure with no location, whose `Reason` fact
/// is `not_json` for text that is not JSON, `not_an_object` for any JSON
/// but an object, and `required`, `type`, `enum`, or `undeclared` for the
/// first value that does not match, whose path is the `Name` fact.
pub(crate) fn accept(schema: &ArgsSchema, text: &str) -> Result<Json, Error> {
    let mut args: Json = serde_json::from_str(text).map_err(|cause| {
        make_args_error("not_json", None, "the arguments are not valid JSON").with_cause(&cause)
    })?;
    if !args.is_object() {
        return Err(make_args_error(
            "not_an_object",
            None,
            format!(
                "the arguments must be a JSON object, got {}",
                json_kind(&args)
            ),
        ));
    }
    check(schema.root(), &mut args, "", Checked::Arguments).map_err(Mismatch::into_error)?;
    Ok(args)
}

/// What a checked value is, which words the refusal of a field that the
/// value's own object does not declare.
#[derive(Debug, Clone, Copy)]
pub(super) enum Checked {
    /// A run's arguments, whose own object is the root of the declaration.
    Arguments,
    /// A declared default, whose own object is the node that declares it.
    Default,
}

/// Checks `value`, the argument named `name`, against `node`, filling the
/// default of each missing field inside it and turning each whole number
/// under an `integer` node into an integer. `checked` says what the value
/// with the empty name is.
pub(super) fn check(
    node: &Node,
    value: &mut Json,
    name: &str,
    checked: Checked,
) -> Result<(), Mismatch> {
    let mismatch = |reason, detail: String| Mismatch {
        reason,
        name: name.to_owned(),
        detail,
    };
    let wrong_type = |value: &Json| {
        mismatch(
            "type",
            format!("must be {}, got {}", node.kind.expected(), json_kind(value)),
        )
    };
    match &node.kind {
        Kind::String { choices } => {
            let Json::String(text) = value else {
                return Err(wrong_type(value));
            };
            match choices {
                Some(choices) if !choices.contains(text) => {
                    let allowed: Vec<String> = choices.iter().map(|c| format!("{c:?}")).collect();
                    Err(mismatch(
                        "enum",
                        format!("must be one of {}, got {}", allowed.join(", "), shown(text)),
                    ))
                }
                _ => Ok(()),
            }
        }
        Kind::Boolean if value.is_boolean() => Ok(()),
        Kind::Number if value.is_number() => Ok(()),
        Kind::Boolean | Kind::Number => Err(wrong_type(value)),
        Kind::Integer { choices } => {
            let integer = whole(value).map_err(|detail| mismatch("type", detail))?;
            *value = integer.into();
            match choices {
                Some(choices) if !choices.contains(&integer) => {
                    let allowed: Vec<String> = choices.iter().map(i64::to_string).collect();
                    Err(mismatch(
                        "enum",
                        format!("must be one of {}, got {integer}", allowed.join(", ")),
                    ))
                }
                _ => Ok(()),
            }
        }
        Kind::Array { items } => {
            let Json::Array(list) = value else {
                return Err(wrong_type(value));
            };
            list.iter_mut().enumerate().try_for_each(|(index, item)| {
                check(items, item, &format!("{name}[{index}]"), checked)
            })
        }
        Kind::Object(object) => {
            let Json::Object(fields) = value else {
                return Err(wrong_type(value));
            };
            check_fields(object, fields, name, checked)
        }
    }
}

/// Checks the `fields` of the object named `name` against `object`: each
/// declared field in name order, then the first undeclared field. A
/// declared field that is left out takes its default, and one with no
/// default is refused when the object requires it.
fn check_fields(
    object: &ObjectNode,
    fields: &mut Map<String, Json>,
    name: &str,
    checked: Checked,
) -> Result<(), Mismatch> {
    for (field, node) in &object.fields {
        let path = if name.is_empty() {
            field.clone()
        } else {
            format!("{name}.{field}")
        };
        let required = object.required.contains(field);
        // A null for a field that is not required is the field left out.
        if !required && fields.get(field).is_some_and(Json::is_null) {
            fields.remove(field);
        }
        let absent = |detail: &str| Mismatch {
            reason: "required",
            name: path.clone(),
            detail: detail.to_owned(),
        };
        match fields.get_mut(field) {
            Some(Json::Null) => return Err(absent("is required, got null")),
            Some(value) => check(node, value, &path, checked)?,
            None => match &node.default {
                Some(default) => {
                    fields.insert(field.clone(), default.clone());
                }
                None if required => return Err(absent("is required")),
                None => {}
            },
        }
    }
    if object.open {
        return Ok(());
    }
    match fields.keys().find(|key| !object.fields.contains_key(*key)) {
        Some(undeclared) => Err(Mismatch {
            reason: "undeclared",
            name: if name.is_empty() {
                undeclared.clone()
            } else {
                format!("{name}.{undeclared}")
            },
            detail: format!("is not declared; {}", declared(object, name, checked)),
        }),
        None => Ok(()),
    }
}

/// Names the fields the object named `name` declares, for the refusal of
/// an undeclared one. The object with the empty name is the root of the
/// declaration for a run's arguments, and the declaring node for a
/// default.
fn declared(object: &ObjectNode, name: &str, checked: Checked) -> String {
    let names: Vec<String> = object
        .fields
        .keys()
        .map(|field| format!("`{field}`"))
        .collect();
    let names = names.join(", ");
    match (name.is_empty(), names.is_empty(), checked) {
        (true, true, Checked::Arguments) => "the prompt declares no arguments".to_owned(),
        (true, false, Checked::Arguments) => format!("the declared arguments are {names}"),
        (true, true, Checked::Default) => "this node declares no fields".to_owned(),
        (true, false, Checked::Default) => format!("the declared fields of this node are {names}"),
        (false, true, _) => format!("`{name}` declares no fields"),
        (false, false, _) => format!("the declared fields of `{name}` are {names}"),
    }
}

/// The integer `value` holds: an integer as it is, and a number with no
/// fractional part that 64 bits hold as that integer. A number that is
/// not written as a 64-bit integer is read as a float, and one whose
/// float is at either end of the 64-bit range is refused, because digits
/// past the range round to that end. Otherwise the refusal's words after
/// the argument's name.
fn whole(value: &Json) -> Result<i64, String> {
    let Json::Number(number) = value else {
        return Err(format!("must be an integer, got {}", json_kind(value)));
    };
    if let Some(integer) = number.as_i64() {
        return Ok(integer);
    }
    match number.as_f64() {
        Some(float) if float.fract() != 0.0 => Err("must be an integer, got number".to_owned()),
        Some(float) if !number.is_u64() && float.abs() < ABOVE_INTEGERS => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the float is a whole number inside i64's range"
            )]
            let integer = float as i64;
            Ok(integer)
        }
        _ => Err(format!(
            "must be an integer that fits in 64 bits, got {number}"
        )),
    }
}

/// A refused string as a refusal shows it: quoted, and cut after
/// [`SHOWN_CHARACTERS`] characters.
fn shown(text: &str) -> String {
    let mut clipped: String = text.chars().take(SHOWN_CHARACTERS).collect();
    if clipped.len() < text.len() {
        clipped.push_str("...");
    }
    format!("{clipped:?}")
}

/// Names the JSON type of `value`, with `integer` for a number written
/// with no fraction and no exponent, and `number` for any other.
fn json_kind(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

/// Builds the failure of a run's arguments: `message` under `ArgsInvalid`,
/// with the `Reason` fact `reason` and, for one argument, its path as the
/// `Name` fact. The prose is the message and then the code's default.
fn make_args_error(reason: &str, name: Option<&str>, message: impl Into<String>) -> Error {
    let code = ErrorCode::ArgsInvalid;
    let message = message.into();
    let prose = format!("{message}. {}", code.prose());
    let mut error = Error::new(code, message, prose);
    error.facts.insert(ErrorFact::Reason, reason.into());
    if let Some(name) = name {
        error.facts.insert(ErrorFact::Name, name.into());
    }
    error
}
