//! The JSON data of a Lua value and of JSON text, with every key of a
//! table or an object read as a name.
//!
//! `serde_json::Value` keeps one key of a map for itself in a build that
//! has `serde_json`'s `raw_value` feature on, which any crate of the build
//! can turn on: a map whose first key is `$serde_json::private::RawValue`
//! is read as the JSON text under that key. `mlua`'s `from_value` hands a
//! table that is no list to the type it converts to as a map, and Lua's
//! own walk says which key of a table comes first, so a table with that
//! key and one more would be the text's data in one run and a failure in
//! the next. A parse of JSON text hands an object over the same way. In
//! both, the text under the key is parsed with a nesting limit of its own,
//! so it nests as deep as it likes under what holds it, and a text of
//! 1.5 KB that holds such a key four times over ended the process with a
//! stack overflow. [`PlainJson`] reads what `serde_json::Value` reads,
//! without that rule, so the key is a name like any other with the
//! feature on and with it off.
//!
//! This holds in a build without `serde_json`'s `arbitrary_precision`
//! feature, which no crate of the workspace turns on. With it,
//! `serde_json` hands a number of JSON text over as a map of one key, and
//! [`PlainJson`] reads that map as an object.

use std::fmt;

use mlua::{Lua, LuaSerdeExt, Value};
use serde::de::{Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value as Json};

/// The JSON data `from_value` makes of `value`.
///
/// # Errors
/// Returns the failure of `from_value` for a value with no JSON form.
pub(crate) fn json_of(lua: &Lua, value: Value) -> mlua::Result<Json> {
    lua.from_value::<PlainJson>(value)
        .map(|PlainJson(json)| json)
}

/// The JSON data of the one document `text` holds.
///
/// # Errors
/// Returns `serde_json`'s failure for a text that is not one JSON document.
pub(crate) fn json_of_text(text: &[u8]) -> serde_json::Result<Json> {
    serde_json::from_slice::<PlainJson>(text).map(|PlainJson(json)| json)
}

/// The JSON data of every document `text` holds, in order, with any
/// whitespace between two of them.
///
/// # Errors
/// Returns `serde_json`'s failure for the first part of `text` that is no
/// JSON document.
pub(crate) fn json_of_each_text(text: &[u8]) -> serde_json::Result<Vec<Json>> {
    serde_json::Deserializer::from_slice(text)
        .into_iter::<PlainJson>()
        .map(|document| document.map(|PlainJson(json)| json))
        .collect()
}

/// JSON data that reads every key of a map as a name.
struct PlainJson(Json);

impl<'de> Deserialize<'de> for PlainJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor).map(PlainJson)
    }
}

/// Reads one value as `serde_json::Value` reads it, in the same words.
struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
        Ok(Json::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
        Ok(Json::from(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Json, E> {
        Ok(Number::from_f64(value).map_or(Json::Null, Json::Number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Json, E> {
        Ok(Json::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Json, E> {
        Ok(Json::String(value))
    }

    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Json, A::Error> {
        let mut list = Vec::new();
        while let Some(PlainJson(item)) = items.next_element()? {
            list.push(item);
        }
        Ok(Json::Array(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Json, A::Error> {
        let mut object = Map::new();
        while let Some(name) = entries.next_key_seed(Name)? {
            let PlainJson(value) = entries.next_value()?;
            object.insert(name, value);
        }
        Ok(Json::Object(object))
    }
}

/// Reads the key of a map, which is a string, in the words
/// `serde_json::Value` has for the first key of one.
struct Name;

impl<'de> DeserializeSeed<'de> for Name {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for Name {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string key")
    }

    fn visit_str<E>(self, name: &str) -> Result<String, E> {
        Ok(name.to_owned())
    }

    fn visit_string<E>(self, name: String) -> Result<String, E> {
        Ok(name)
    }
}
