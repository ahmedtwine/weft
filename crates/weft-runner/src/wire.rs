use serde_json::{Value as Json, json};
use weft_parse::{Ran, Span};
use weft_simulator::{Fault, Runner, Source, choice};

use crate::transport::Transport;

pub struct Wire {
    transport: Box<dyn Transport>,
    schema: Json,
    cut: bool,
}

fn spans(node: &Json) -> Option<Vec<Span>> {
    let line = |s: &Json, k: &str| usize::try_from(s.get(k)?.as_u64()?).ok();
    node.as_array()?
        .iter()
        .map(|s| {
            Some(Span {
                file: s.get("file")?.as_str()?.to_string(),
                from: line(s, "from")?,
                to: line(s, "to")?,
            })
        })
        .collect()
}

fn code(answer: &Json) -> Option<Ran> {
    let ran = answer.get("ran")?;
    Some(Ran {
        reset: spans(ran.get("reset")?)?,
        actions: ran
            .get("actions")?
            .as_object()?
            .iter()
            .map(|(verb, all)| Some((verb.clone(), spans(all)?)))
            .collect::<Option<_>>()?,
    })
}

impl Wire {
    pub fn new(transport: Box<dyn Transport>) -> Result<Wire, String> {
        let mut wire = Wire {
            transport,
            schema: Json::Null,
            cut: false,
        };
        wire.schema = wire.transport.schema()?;
        Ok(wire)
    }

    fn uncut(&mut self) -> Result<(), String> {
        if self.cut {
            self.transport.ask(&json!({ "stop": true }))?;
            self.cut = false;
        }
        Ok(())
    }

    fn ask(&mut self, message: Json, source: &dyn Source) -> Result<Json, Fault> {
        self.uncut().map_err(Fault::Broken)?;
        let mut message = message;
        loop {
            let answer = self.transport.ask(&message).map_err(Fault::Broken)?;
            if let Some(score) = answer.get("score") {
                source.target(score.as_f64().ok_or_else(|| {
                    Fault::Broken(format!("the score `{score}` is not a number"))
                })?);
            }
            match (
                answer.get("state"),
                answer.get("error").and_then(Json::as_str),
                answer.get("draw"),
            ) {
                (Some(state), None, None) => return Ok(state.clone()),
                (None, Some(error), None) => return Err(Fault::Thrown(error.to_string())),
                (None, None, Some(draw)) => {
                    let c = choice("draw", draw).map_err(Fault::Broken)?;
                    self.cut = true;
                    let drawn = source.draw(&c);
                    self.cut = false;
                    message = json!({ "drawn": drawn });
                }
                _ => return Err(Fault::Broken(format!("the runner answers `{answer}`"))),
            }
        }
    }
}

impl Runner for Wire {
    fn schema(&self) -> &Json {
        &self.schema
    }

    fn reset(&mut self, source: &dyn Source) -> Result<Json, Fault> {
        self.ask(json!({ "reset": true }), source)
    }

    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Json, Fault> {
        self.ask(
            json!({ "apply": verb, "object": object, "input": input }),
            source,
        )
    }

    fn ran(&mut self) -> Result<Ran, String> {
        self.uncut()?;
        let answer = self.transport.ask(&json!({ "ran": true }))?;
        code(&answer).ok_or_else(|| format!("the runner answers `{answer}` to `ran`"))
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.uncut();
        self.transport.done();
    }
}
