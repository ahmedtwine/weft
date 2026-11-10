use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use serde_json::Value as Json;
use weft_parse::{Check, Ran, State};

use crate::{Answer, Choice, Fault, Runner, Source, Tags};

const TRIES: usize = 3;

#[derive(Clone, PartialEq)]
pub struct Next {
    pub state: State,
    pub refused: Option<String>,
}

type Done = Result<Next, String>;
type Draws = Vec<(Choice, Json)>;

#[derive(Clone)]
enum Reply {
    Draw(Choice),
    Done(Done, Option<f64>),
    Skip,
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

    fn run(
        &self,
        runner: &mut dyn Runner,
        tags: &Tags,
        source: &dyn Source,
    ) -> Result<Done, Fault> {
        let raw = match self {
            Call::Reset => runner.reset(source),
            Call::Apply(object, verb, input) => runner.apply(object, verb, input, source),
        };
        match raw {
            Ok(Answer { state, refused }) => tags
                .read(&state)
                .map(|state| Ok(Next { state, refused }))
                .map_err(Fault::Broken),
            Err(Fault::Thrown(e)) => Ok(Err(e)),
            Err(fault) => Err(fault),
        }
    }
}

struct Made {
    call: Call,
    drawn: Draws,
    done: Done,
}

fn same(a: &Done, b: &Done) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => a == b,
        (a, b) => a.is_err() && b.is_err(),
    }
}

fn again(runner: &mut dyn Runner, tags: &Tags, made: &[Made]) -> Result<bool, Fault> {
    for made in made {
        let told = Told::new(None, made.drawn.iter().cloned().collect());
        let done = made.call.run(runner, tags, &told)?;
        if !told.kept() || !same(&done, &made.done) {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Told<'s> {
    source: Option<&'s dyn Source>,
    ahead: RefCell<VecDeque<(Choice, Json)>>,
    drawn: RefCell<Draws>,
    score: Cell<Option<f64>>,
    strayed: Cell<bool>,
}

impl<'s> Told<'s> {
    fn new(source: Option<&'s dyn Source>, ahead: VecDeque<(Choice, Json)>) -> Told<'s> {
        Told {
            source,
            ahead: RefCell::new(ahead),
            drawn: RefCell::new(Vec::new()),
            score: Cell::new(None),
            strayed: Cell::new(false),
        }
    }

    fn fresh(&self, choice: &Choice) -> Json {
        match self.source {
            Some(source) => source.draw(choice),
            None => {
                self.strayed.set(true);
                Json::Null
            }
        }
    }

    fn kept(&self) -> bool {
        !self.strayed.get() && self.ahead.borrow().is_empty()
    }
}

impl Source for Told<'_> {
    fn draw(&self, choice: &Choice) -> Json {
        let ahead = self.ahead.borrow_mut().pop_front();
        let value = match ahead {
            Some((known, value)) => {
                self.strayed.set(self.strayed.get() || known != *choice);
                value
            }
            None => self.fresh(choice),
        };
        self.drawn
            .borrow_mut()
            .push((choice.clone(), value.clone()));
        value
    }

    fn index(&self, count: usize) -> usize {
        self.draw(&Choice::Int(0, count as i64 - 1))
            .as_u64()
            .map_or(0, |n| n as usize)
    }

    fn target(&self, score: f64) {
        self.score
            .set(Some(self.score.get().map_or(score, |s| s.max(score))));
    }
}

pub struct Memo<'a> {
    pub tags: Tags,
    runner: &'a mut dyn Runner,
    known: HashMap<(usize, String), (Reply, usize)>,
    made: Vec<Made>,
    kept: Vec<Made>,
    at: usize,
    live: bool,
}

impl<'a> Memo<'a> {
    pub fn new(runner: &'a mut dyn Runner, tags: Tags) -> Memo<'a> {
        Memo {
            tags,
            runner,
            known: HashMap::new(),
            made: Vec::new(),
            kept: Vec::new(),
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

    pub fn ran(&mut self) -> Result<Ran, String> {
        self.runner.ran()
    }

    pub fn reset(&mut self, source: &dyn Source) -> Result<Next, Fault> {
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
    ) -> Result<Next, Fault> {
        self.call(
            Call::Apply(object.to_string(), verb.to_string(), input.clone()),
            source,
        )
    }

    pub fn keep(&mut self) {
        self.kept = std::mem::take(&mut self.made);
    }

    pub fn repeats(&mut self, times: usize) -> Result<usize, Fault> {
        (0..times).try_fold(0, |seen, _| {
            again(self.runner, &self.tags, &self.kept).map(|met| seen + usize::from(met))
        })
    }

    fn live(&mut self) -> Result<(), Fault> {
        for _ in 0..TRIES {
            if self.live || again(self.runner, &self.tags, &self.made)? {
                self.live = true;
                return Ok(());
            }
        }
        Err(Fault::Changed)
    }

    fn learn(&mut self, at: usize, said: String, reply: Reply) -> usize {
        let fresh = self.known.len() + 1;
        self.known.entry((at, said)).or_insert((reply, fresh)).1
    }

    fn end(&mut self, call: Call, drawn: Draws, done: Done, at: usize) -> Result<Next, Fault> {
        self.at = at;
        self.made.push(Made {
            call,
            drawn,
            done: done.clone(),
        });
        done.map_err(Fault::Thrown)
    }

    fn heard(
        &mut self,
        call: &Call,
        source: &dyn Source,
        ahead: VecDeque<(Choice, Json)>,
    ) -> Result<(Draws, Done, usize), Fault> {
        let told = Told::new(Some(source), ahead);
        let done = call.run(self.runner, &self.tags, &told)?;
        if !told.kept() {
            return Err(Fault::Changed);
        }
        let score = told.score.get();
        score.into_iter().for_each(|s| source.target(s));
        let drawn = told.drawn.into_inner();
        let mut at = self.at;
        let mut said = call.said();
        for (choice, value) in &drawn {
            at = self.learn(at, said, Reply::Draw(choice.clone()));
            said = value.to_string();
        }
        let at = self.learn(at, said, Reply::Done(done.clone(), score));
        Ok((drawn, done, at))
    }

    fn call(&mut self, call: Call, source: &dyn Source) -> Result<Next, Fault> {
        let mut at = self.at;
        let mut said = call.said();
        let mut ahead = VecDeque::new();
        while !self.live {
            match self.known.get(&(at, said.clone())).cloned() {
                Some((Reply::Draw(choice), next)) => {
                    let value = source.draw(&choice);
                    said = value.to_string();
                    ahead.push_back((choice, value));
                    at = next;
                }
                Some((Reply::Done(done, score), next)) => {
                    score.into_iter().for_each(|s| source.target(s));
                    return self.end(call, ahead.into(), done, next);
                }
                Some((Reply::Skip, _)) => return Err(Fault::Changed),
                None => break,
            }
        }
        let (drawn, done, at) = self
            .live()
            .and_then(|()| self.heard(&call, source, ahead))
            .inspect_err(|_| {
                self.known.insert((at, said), (Reply::Skip, 0));
            })?;
        self.end(call, drawn, done, at)
    }
}
