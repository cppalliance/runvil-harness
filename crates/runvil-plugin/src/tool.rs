use std::fmt;
use std::future::Future;
use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode};
use runvil_types::future::BoxFuture;
use runvil_types::packs::PackId;
use runvil_types::tools::{OutputTrust, ToolId, ToolInfo, ToolOutput, make_tool_error};
use schemars::generate::SchemaSettings;
use schemars::transform::{RestrictFormats, transform_subschemas};
use schemars::{JsonSchema, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Pack, PackFactory, ToolContext};

/// One tool of a Pack whose arguments and output are Rust types.
pub trait Tool: Send + Sync + 'static {
    /// The tool's name within its Pack, the last segment of its id.
    const NAME: &'static str;
    /// The one-sentence description the model reads.
    const DESCRIPTION: &'static str;
    /// Whether every output is trusted or untrusted.
    /// A script that calls a trusted tool receives the output as data.
    const TRUST: OutputTrust;
    /// The arguments of one call.
    /// Their JSON Schema is the tool's parameter schema.
    type Args: DeserializeOwned + JsonSchema + Send;
    /// What one call returns, which becomes the JSON text of the output.
    type Output: Serialize;

    /// Performs one tool call.
    fn call(
        &self,
        cx: ToolContext<'_>,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Error>> + Send;
}

/// A [`PackFactory`] whose Pack offers each [`Tool`] added to it.
#[derive(Default)]
pub struct Tools {
    tools: Vec<Arc<dyn Erased>>,
}

impl Tools {
    /// Adds `tool` to the Pack.
    #[must_use]
    pub fn with(mut self, tool: impl Tool) -> Tools {
        self.tools.push(Arc::new(tool));
        self
    }
}

impl fmt::Debug for Tools {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self
            .tools
            .iter()
            .map(|tool| tool.name())
            .collect::<Vec<_>>();
        f.debug_struct("Tools").field("tools", &names).finish()
    }
}

impl PackFactory for Tools {
    fn new_pack(&self, id: &PackId) -> Result<Arc<dyn Pack>, Error> {
        let tools = self
            .tools
            .iter()
            .map(|tool| Ok((tool.info(id)?, Arc::clone(tool))))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Arc::new(Instance { tools }))
    }
}

trait Erased: Send + Sync {
    fn name(&self) -> &'static str;

    fn info(&self, pack: &PackId) -> Result<ToolInfo, Error>;

    fn run<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>>;
}

impl<T: Tool> Erased for T {
    fn name(&self) -> &'static str {
        T::NAME
    }

    fn info(&self, pack: &PackId) -> Result<ToolInfo, Error> {
        let id = ToolId::parse(&format!("{pack}/{}", T::NAME))?;
        let info = ToolInfo::new(id, T::DESCRIPTION, parameters::<T::Args>());
        Ok(info.structured(T::TRUST == OutputTrust::Trusted))
    }

    fn run<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            if !args.is_object() {
                return Err(make_tool_error(
                    ErrorCode::ToolInvalidArguments,
                    "invalid arguments: expected an object",
                ));
            }
            let args = serde_json::from_value::<T::Args>(args).map_err(|error| {
                make_tool_error(
                    ErrorCode::ToolInvalidArguments,
                    format!("invalid arguments: {error}"),
                )
            })?;
            let output = self.call(cx, args).await?;
            let text = serde_json::to_value(output)
                .map_err(|error| {
                    make_tool_error(
                        ErrorCode::ToolOther,
                        "the output could not be rendered as JSON",
                    )
                    .with_cause(&error)
                })?
                .to_string();
            Ok(if T::TRUST == OutputTrust::Trusted {
                ToolOutput::trusted(text)
            } else {
                ToolOutput::untrusted(text)
            })
        })
    }
}

struct Instance {
    tools: Vec<(ToolInfo, Arc<dyn Erased>)>,
}

impl Pack for Instance {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.iter().map(|(info, _)| info.clone()).collect()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        let called = cx.tool();
        let Some((_, tool)) = self.tools.iter().find(|(info, _)| info.id == *called) else {
            let text = format!("{} has no tool named {}", called.pack(), called.name());
            return Box::pin(async move { Err(make_tool_error(ErrorCode::ToolOther, text)) });
        };
        tool.run(cx, args)
    }
}

fn parameters<Args: JsonSchema>() -> Value {
    let mut schema = SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .with_transform(RestrictFormats::default())
        .with_transform(omit_null)
        .into_generator()
        .into_root_schema_for::<Args>();
    schema.remove("$schema");
    schema.remove("title");
    schema.to_value()
}

fn omit_null(schema: &mut Schema) {
    transform_subschemas(&mut omit_null, schema);
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(Value::Object(properties)) = schema.get_mut("properties") else {
        return;
    };
    for (name, property) in properties {
        if required.iter().any(|required| required == name) {
            continue;
        }
        if let Some(Value::Array(kinds)) = property.get_mut("type") {
            kinds.retain(|kind| kind != "null");
            if kinds.len() == 1 {
                property["type"] = kinds.remove(0);
            }
        }
        for key in ["enum", "anyOf"] {
            if let Some(Value::Array(choices)) = property.get_mut(key) {
                choices.retain(|choice| !choice.is_null() && choice["type"] != "null");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use runvil_types::tools::{ToolCallOrigin, ToolCaller};
    use runvil_vfs::{Origin, VfsRef};
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[test]
    fn a_pack_lists_its_typed_tools_and_answers_their_calls() {
        let pack = pack();

        assert_eq!(
            pack.tools(),
            [
                ToolInfo::new(
                    ToolId::parse("demo/scale").unwrap(),
                    "Multiply a number.",
                    json!({
                        "type": "object",
                        "properties": {
                            "number": { "type": "integer" },
                            "by": { "type": "integer", "description": "The factor." }
                        },
                        "required": ["number", "by"]
                    }),
                )
                .structured(true),
                ToolInfo::new(
                    ToolId::parse("demo/shout").unwrap(),
                    "Uppercase a text.",
                    json!({
                        "type": "object",
                        "properties": { "text": { "type": "string" } },
                        "required": ["text"]
                    }),
                ),
            ]
        );
        assert_eq!(
            call(&*pack, "demo/scale", json!({ "number": 14, "by": 3 })),
            Ok(ToolOutput::trusted(r#"{"product":42,"scaled":14}"#))
        );
        assert_eq!(
            call(&*pack, "demo/shout", json!({ "text": "quiet" })),
            Ok(ToolOutput::untrusted(r#""QUIET""#))
        );
    }

    #[test]
    fn a_call_the_pack_cannot_serve_names_what_is_wrong() {
        let pack = pack();

        assert_eq!(
            call(&*pack, "demo/scale", json!({ "number": 14 })),
            Err(make_tool_error(
                ErrorCode::ToolInvalidArguments,
                "invalid arguments: missing field `by`"
            ))
        );
        assert_eq!(
            call(&*pack, "demo/scale", json!([14, 3])),
            Err(make_tool_error(
                ErrorCode::ToolInvalidArguments,
                "invalid arguments: expected an object"
            ))
        );
        assert_eq!(
            call(&*pack, "demo/whisper", json!({ "text": "quiet" })),
            Err(make_tool_error(
                ErrorCode::ToolOther,
                "demo has no tool named whisper"
            ))
        );
    }

    #[test]
    fn an_optional_parameter_is_one_the_caller_omits() {
        #[derive(Deserialize, JsonSchema)]
        #[expect(dead_code, reason = "only the schema is read")]
        struct Probe {
            plain: Option<String>,
            unit: Option<Unit>,
            nested: Option<Nested>,
            tagged: Option<Tagged>,
            slots: Vec<Option<bool>>,
        }

        #[derive(Deserialize, JsonSchema)]
        enum Unit {
            On,
            Off,
        }

        #[derive(Deserialize, JsonSchema)]
        #[expect(dead_code, reason = "only the schema is read")]
        struct Nested {
            inner: Option<u8>,
        }

        #[derive(Deserialize, JsonSchema)]
        #[expect(dead_code, reason = "only the schema is read")]
        enum Tagged {
            Count(f64),
            Label(String),
        }

        assert_eq!(
            parameters::<Probe>(),
            json!({
                "type": "object",
                "properties": {
                    "plain": { "type": "string" },
                    "unit": { "type": "string", "enum": ["On", "Off"] },
                    "nested": {
                        "type": "object",
                        "properties": {
                            "inner": { "type": "integer", "minimum": 0, "maximum": 255 }
                        }
                    },
                    "tagged": {
                        "anyOf": [{
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": { "Count": { "type": "number" } },
                                    "required": ["Count"],
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "properties": { "Label": { "type": "string" } },
                                    "required": ["Label"],
                                    "additionalProperties": false
                                }
                            ]
                        }]
                    },
                    "slots": { "type": "array", "items": { "type": ["boolean", "null"] } }
                },
                "required": ["slots"]
            })
        );
    }

    struct Scale;

    #[derive(Deserialize, JsonSchema)]
    struct Factors {
        number: i64,
        #[schemars(description = "The factor.")]
        by: i64,
    }

    #[derive(Serialize)]
    struct Scaled {
        scaled: i64,
        product: i64,
    }

    impl Tool for Scale {
        const NAME: &'static str = "scale";
        const DESCRIPTION: &'static str = "Multiply a number.";
        const TRUST: OutputTrust = OutputTrust::Trusted;
        type Args = Factors;
        type Output = Scaled;

        fn call(
            &self,
            _cx: ToolContext<'_>,
            factors: Factors,
        ) -> impl Future<Output = Result<Scaled, Error>> + Send {
            future::ready(Ok(Scaled {
                scaled: factors.number,
                product: factors.number * factors.by,
            }))
        }
    }

    struct Shout;

    #[derive(Deserialize, JsonSchema)]
    struct Words {
        text: String,
    }

    impl Tool for Shout {
        const NAME: &'static str = "shout";
        const DESCRIPTION: &'static str = "Uppercase a text.";
        const TRUST: OutputTrust = OutputTrust::Untrusted;
        type Args = Words;
        type Output = String;

        fn call(
            &self,
            _cx: ToolContext<'_>,
            words: Words,
        ) -> impl Future<Output = Result<String, Error>> + Send {
            future::ready(Ok(words.text.to_uppercase()))
        }
    }

    fn pack() -> Arc<dyn Pack> {
        Tools::default()
            .with(Scale)
            .with(Shout)
            .new_pack(&PackId::parse("demo").unwrap())
            .unwrap()
    }

    fn call(pack: &dyn Pack, tool: &str, args: Value) -> Result<ToolOutput, Error> {
        let tool = ToolId::parse(tool).unwrap();
        let access = VfsRef::default()
            .acquire(Origin::new("typed tool test"))
            .unwrap();
        let origin = ToolCallOrigin::new("Demo", ToolCaller::Script);
        let mut answer = pin!(pack.call(ToolContext::new(&tool, &access, &origin), args));
        loop {
            if let Poll::Ready(output) = answer
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            {
                return output;
            }
        }
    }
}
