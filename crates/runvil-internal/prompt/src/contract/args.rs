//! Parses the `args` frontmatter key: the shape of the one JSON object a
//! caller passes the prompt, declared as a subset of JSON Schema.
//!
//! A declaration is a tree of nodes. Every node has a `type`, one of
//! `string`, `boolean`, `integer`, `number`, `array`, and `object`, and the
//! root is an `object`. The other keywords are `description` on any node,
//! `default` on a field's node, `enum` on a `string` or an `integer`,
//! `items` on an `array`, and `properties`, `required`, and
//! `additionalProperties` on an `object`. Any other keyword, a keyword on a
//! type it does not apply to, and a keyword written twice fail the parse,
//! so a misspelled keyword is never read as an ignored one. An object takes
//! only the fields its `properties` declare, unless it says
//! `additionalProperties: true`.
//!
//! The parse is a streaming walk, so a refusal keeps the YAML path and the
//! position of the keyword or node it is about. A prompt with no `args:`
//! key gets the default declaration: one optional string named `text`. The
//! run's first step checks the caller's arguments against the declaration
//! ([`super::args_check`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value as Json};
use serde_yaml_ng::Value as Yaml;

use super::args_check;

/// The name of the one argument the default declaration holds.
pub(crate) const TEXT_FIELD: &str = "text";

/// The description of the default declaration's one argument.
const TEXT_DESCRIPTION: &str = "Freeform text for this prompt";

/// Every keyword a node may hold, as a refusal lists them.
const KEYWORDS: &str = "`type`, `properties`, `required`, `additionalProperties`, `items`, \
                        `enum`, `description`, and `default`";

/// The arguments a prompt declares: the shape of the one JSON object its
/// caller passes it.
///
/// Every prompt has a declaration. A prompt whose frontmatter omits the
/// `args` key gets the default one, an object with one optional string
/// named `text`, and a written copy of the default equals it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsSchema {
    /// The root node, whose kind is an object.
    root: Node,
    /// The declaration as plain JSON Schema, built once from the root.
    json: Json,
}

impl ArgsSchema {
    /// Returns the declaration as plain JSON Schema, ready to publish as a
    /// tool's input schema.
    ///
    /// The root is `type: object`. Every object lists its `properties`,
    /// lists `required` when it requires a field, and says
    /// `additionalProperties: false` unless it takes fields it does not
    /// declare.
    #[must_use]
    pub fn as_json(&self) -> &serde_json::Value {
        &self.json
    }

    /// The root node.
    pub(super) fn root(&self) -> &Node {
        &self.root
    }

    /// The declaration whose root is `root`.
    fn from_root(root: Node) -> ArgsSchema {
        let json = root.to_json();
        ArgsSchema { root, json }
    }
}

impl Default for ArgsSchema {
    fn default() -> Self {
        let text = Node {
            kind: Kind::String { choices: None },
            description: Some(TEXT_DESCRIPTION.to_owned()),
            default: None,
        };
        ArgsSchema::from_root(Node {
            kind: Kind::Object(ObjectNode {
                fields: BTreeMap::from([(TEXT_FIELD.to_owned(), text)]),
                required: BTreeSet::new(),
                open: false,
            }),
            description: None,
            default: None,
        })
    }
}

impl<'de> Deserialize<'de> for ArgsSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        NodeSeed(Place::Root)
            .deserialize(deserializer)
            .map(ArgsSchema::from_root)
    }
}

/// One node of a declaration: its type with what that type declares, its
/// description, and its default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    /// The node's type and the keywords that belong to it.
    pub(super) kind: Kind,
    /// The human-readable description.
    description: Option<String>,
    /// The value a field of this node takes when the caller leaves it
    /// out. It matches the node, and its own missing fields hold their
    /// defaults.
    pub(super) default: Option<Json>,
}

/// A node's type, with the keywords that belong to that type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Kind {
    /// A JSON string, one of `choices` when the node has an `enum`.
    String {
        /// The strings the node's `enum` lists.
        choices: Option<Vec<String>>,
    },
    /// JSON `true` or `false`.
    Boolean,
    /// A JSON number with no fractional part that fits in 64 bits, one of
    /// `choices` when the node has an `enum`.
    Integer {
        /// The integers the node's `enum` lists.
        choices: Option<Vec<i64>>,
    },
    /// Any JSON number.
    Number,
    /// A JSON array whose every item matches `items`.
    Array {
        /// The node of each item.
        items: Box<Node>,
    },
    /// A JSON object.
    Object(ObjectNode),
}

impl Kind {
    /// The word a declaration writes under `type`.
    fn word(&self) -> &'static str {
        match self {
            Kind::String { .. } => "string",
            Kind::Boolean => "boolean",
            Kind::Integer { .. } => "integer",
            Kind::Number => "number",
            Kind::Array { .. } => "array",
            Kind::Object(_) => "object",
        }
    }

    /// The type with its article, as a refusal names what a value must be.
    pub(super) fn expected(&self) -> &'static str {
        match self {
            Kind::String { .. } => "a string",
            Kind::Boolean => "a boolean",
            Kind::Integer { .. } => "an integer",
            Kind::Number => "a number",
            Kind::Array { .. } => "an array",
            Kind::Object(_) => "an object",
        }
    }
}

/// What an `object` node declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectNode {
    /// The declared fields, by name.
    pub(super) fields: BTreeMap<String, Node>,
    /// The names of the fields a caller must send.
    pub(super) required: BTreeSet<String>,
    /// Whether the object also takes fields it does not declare.
    pub(super) open: bool,
}

impl Node {
    /// The node as plain JSON Schema.
    fn to_json(&self) -> Json {
        let mut schema = Map::new();
        schema.insert("type".to_owned(), self.kind.word().into());
        if let Some(description) = &self.description {
            schema.insert("description".to_owned(), description.clone().into());
        }
        if let Some(default) = &self.default {
            schema.insert("default".to_owned(), default.clone());
        }
        match &self.kind {
            Kind::String {
                choices: Some(choices),
            } => {
                schema.insert("enum".to_owned(), choices.clone().into());
            }
            Kind::Integer {
                choices: Some(choices),
            } => {
                schema.insert("enum".to_owned(), choices.clone().into());
            }
            Kind::Array { items } => {
                schema.insert("items".to_owned(), items.to_json());
            }
            Kind::Object(object) => {
                let properties = object
                    .fields
                    .iter()
                    .map(|(name, node)| (name.clone(), node.to_json()))
                    .collect::<Map<String, Json>>();
                schema.insert("properties".to_owned(), Json::Object(properties));
                if !object.required.is_empty() {
                    let required: Vec<&str> = object.required.iter().map(String::as_str).collect();
                    schema.insert("required".to_owned(), required.into());
                }
                schema.insert("additionalProperties".to_owned(), object.open.into());
            }
            Kind::String { choices: None }
            | Kind::Integer { choices: None }
            | Kind::Boolean
            | Kind::Number => {}
        }
        Json::Object(schema)
    }
}

/// A field's node, which the `properties` map deserializes.
impl<'de> Deserialize<'de> for Node {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        NodeSeed(Place::Field).deserialize(deserializer)
    }
}

/// Where a node sits in a declaration, which decides whether it takes a
/// `default` and which type it must have.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The root of the declaration.
    Root,
    /// A field under `properties`.
    Field,
    /// The node under `items`.
    Items,
}

/// Deserializes the node at one [`Place`].
struct NodeSeed(Place);

impl<'de> DeserializeSeed<'de> for NodeSeed {
    type Value = Node;

    fn deserialize<D>(self, deserializer: D) -> Result<Node, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(NodeVisitor(self.0))
    }
}

/// The streaming walk over one node's keywords.
struct NodeVisitor(Place);

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map of schema keywords")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Node, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut raw = RawNode::default();
        while let Some(keyword) = map.next_key::<Keyword>()? {
            raw.read(keyword, &mut map)?;
        }
        raw.into_node(self.0).map_err(de::Error::custom)
    }
}

/// A keyword of a node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    Type,
    Properties,
    Required,
    AdditionalProperties,
    Items,
    Enum,
    Description,
    Default,
}

impl Keyword {
    /// The keyword as a declaration writes it.
    fn word(self) -> &'static str {
        match self {
            Keyword::Type => "type",
            Keyword::Properties => "properties",
            Keyword::Required => "required",
            Keyword::AdditionalProperties => "additionalProperties",
            Keyword::Items => "items",
            Keyword::Enum => "enum",
            Keyword::Description => "description",
            Keyword::Default => "default",
        }
    }
}

impl<'de> Deserialize<'de> for Keyword {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(KeywordVisitor)
    }
}

/// Reads one map key as a [`Keyword`], refusing any other key where it
/// stands, so the refusal has the key's own line and column.
struct KeywordVisitor;

impl Visitor<'_> for KeywordVisitor {
    type Value = Keyword;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a schema keyword")
    }

    fn visit_str<E>(self, text: &str) -> Result<Keyword, E>
    where
        E: de::Error,
    {
        match text {
            "type" => Ok(Keyword::Type),
            "properties" => Ok(Keyword::Properties),
            "required" => Ok(Keyword::Required),
            "additionalProperties" => Ok(Keyword::AdditionalProperties),
            "items" => Ok(Keyword::Items),
            "enum" => Ok(Keyword::Enum),
            "description" => Ok(Keyword::Description),
            "default" => Ok(Keyword::Default),
            other => Err(E::custom(format!(
                "unknown keyword {other:?}; the keywords are {KEYWORDS}"
            ))),
        }
    }
}

/// The word under a node's `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeWord {
    String,
    Boolean,
    Integer,
    Number,
    Array,
    Object,
}

/// The six type words, in the order a refusal lists them.
const TYPE_WORDS: &[&str] = &["string", "boolean", "integer", "number", "array", "object"];

impl<'de> Deserialize<'de> for TypeWord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(TypeWordVisitor)
    }
}

/// Reads the value of `type` as one [`TypeWord`], refusing any other word
/// and any value that is not one word, such as a list of words.
struct TypeWordVisitor;

impl Visitor<'_> for TypeWordVisitor {
    type Value = TypeWord;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "one type word: `string`, `boolean`, `integer`, `number`, `array`, or `object`",
        )
    }

    fn visit_str<E>(self, text: &str) -> Result<TypeWord, E>
    where
        E: de::Error,
    {
        match text {
            "string" => Ok(TypeWord::String),
            "boolean" => Ok(TypeWord::Boolean),
            "integer" => Ok(TypeWord::Integer),
            "number" => Ok(TypeWord::Number),
            "array" => Ok(TypeWord::Array),
            "object" => Ok(TypeWord::Object),
            other => Err(E::unknown_variant(other, TYPE_WORDS)),
        }
    }
}

impl TypeWord {
    /// The word as a declaration writes it.
    fn word(self) -> &'static str {
        match self {
            TypeWord::String => "string",
            TypeWord::Boolean => "boolean",
            TypeWord::Integer => "integer",
            TypeWord::Number => "number",
            TypeWord::Array => "array",
            TypeWord::Object => "object",
        }
    }
}

/// The fields under `properties`: a map from each argument name, checked
/// against the name grammar, to its node, with no name declared twice.
struct Fields(BTreeMap<String, Node>);

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::deserialize_contract_map(deserializer, "argument name").map(Fields)
    }
}

/// The keywords one node wrote, as read, before they are checked against
/// each other.
#[derive(Default)]
struct RawNode {
    kind: Option<TypeWord>,
    fields: Option<BTreeMap<String, Node>>,
    required: Option<Vec<String>>,
    open: Option<bool>,
    items: Option<Node>,
    choices: Option<Vec<Yaml>>,
    description: Option<String>,
    default: Option<Yaml>,
}

impl RawNode {
    /// Reads the value of `keyword` from `map`, refusing a keyword the
    /// node already holds.
    fn read<'de, A>(&mut self, keyword: Keyword, map: &mut A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        let written = match keyword {
            Keyword::Type => self.kind.is_some(),
            Keyword::Properties => self.fields.is_some(),
            Keyword::Required => self.required.is_some(),
            Keyword::AdditionalProperties => self.open.is_some(),
            Keyword::Items => self.items.is_some(),
            Keyword::Enum => self.choices.is_some(),
            Keyword::Description => self.description.is_some(),
            Keyword::Default => self.default.is_some(),
        };
        if written {
            return Err(de::Error::custom(format!(
                "keyword `{}` is written twice",
                keyword.word()
            )));
        }
        match keyword {
            Keyword::Type => self.kind = Some(map.next_value()?),
            Keyword::Properties => self.fields = Some(map.next_value::<Fields>()?.0),
            Keyword::Required => self.required = Some(map.next_value()?),
            Keyword::AdditionalProperties => self.open = Some(map.next_value()?),
            Keyword::Items => self.items = Some(map.next_value_seed(NodeSeed(Place::Items))?),
            Keyword::Enum => self.choices = Some(map.next_value()?),
            Keyword::Description => self.description = Some(map.next_value()?),
            Keyword::Default => self.default = Some(map.next_value()?),
        }
        Ok(())
    }

    /// Checks the keywords against each other and against the node's
    /// `place`, and builds the node. The refusal is the text after the
    /// node's path.
    fn into_node(self, place: Place) -> Result<Node, String> {
        let Some(kind) = self.kind else {
            return Err(
                "a node needs a `type`: `string`, `boolean`, `integer`, `number`, `array`, or \
                 `object`"
                    .to_owned(),
            );
        };
        if place == Place::Root && kind != TypeWord::Object {
            return Err(format!(
                "the root of `args` must be `type: object`, and it is `{}`",
                kind.word()
            ));
        }
        if let Some((keyword, applies)) = self.misplaced(kind) {
            return Err(format!(
                "`{keyword}` applies to {applies}, and this node is `{}`",
                kind.word()
            ));
        }
        match (place, &self.default) {
            (Place::Root, Some(_)) => {
                return Err("the root of `args` takes no `default`".to_owned());
            }
            (Place::Items, Some(_)) => return Err("a list item takes no `default`".to_owned()),
            _ => {}
        }
        let kind = match kind {
            TypeWord::String => Kind::String {
                choices: self.choices.as_deref().map(string_choices).transpose()?,
            },
            TypeWord::Integer => Kind::Integer {
                choices: self.choices.as_deref().map(integer_choices).transpose()?,
            },
            TypeWord::Boolean => Kind::Boolean,
            TypeWord::Number => Kind::Number,
            TypeWord::Array => Kind::Array {
                items: Box::new(self.items.ok_or("an `array` node needs `items`")?),
            },
            TypeWord::Object => Kind::Object(object_node(self.fields, self.required, self.open)?),
        };
        let mut node = Node {
            kind,
            description: self.description,
            default: None,
        };
        if let Some(default) = &self.default {
            node.default = default_of(&node, default)?;
        }
        Ok(node)
    }

    /// The first keyword the node holds that does not apply to `kind`,
    /// with the nodes it applies to.
    fn misplaced(&self, kind: TypeWord) -> Option<(&'static str, &'static str)> {
        let object = kind == TypeWord::Object;
        let listed = matches!(kind, TypeWord::String | TypeWord::Integer);
        [
            (
                self.items.is_some() && kind != TypeWord::Array,
                "items",
                "an `array` node",
            ),
            (
                self.fields.is_some() && !object,
                "properties",
                "an `object` node",
            ),
            (
                self.required.is_some() && !object,
                "required",
                "an `object` node",
            ),
            (
                self.open.is_some() && !object,
                "additionalProperties",
                "an `object` node",
            ),
            (
                self.choices.is_some() && !listed,
                "enum",
                "a `string` or an `integer` node",
            ),
        ]
        .into_iter()
        .find(|(misplaced, ..)| *misplaced)
        .map(|(_, keyword, applies)| (keyword, applies))
    }
}

/// What an `object` node declares, from its three keywords: `required`
/// names only declared fields, each once.
fn object_node(
    fields: Option<BTreeMap<String, Node>>,
    required: Option<Vec<String>>,
    open: Option<bool>,
) -> Result<ObjectNode, String> {
    let fields = fields.unwrap_or_default();
    let mut names = BTreeSet::new();
    for name in required.unwrap_or_default() {
        if !fields.contains_key(&name) {
            return Err(format!(
                "`required` names `{name}`, which `properties` does not declare"
            ));
        }
        if names.contains(&name) {
            return Err(format!("`required` names `{name}` twice"));
        }
        names.insert(name);
    }
    Ok(ObjectNode {
        fields,
        required: names,
        open: open.unwrap_or(false),
    })
}

/// The strings a `string` node's `enum` lists: at least one, each once.
fn string_choices(listed: &[Yaml]) -> Result<Vec<String>, String> {
    let mut choices: Vec<String> = Vec::with_capacity(listed.len());
    for value in listed {
        let Yaml::String(text) = value else {
            return Err(format!(
                "the `enum` of a `string` node lists strings, and it lists {}",
                yaml_kind(value)
            ));
        };
        if choices.contains(text) {
            return Err(format!("`enum` lists {text:?} twice"));
        }
        choices.push(text.clone());
    }
    if choices.is_empty() {
        return Err("`enum` lists no value".to_owned());
    }
    Ok(choices)
}

/// The integers an `integer` node's `enum` lists: at least one, each once.
fn integer_choices(listed: &[Yaml]) -> Result<Vec<i64>, String> {
    let mut choices: Vec<i64> = Vec::with_capacity(listed.len());
    for value in listed {
        let Some(integer) = value.as_i64() else {
            return Err(match value.as_u64() {
                Some(large) => format!(
                    "the `enum` of an `integer` node lists integers that fit in 64 bits, and \
                     it lists {large}"
                ),
                None => format!(
                    "the `enum` of an `integer` node lists integers, and it lists {}",
                    yaml_kind(value)
                ),
            });
        };
        if choices.contains(&integer) {
            return Err(format!("`enum` lists {integer} twice"));
        }
        choices.push(integer);
    }
    if choices.is_empty() {
        return Err("`enum` lists no value".to_owned());
    }
    Ok(choices)
}

/// The default `node` declares: `written` as JSON, matched against the
/// node, with its own missing fields filled. A YAML null declares none.
fn default_of(node: &Node, written: &Yaml) -> Result<Option<Json>, String> {
    let mut default = yaml_to_json(written).map_err(|why| format!("the default holds {why}"))?;
    if default.is_null() {
        return Ok(None);
    }
    args_check::check(node, &mut default, "", args_check::Checked::Default)
        .map_err(|mismatch| mismatch.of_default())
        .map(|()| Some(default))
}

/// `value` as JSON, or what in it has no JSON form: a number that is not
/// finite, a key that is not a string, or a tagged value.
fn yaml_to_json(value: &Yaml) -> Result<Json, &'static str> {
    match value {
        Yaml::Null => Ok(Json::Null),
        Yaml::Bool(flag) => Ok(Json::Bool(*flag)),
        Yaml::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(integer.into())
            } else if let Some(integer) = number.as_u64() {
                Ok(integer.into())
            } else {
                number
                    .as_f64()
                    .and_then(serde_json::Number::from_f64)
                    .map(Json::Number)
                    .ok_or("a number that is not finite")
            }
        }
        Yaml::String(text) => Ok(Json::String(text.clone())),
        Yaml::Sequence(items) => items.iter().map(yaml_to_json).collect(),
        Yaml::Mapping(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                let Yaml::String(name) = key else {
                    return Err("a key that is not a string");
                };
                object.insert(name.clone(), yaml_to_json(value)?);
            }
            Ok(Json::Object(object))
        }
        Yaml::Tagged(_) => Err("a tagged value"),
    }
}

/// Names the YAML kind of `value`, with its article, for a refusal.
fn yaml_kind(value: &Yaml) -> &'static str {
    match value {
        Yaml::Null => "a null",
        Yaml::Bool(_) => "a boolean",
        Yaml::Number(number) if number.is_i64() || number.is_u64() => "an integer",
        Yaml::Number(_) => "a number",
        Yaml::String(_) => "a string",
        Yaml::Sequence(_) => "a sequence",
        Yaml::Mapping(_) => "a mapping",
        Yaml::Tagged(_) => "a tagged value",
    }
}
