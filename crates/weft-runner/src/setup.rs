use std::path::Path;

use serde_json::{Map, Value as Json, json};
use weft_parse::{Diagram, Ran, Span, State, Value};

use weft_simulator::{Answer, Fault, Runner, Source};

pub struct Setup<'a> {
    d: &'a Diagram,
    file: String,
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
    pub fn new(d: &'a Diagram, file: &Path) -> Setup<'a> {
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
            file: file.strip_prefix(".").unwrap_or(file).display().to_string(),
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

    fn reset(&mut self, _: &dyn Source) -> Result<Answer, Fault> {
        self.now = self.d.init.clone();
        self.read().map(Answer::from)
    }

    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        _: &Json,
        _: &dyn Source,
    ) -> Result<Answer, Fault> {
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
        self.read().map(Answer::from)
    }

    fn ran(&mut self) -> Result<Ran, String> {
        let span = |line: usize| Span {
            file: self.file.clone(),
            from: line,
            to: line,
        };
        let mut actions: Vec<(String, Vec<Span>)> = Vec::new();
        for m in self.d.objects.iter().flat_map(|o| &o.moves) {
            match actions.iter_mut().find(|(verb, _)| *verb == m.verb) {
                Some((_, spans)) => spans.push(span(m.line)),
                None => actions.push((m.verb.clone(), vec![span(m.line)])),
            }
        }
        Ok(Ran {
            reset: self
                .d
                .objects
                .iter()
                .filter(|o| !o.initial.is_empty())
                .map(|o| span(o.line))
                .collect(),
            actions,
        })
    }
}
