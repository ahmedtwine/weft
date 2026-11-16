use std::sync::Arc;

use serde_json::{Value as Json, json};
use weft_hegel::{Generator, Halt};
use weft_parse::State;

pub(crate) const START: &str = "start";

pub(crate) struct Start {
    pub line: usize,
    pub input: Option<Generator>,
}

pub(crate) struct Action {
    pub object: String,
    pub verb: String,
    pub line: usize,
    pub drawn: Generator,
}

#[derive(Default)]
pub(crate) struct Hello {
    pub schemas: Vec<(String, Json)>,
    pub start: Vec<Start>,
    pub actions: Vec<Action>,
}

pub(crate) struct Call<'a> {
    pub line: usize,
    pub input: &'a Json,
}

pub(crate) enum Turn<'a> {
    Start(&'a [Call<'a>]),
    Run(&'a str, Call<'a>),
    Read,
    Done,
}

pub(crate) enum Report {
    State(State),
    Thrown(String),
    Refused(String),
}

pub(crate) enum Ask {
    Generate(Arc<Generator>),
    PoolAdd(String, Json),
    PoolGenerate(String, bool),
    Target(f64, String),
}

pub(crate) type Answer<'a> = dyn FnMut(Ask) -> Result<Option<Json>, Halt> + 'a;

impl Call<'_> {
    fn json(&self) -> Json {
        json!({ "use": self.line, "input": self.input })
    }
}

impl Turn<'_> {
    pub(crate) fn json(&self) -> String {
        match self {
            Turn::Start(calls) => {
                json!({ "step": START, "calls": calls.iter().map(Call::json).collect::<Vec<_>>() })
            }
            Turn::Run(verb, call) => json!({ "step": verb, "calls": [call.json()] }),
            Turn::Read => json!({ "calls": [] }),
            Turn::Done => json!({ "done": true }),
        }
        .to_string()
    }
}
