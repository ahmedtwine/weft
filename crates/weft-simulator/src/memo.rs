use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use serde_json::Value as Json;
use weft_parse::{Check, State};

use crate::{Choice, Fault, Runner, Source};

type Done = Result<Json, String>;

#[derive(Clone, PartialEq)]
enum Reply {
    Draw(Choice),
    Done(Done, Option<f64>),
}

enum Call {
    Reset,
    Apply(String, String, Json),
}

impl Call {
    fn said(&self) -> String {
        match self {
            Call::Reset => "reset".to_string(),
            Call::Apply(object, verb, input) => format!("{verb} {object} {input}"),
        }
    }

    fn run(&self, runner: &mut dyn Runner, source: &dyn Source) -> Result<Done, Fault> {
        let done = match self {
            Call::Reset => runner.reset(source),
            Call::Apply(object, verb, input) => runner.apply(object, verb, input, source),
        };
        match done {
            Ok(state) => Ok(Ok(state)),
            Err(Fault::Thrown(e)) => Ok(Err(e)),
            Err(fault) => Err(fault),
        }
    }
}

struct Made {
    call: Call,
    drawn: Vec<Json>,
    done: Done,
}

struct Told<'s> {
    source: Option<&'s dyn Source>,
    ahead: RefCell<VecDeque<Json>>,
    drawn: RefCell<Vec<(Choice, Json)>>,
    score: Cell<Option<f64>>,
    over: Cell<bool>,
}

impl<'s> Told<'s> {
    fn new(source: Option<&'s dyn Source>, ahead: VecDeque<Json>) -> Told<'s> {
        Told {
            source,
            ahead: RefCell::new(ahead),
            drawn: RefCell::new(Vec::new()),
            score: Cell::new(None),
            over: Cell::new(false),
        }
    }

    fn fresh(&self, choice: &Choice) -> Json {
        match self.source {
            Some(source) => source.draw(choice),
            None => {
                self.over.set(true);
                Json::Null
            }
        }
    }
}

impl Source for Told<'_> {
    fn draw(&self, choice: &Choice) -> Json {
        let ahead = self.ahead.borrow_mut().pop_front();
        let value = ahead.unwrap_or_else(|| self.fresh(choice));
        self.drawn
            .borrow_mut()
            .push((choice.clone(), value.clone()));
        value
    }

    fn bool(&self) -> bool {
        self.draw(&Choice::Bool).as_bool().unwrap_or_default()
    }

    fn int(&self, min: i64, max: i64) -> i64 {
        self.draw(&Choice::Int(min, max)).as_i64().unwrap_or(min)
    }

    fn text(&self, min: usize, max: Option<usize>) -> String {
        match self.draw(&Choice::Text(min, max)) {
            Json::String(text) => text,
            _ => String::new(),
        }
    }

    fn index(&self, count: usize) -> usize {
        self.int(0, count as i64 - 1) as usize
    }

    fn target(&self, score: f64) {
        self.score
            .set(Some(self.score.get().map_or(score, |s| s.max(score))));
    }
}

pub struct Memo<'a> {
    runner: &'a mut dyn Runner,
    known: Option<HashMap<(usize, String), (Reply, usize)>>,
    made: Vec<Made>,
    at: usize,
    live: bool,
}

impl<'a> Memo<'a> {
    pub fn new(runner: &'a mut dyn Runner) -> Memo<'a> {
        Memo {
            runner,
            known: Some(HashMap::new()),
            made: Vec::new(),
            at: 0,
            live: false,
        }
    }

    pub fn check(
        &mut self,
        check: &Check,
        cur: &State,
        nxt: &State,
        verb: &str,
    ) -> Result<Option<Vec<String>>, Fault> {
        self.runner.check(check, cur, nxt, verb)
    }

    pub fn reset(&mut self, source: &dyn Source) -> Result<Json, Fault> {
        self.made.clear();
        self.at = 0;
        self.live = false;
        self.call(Call::Reset, source)
    }

    pub fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Json, Fault> {
        self.call(
            Call::Apply(object.to_string(), verb.to_string(), input.clone()),
            source,
        )
    }

    pub fn live(&mut self) -> Result<(), Fault> {
        if self.live {
            return Ok(());
        }
        for made in &self.made {
            let told = Told::new(None, made.drawn.iter().cloned().collect());
            let done = made.call.run(self.runner, &told)?;
            if told.over.get() || !told.ahead.borrow().is_empty() || done != made.done {
                self.known = None;
                return Err(Fault::Changed);
            }
        }
        self.live = true;
        Ok(())
    }

    fn learn(&mut self, at: usize, said: String, reply: Reply) -> usize {
        let Some(known) = &mut self.known else {
            return 0;
        };
        let fresh = known.len() + 1;
        let (old, next) = known.entry((at, said)).or_insert((reply.clone(), fresh));
        match *old == reply {
            true => *next,
            false => {
                self.known = None;
                0
            }
        }
    }

    fn end(&mut self, call: Call, drawn: Vec<Json>, done: Done, at: usize) -> Result<Json, Fault> {
        self.at = at;
        self.made.push(Made {
            call,
            drawn,
            done: done.clone(),
        });
        done.map_err(Fault::Thrown)
    }

    fn call(&mut self, call: Call, source: &dyn Source) -> Result<Json, Fault> {
        let mut at = self.at;
        let mut said = call.said();
        let mut ahead = VecDeque::new();
        while !self.live {
            let known = self.known.as_ref().and_then(|k| k.get(&(at, said.clone())));
            match known.cloned() {
                Some((Reply::Draw(choice), next)) => {
                    let value = source.draw(&choice);
                    said = value.to_string();
                    ahead.push_back(value);
                    at = next;
                }
                Some((Reply::Done(done, score), next)) => {
                    score.into_iter().for_each(|s| source.target(s));
                    return self.end(call, ahead.into(), done, next);
                }
                None => self.live()?,
            }
        }
        let told = Told::new(Some(source), ahead);
        let done = call.run(self.runner, &told)?;
        let score = told.score.get();
        score.into_iter().for_each(|s| source.target(s));
        let mut at = self.at;
        let mut said = call.said();
        let mut drawn = Vec::new();
        for (choice, value) in told.drawn.into_inner() {
            at = self.learn(at, said, Reply::Draw(choice));
            said = value.to_string();
            drawn.push(value);
        }
        let at = self.learn(at, said, Reply::Done(done.clone(), score));
        self.end(call, drawn, done, at)
    }
}
