use rig::AgentBuilder;
use rig::catalog::Catalog;
use rig::providers::registry::ConnectOptions;
use rig::schemars::Schema;
use serde_json::{Value, json};

use crate::Result;
use crate::provider::{cached, secret};

pub const MODEL: &str = "gcp.gemini/gemini-flash-latest";
const INSTRUCTIONS: &str = "You get an artifact, the part of it that a person rejects, the marked text in that part, the note of that person, and a size. First find that many possible reasons for the note. Each reason is one property of the marked text, stated in general words that apply to any artifact of the same medium. The reasons compete: each one is a different explanation of the note, from a different kind of property, such as the form, the words, the actor, the claim, or what the reader can do with the part. Two reasons that differ only in wording are one reason. List the reasons in `reasons` before you write any alternate. Then write one alternate for each reason, in the same order. The alternate keeps the topic, each fact and each other property of the part, and removes only the property of its reason. When the person accepts the alternate, the reason is part of the note. When the person rejects it, the reason is not. In `change`, state the property that the alternate removes, in ten words or fewer.";

pub struct Gemini {
    runtime: tokio::runtime::Runtime,
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "reasons": {"type": "array", "items": {"type": "string"}},
            "alternates": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "change": {"type": "string"}
                    },
                    "required": ["text", "change"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["reasons", "alternates"],
        "additionalProperties": false
    })
}

async fn ask(model: &str, context: &Value) -> Result<Value> {
    let failed = |e: &dyn std::fmt::Display| format!("{model}: {e}");
    let connected = Catalog::builtin()
        .connect_with(
            model,
            ConnectOptions::new().api_key(secret("GEMINI_API_KEY")?),
        )
        .map_err(|e| failed(&e))?;
    let output = AgentBuilder::new(connected)
        .preamble(INSTRUCTIONS)
        .output_schema_raw(Schema::try_from(schema()).map_err(|e| failed(&e))?)
        .build()
        .prompt(context.to_string())
        .await
        .map_err(|e| failed(&e))?
        .output();
    serde_json::from_str(&output).map_err(|e| failed(&format!("{e}: {output}")))
}

impl Gemini {
    pub fn open() -> Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|e| e.to_string())?,
        })
    }

    pub fn draw(&self, model: &str, context: &Value) -> Result<Vec<Value>> {
        let answer: Value = self.runtime.block_on(cached(
            (model, INSTRUCTIONS, context.to_string()),
            ask(model, context),
        ))?;
        Ok(answer["alternates"].as_array().cloned().unwrap_or_default())
    }
}
