use serde_json::{Map, Value as Json, json};
use weft_parse::{Diagram, State, Value};

use weft_simulator::{Fault, Runner, Source};

pub struct Setup<'a> {
    d: &'a Diagram,
    now: State,
    schema: Json,
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Text(_) => "string",
        Value::Int(_) => "integer",
    }
}

impl<'a> Setup<'a> {
    pub fn new(d: &'a Diagram) -> Setup<'a> {
        let state: Map<String, Json> = d
            .objects
            .iter()
            .map(|o| {
                let keys: Map<String, Json> = o
                    .initial
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            json!({ "type": kind(v), "weft": format!("{}.{k}", o.name) }),
                        )
                    })
                    .collect();
                (
                    o.name.clone(),
                    json!({ "type": "object", "properties": keys }),
                )
            })
            .collect();
        let mut verbs: Vec<&str> = d
            .objects
            .iter()
            .flat_map(|o| &o.moves)
            .map(|m| m.verb.as_str())
            .collect();
        verbs.sort_unstable();
        verbs.dedup();
        let actions: Vec<Json> = verbs
            .into_iter()
            .map(|verb| json!({ "type": "object", "weft": verb, "properties": {} }))
            .collect();
        Setup {
            d,
            now: d.init.clone(),
            schema: json!({
                "state": { "type": "object", "properties": state },
                "actions": actions
            }),
        }
    }

    fn read(&self) -> Result<Json, Fault> {
        self.now
            .iter()
            .map(|(o, pairs)| {
                pairs
                    .iter()
                    .map(|(k, v)| {
                        match v {
                            Value::Text(s) => Ok(Json::from(s.as_str())),
                            Value::Int(n) => i64::try_from(*n).map(Json::from).map_err(|_| {
                                Fault::Broken(format!("`{o}.{k}` is `{n}`, above an i64"))
                            }),
                        }
                        .map(|j| (k.clone(), j))
                    })
                    .collect::<Result<Map<_, _>, _>>()
                    .map(|keys| (o.clone(), Json::Object(keys)))
            })
            .collect::<Result<Map<_, _>, _>>()
            .map(Json::Object)
    }
}

impl Runner for Setup<'_> {
    fn schema(&self) -> &Json {
        &self.schema
    }

    fn reset(&mut self, _: &dyn Source) -> Result<Json, Fault> {
        self.now = self.d.init.clone();
        self.read()
    }

    fn apply(&mut self, object: &str, verb: &str, _: &Json, _: &dyn Source) -> Result<Json, Fault> {
        let to = self
            .d
            .enabled(&self.now)
            .into_iter()
            .find(|(o, v, _)| o.name == object && *v == verb)
            .map(|(_, _, to)| to)
            .ok_or_else(|| {
                Fault::Broken(format!("`{object}` has no `on {verb}` line from its state"))
            })?;
        self.now.insert(object.to_string(), to);
        self.read()
    }
}
