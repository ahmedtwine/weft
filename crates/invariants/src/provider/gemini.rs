use rig::AgentBuilder;
use rig::catalog::Catalog;
use rig::providers::registry::ConnectOptions;
use rig::schemars::Schema;
use serde_json::{Value, json};

use crate::Result;
use crate::provider::{cached, secret};

pub const MODEL: &str = "gcp.gemini/gemini-flash-latest";
const INSTRUCTIONS: &str = "You get an artifact, a part that a person rejects, the marked text in it, the note of the person, and a size. Write that many invariants that the part violates and that can explain the note, each in one general sentence of ten words or fewer, with a one-word name. For each invariant, write one alternate of the part that keeps each fact, satisfies that invariant, and violates each other invariant.";

pub struct Gemini {
    runtime: tokio::runtime::Runtime,
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "invariants": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "measure": {"type": "string"}
                    },
                    "required": ["name", "measure"],
                    "additionalProperties": false
                }
            },
            "alternates": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["invariants", "alternates"],
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
        let invariants = answer["invariants"].as_array().cloned().unwrap_or_default();
        Ok(answer["alternates"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .zip(invariants)
            .map(|(text, invariant)| {
                json!({"text": text, "name": invariant["name"], "measure": invariant["measure"]})
            })
            .collect())
    }
}
