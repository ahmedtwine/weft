use serde_json::{Value as Json, json};
use weft_parse::State;

pub(crate) const START: &str = "start";

pub(crate) struct Call<'a> {
    pub function: &'a str,
    pub input: &'a Json,
}

pub(crate) enum Turn<'a> {
    Start(&'a [Call<'a>]),
    Run(&'a str, Call<'a>),
    Read,
    Done,
}

pub(crate) enum Report {
    State(State, Json),
    Thrown(String),
    Refused(String),
}

impl Call<'_> {
    fn json(&self) -> Json {
        json!({ "function": self.function, "input": self.input })
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
