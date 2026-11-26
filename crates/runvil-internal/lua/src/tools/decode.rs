//! Shared value decoding for the `tools.*` Engine functions.
//!
//! The id-or-tool-object polymorphism of `tools.call` lives here once: a
//! string holding a `/` is a tool id and a tool object stands for its id.
//! The short-form `parameters` conversion of `tools.adhoc` sits beside it
//! as the namespace's other argument decode.

use mlua::{Lua, LuaString, Table, Value};
use serde_json::{Value as Json, json};

use super::userdata::LuaToolHandle;
use crate::meta_names::MetaNames;
use crate::order::FirstRefusal;
use crate::proxy::{quoted, view_data_within};
use crate::walk_budget::{ReadCost, TooLarge, WalkBudget};

/// Resolves the tool a script's `tools.call` names to its id.
///
/// A string holding a `/` is the id verbatim, a tool object contributes
/// its id, and anything else is an argument error naming the accepted
/// forms.
///
/// # Errors
/// Returns an `mlua` external error when the value is neither a string
/// holding a `/` nor a tool object.
pub(crate) fn tool_id(value: &Value) -> mlua::Result<String> {
    match value {
        Value::String(text) => {
            let name = text.to_string_lossy();
            if name.contains('/') {
                Ok(name)
            } else {
                Err(mlua::Error::external(format!(
                    "tools.call: {name:?} is not a tool id"
                )))
            }
        }
        other => object_id(other).ok_or_else(|| {
            mlua::Error::external(format!(
                "tools.call takes a tool id or a tool object, got {}",
                other.type_name()
            ))
        }),
    }
}

/// Resolves the tool a model-issued call names: the wire name the model
/// sent, verbatim.
///
/// # Errors
/// Returns an `mlua` external error when the value is not a string.
pub(crate) fn model_tool_name(value: &Value) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        other => Err(mlua::Error::external(format!(
            "a model-issued tool call names a tool by its wire name, got {}",
            other.type_name()
        ))),
    }
}

/// The id of the tool object `value` is, or `None` for any other value,
/// a userdata that is not a tool object included.
fn object_id(value: &Value) -> Option<String> {
    match value {
        Value::UserData(object) => object
            .borrow::<LuaToolHandle>()
            .ok()
            .map(|handle| handle.id().to_string()),
        _ => None,
    }
}

/// Builds the JSON Schema `parameters` object from the short form of a
/// `tools.adhoc` `parameters` table. Each key is a parameter's name, a
/// string, and each value is a bare type string or a `{type, description}`
/// array; every declared parameter is required.
///
/// A weak array is refused, whatever it holds: the collector can take its
/// description, and the array would then be one of the two forms in one
/// run and not in the next. The caller refuses a weak `params`.
///
/// A key that is a number is refused. Lua would read it as a string, and
/// the keys `1` and `'1'` would then name one parameter, whose type would
/// be that of the one Lua's own walk met last.
///
/// One [`WalkBudget`] bounds the whole conversion, which is one Rust call
/// that no instruction hook interrupts. Each parameter costs a value, each
/// view on the way to its value costs what [`WalkBudget::charge_view`]
/// takes, each `__mode` scanned on that way costs its bytes, and each
/// name, type, and description costs its bytes before it is read. A
/// description that many parameters share is copied once for each, and a
/// chain of views that many parameters share is passed once for each, so
/// the schema and the work can be far larger than the Lua heap that holds
/// the table. What an entry costs does not depend on the entries read
/// before it, so a table is too large in every order of Lua's own walk or
/// in none.
///
/// The first refusal drops the schema built so far, and nothing is copied
/// after it: a table to refuse costs the reads and no more. A refusal
/// quotes a name and a type as [`quoted`] gives them, so its size does not
/// follow theirs.
///
/// `properties` is a `serde_json::Map`, a `BTreeMap` while `serde_json`'s
/// `preserve_order` is off, so only `required` needs the sort below.
///
/// # Errors
/// Refuses a table past the budget. Refuses a key that is not a UTF-8
/// string, a value that is a weak table, and a value that is not one of
/// the two forms: of several such entries, the one whose key comes first
/// in the shared order of keys.
pub(super) fn adhoc_params_schema(lua: &Lua, params: &Table) -> mlua::Result<Json> {
    let names = MetaNames::default();
    let mut budget = WalkBudget::for_vm(lua);
    let mut properties = Some(serde_json::Map::new());
    let mut refused = FirstRefusal::default();
    for pair in params.pairs::<Value, Value>() {
        let (key, spec) = pair?;
        budget.charge_node().map_err(too_large)?;
        match adhoc_param(lua, &names, &mut budget, &key, &spec)? {
            Ok(param) => {
                if let Some(properties) = &mut properties {
                    properties.insert(param.name.to_string_lossy(), param.schema());
                }
            }
            Err(refusal) => {
                properties = None;
                refused.refuse(&key, refusal);
            }
        }
    }
    refused.into_result()?;
    let properties = properties.unwrap_or_default();
    // Bytewise sort, matching the shared `SortKey::Text` order, so the
    // generated schema is the same text in every process.
    let mut required = properties.keys().cloned().collect::<Vec<_>>();
    required.sort();
    Ok(json!({
        "type": "object",
        "properties": properties,
        "required": required,
    }))
}

/// What the short form says when its conversion goes past the budget.
const PARAMETERS_TOO_LARGE: &str = "tools.adhoc: parameters are too large to convert";

/// The four types a parameter of the short form can have.
const PARAMETER_TYPES: [&str; 4] = ["string", "integer", "number", "boolean"];

/// One parameter of the short form, read and not yet copied: the name and
/// the description are the table's own strings, both UTF-8.
struct AdhocParam {
    name: LuaString,
    ty: &'static str,
    description: Option<LuaString>,
}

impl AdhocParam {
    /// The parameter's JSON Schema, with a copy of its description.
    fn schema(&self) -> Json {
        let mut property = json!({ "type": self.ty });
        if let Some(description) = &self.description {
            property["description"] = Json::String(description.to_string_lossy());
        }
        property
    }
}

/// The refusal of a conversion past its budget.
fn too_large(_: TooLarge) -> mlua::Error {
    mlua::Error::external(PARAMETERS_TOO_LARGE)
}

/// One parameter of the short form, or the refusal of its entry.
///
/// # Errors
/// Returns the failure of a spent budget or of a read, which ends the
/// conversion, where a refusal waits for the entries left to read.
fn adhoc_param(
    lua: &Lua,
    names: &MetaNames,
    budget: &mut WalkBudget,
    key: &Value,
    spec: &Value,
) -> mlua::Result<Result<AdhocParam, mlua::Error>> {
    let Value::String(name) = key else {
        return Ok(Err(mlua::Error::external(format!(
            "tools.adhoc: a parameter's name must be a string, got {}",
            key.type_name()
        ))));
    };
    budget
        .charge_bytes(name.as_bytes().len())
        .map_err(too_large)?;
    if let Err(error) = name.to_str() {
        return Ok(Err(error));
    }
    let shown = || quoted(&name.as_bytes());
    let (ty, description) = match declared(lua, names, budget, spec)? {
        Declared::Typed(ty, description) => (ty, description),
        Declared::Weak(weakness) => {
            return Ok(Err(mlua::Error::external(format!(
                "tools.adhoc: param {:?} is {weakness}",
                shown()
            ))));
        }
        Declared::Other => {
            return Ok(Err(mlua::Error::external(format!(
                "tools.adhoc: param {:?} must be a type string or a {{type, description}} array",
                shown()
            ))));
        }
    };
    budget
        .charge_bytes(ty.as_bytes().len())
        .map_err(too_large)?;
    let known = PARAMETER_TYPES
        .into_iter()
        .find(|known| ty.as_bytes() == known.as_bytes());
    let Some(ty) = known else {
        return Ok(Err(mlua::Error::external(format!(
            "tools.adhoc: param {:?} has unsupported type {:?}: \
             expected \"string\", \"integer\", \"number\", or \"boolean\"",
            shown(),
            quoted(&ty.as_bytes())
        ))));
    };
    if let Some(description) = &description {
        budget
            .charge_bytes(description.as_bytes().len())
            .map_err(too_large)?;
        if let Err(error) = description.to_str() {
            return Ok(Err(error));
        }
    }
    Ok(Ok(AdhocParam {
        name: name.clone(),
        ty,
        description,
    }))
}

/// What the value of a parameter declares.
enum Declared {
    /// One of the two forms, a type string or an array of a type string
    /// and an optional description string: the type, and the description.
    Typed(LuaString, Option<LuaString>),
    /// Nothing that is the same in two runs: the value is a weak table, a
    /// view of one or through one, or a table whose metatable is weak.
    /// This is what a refusal says of it.
    Weak(&'static str),
    /// Neither form.
    Other,
}

/// What the parameter's value `spec` declares.
///
/// An array is read raw, and a sealed view as the table it stands for, so
/// the read runs no code of the author. The parameters are read in the
/// order of Lua's own walk, which differs from one run to the next, and a
/// metamethod could tell that order. Each view passed is charged to
/// `budget`, and so are the bytes of each `__mode` the read scans.
fn declared(
    lua: &Lua,
    names: &MetaNames,
    budget: &mut WalkBudget,
    spec: &Value,
) -> mlua::Result<Declared> {
    let spec = match spec {
        Value::String(ty) => return Ok(Declared::Typed(ty.clone(), None)),
        Value::Table(spec) => view_data_within(lua, names, spec, |cost| {
            match cost {
                ReadCost::View => budget.charge_view(),
                ReadCost::Mode(bytes) => budget.charge_bytes(bytes),
            }
            .map_err(too_large)
        })?,
        _ => return Ok(Declared::Other),
    };
    let spec = match spec {
        Ok(spec) => spec,
        Err(weakness) => return Ok(Declared::Weak(weakness)),
    };
    Ok(
        match (spec.raw_get::<Value>(1)?, spec.raw_get::<Value>(2)?) {
            (Value::String(ty), Value::Nil) => Declared::Typed(ty, None),
            (Value::String(ty), Value::String(description)) => {
                Declared::Typed(ty, Some(description))
            }
            _ => Declared::Other,
        },
    )
}
