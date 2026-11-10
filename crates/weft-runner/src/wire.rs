use std::path::Path;

use serde_json::{Value as Json, json};
use weft_parse::{Ran, Span};
use weft_simulator::{Answer, Fault, Runner, Source, choice, tags};

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

fn outside(file: &str, said: &str, dir: &Path) -> Result<(), String> {
    match Path::new(file).starts_with(dir) {
        true => Err(format!(
            "{said} is from `{file}`, a file of the driver: only the code outside `{}` gives it",
            dir.display()
        )),
        false => Ok(()),
    }
}

fn tagged(node: &Json, dir: &Path) -> Result<(), String> {
    for tag in tags(node) {
        let said = format!("the tag `{tag}`");
        let from: Vec<&str> = node
            .get("from")
            .and_then(Json::as_array)
            .map_or(Vec::new(), |all| {
                all.iter().filter_map(Json::as_str).collect()
            });
        if from.is_empty() {
            return Err(format!(
                "{said} has no `from`: the runner names the file that registers each tag"
            ));
        }
        from.into_iter()
            .try_for_each(|file| outside(file, &said, dir))?;
    }
    let named = node
        .get("properties")
        .and_then(Json::as_object)
        .into_iter()
        .flat_map(|all| all.values());
    let listed = ["anyOf", "oneOf"]
        .into_iter()
        .filter_map(|k| node.get(k)?.as_array())
        .flatten();
    let single = ["items", "additionalProperties"]
        .into_iter()
        .filter_map(|k| node.get(k));
    named
        .chain(listed)
        .chain(single)
        .try_for_each(|n| tagged(n, dir))
}

fn run(node: &Json, kind: &str, dir: &Path) -> Result<(), String> {
    let said = format!("the function of the {kind} `{}`", tags(node).join(", "));
    let file = node.get("run").and_then(Json::as_str).ok_or_else(|| {
        format!("{said} has no `run`: the runner names the file that exports the function")
    })?;
    outside(file, &said, dir)
}

fn real(schema: &Json, dir: &Path) -> Result<(), String> {
    schema
        .get("state")
        .into_iter()
        .try_for_each(|state| tagged(state, dir))?;
    for (kind, key) in [("action", "actions"), ("mock", "mocks")] {
        for node in schema
            .get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            tagged(node, dir)?;
            run(node, kind, dir)?;
        }
    }
    Ok(())
}

impl Wire {
    pub fn new(transport: Box<dyn Transport>, dir: &Path) -> Result<Wire, String> {
        let mut wire = Wire {
            transport,
            schema: Json::Null,
            cut: false,
        };
        wire.schema = wire.transport.schema()?;
        real(&wire.schema, dir)?;
        Ok(wire)
    }

    fn uncut(&mut self) -> Result<(), String> {
        if self.cut {
            self.transport.ask(&json!({ "stop": true }))?;
            self.cut = false;
        }
        Ok(())
    }

    fn ask(&mut self, message: Json, source: &dyn Source) -> Result<Answer, Fault> {
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
                (Some(state), None, None) => {
                    return Ok(Answer {
                        state: state.clone(),
                        refused: answer
                            .get("refused")
                            .and_then(Json::as_str)
                            .map(String::from),
                    });
                }
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

    fn reset(&mut self, source: &dyn Source) -> Result<Answer, Fault> {
        self.ask(json!({ "reset": true }), source)
    }

    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Answer, Fault> {
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
