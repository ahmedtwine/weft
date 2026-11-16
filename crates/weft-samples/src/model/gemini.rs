use rig::AgentBuilder;
use rig::catalog::Catalog;
use rig::providers::registry::ConnectOptions;
use rig::schemars::Schema;
use serde_json::{Value, json};

use super::{Store, secret};
use crate::{Candidate, Result};

pub const MODEL: &str = "gcp.gemini/gemini-flash-latest";
const ALTERNATES: &str = "You get an artifact, a part that a person rejects, the marked text in it, the note of the person, and a size. Write that many invariants that the part violates and that can explain the note, each in one general sentence of ten words or fewer, with a one-word name. For each invariant, write one alternate of the part that keeps each fact, satisfies that invariant, and violates each other invariant.";
const CANDIDATES: &str = "You get examples of one system, each with a note, and a context. The context has the grammar of a measure and each thing that a measure can name. Write the invariants that the examples show. Write general invariants, not one invariant for each example. Each invariant has a name of ten words or fewer and one measure in the grammar.";

fn schema(field: &str, more: &Value) -> Value {
    let mut properties = json!({
        "invariants": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    field: {"type": "string"}
                },
                "required": ["name", field],
                "additionalProperties": false
            }
        }
    });
    let mut required = vec!["invariants".to_string()];
    if let (Some(all), Some(more)) = (properties.as_object_mut(), more.as_object()) {
        required.extend(more.keys().cloned());
        all.extend(more.clone());
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

async fn ask(model: &str, instructions: &str, schema: Value, request: &Value) -> Result<Value> {
    let failed = |e: &dyn std::fmt::Display| format!("{model}: {e}");
    let connected = Catalog::builtin()
        .connect_with(
            model,
            ConnectOptions::new().api_key(secret("GEMINI_API_KEY")?),
        )
        .map_err(|e| failed(&e))?;
    let output = AgentBuilder::new(connected)
        .preamble(instructions)
        .output_schema_raw(Schema::try_from(schema).map_err(|e| failed(&e))?)
        .build()
        .prompt(request.to_string())
        .await
        .map_err(|e| failed(&e))?
        .output();
    serde_json::from_str(&output).map_err(|e| failed(&format!("{e}: {output}")))
}

async fn drawn(
    store: &Store,
    model: &str,
    instructions: &str,
    schema: Value,
    request: &Value,
) -> Result<Value> {
    store
        .cached(
            (model, instructions, request.to_string()),
            ask(model, instructions, schema, request),
        )
        .await
}

pub(super) async fn alternates(store: &Store, model: &str, request: &Value) -> Result<Vec<Value>> {
    let more = json!({"alternates": {"type": "array", "items": {"type": "string"}}});
    let answer = drawn(store, model, ALTERNATES, schema("measure", &more), request).await?;
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

pub(super) async fn candidates(
    store: &Store,
    model: &str,
    request: &Value,
) -> Result<Vec<Candidate>> {
    let answer = drawn(
        store,
        model,
        CANDIDATES,
        schema("measure", &json!({})),
        request,
    )
    .await?;
    Ok(answer["invariants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|invariant| {
            Some(Candidate {
                name: invariant["name"].as_str()?.to_string(),
                measure: invariant["measure"].as_str()?.to_string(),
            })
        })
        .collect())
}
