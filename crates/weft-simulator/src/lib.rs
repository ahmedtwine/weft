mod hub;
mod schema;
mod shape;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as Json, json};
use weft_hegel::{Case, Engine, Halt, INTERESTING, INVALID, OVERRUN, VALID};
use weft_parse::{Diagram, Ran, Span, State, shown};

use crate::shape::{Field, field, item, kind};

pub use hub::{Hub, Peer, Remote, Session};

#[derive(Clone, Debug)]
pub struct Hop {
    pub from: String,
    pub to: String,
    pub method: String,
    pub path: String,
    pub body: String,
    pub status: u16,
    pub reply: String,
    pub sent: usize,
    pub answered: usize,
}
pub use weft_hegel::Generator;

const STEPS: i64 = 20;
const TRIES: usize = 4;
const ORIGIN: &str = "weft";
const START: &str = "start";

pub struct Start {
    pub line: usize,
    pub input: Option<Generator>,
    pub pool: Option<String>,
}

pub struct Action {
    pub verb: String,
    pub line: usize,
    pub drawn: Generator,
    pub pool: Option<String>,
}

#[derive(Default)]
pub struct Hello {
    pub schemas: Vec<(String, Json)>,
    pub start: Vec<Start>,
    pub actions: Vec<Action>,
    pub sized: bool,
}

pub struct Call<'a> {
    pub line: usize,
    pub input: &'a Json,
    pub from: &'a Json,
    pub add: Option<u64>,
}

pub enum Turn<'a> {
    Start(&'a [Call<'a>]),
    Run(&'a str, &'a str, Call<'a>),
    Read,
    Done,
}

pub enum Report {
    State(State, Vec<u64>),
    Thrown(String),
    Refused,
}

pub trait Member {
    fn hello(&self) -> &Hello;
    fn turn(&mut self, turn: &Turn) -> Result<Report, String>;
    fn finish(&mut self) -> Result<Ran, String>;
    fn hops(&mut self) -> Vec<Hop> {
        Vec::new()
    }
}

fn heard(members: &mut Members) -> Vec<Hop> {
    members.iter_mut().flat_map(|m| m.hops()).collect()
}

#[derive(Clone)]
pub struct Line {
    pub action: Option<(String, String)>,
    pub input: Vec<Json>,
    pub state: State,
    pub evidence: Vec<String>,
    pub hops: Vec<Hop>,
}

impl Line {
    pub fn step(&self) -> String {
        self.action
            .as_ref()
            .map_or(START.into(), |(verb, object)| format!("{verb} to={object}"))
    }

    pub fn input(&self) -> String {
        self.input
            .iter()
            .map(written)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub struct Tally {
    pub step: String,
    pub steps: u64,
    pub checks: u64,
}

pub struct Outcome {
    pub cases: u64,
    pub runs: u64,
    pub tally: Vec<Tally>,
    pub trace: Vec<Line>,
    pub failed: bool,
    pub caveat: Option<String>,
    pub output: Vec<String>,
    pub ran: Ran,
    pub types: BTreeMap<String, String>,
}

struct Step {
    verb: String,
    object: String,
    owner: usize,
    action: usize,
}

struct Run<'a> {
    d: &'a Diagram,
    steps: Vec<Step>,
    sized: bool,
    machine: bool,
    runs: u64,
    added: u64,
    first: State,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    trace: Vec<Line>,
    story: Vec<Line>,
    refused: BTreeSet<String>,
}

enum Stepped {
    Next,
    Refused,
    Failed(Vec<String>),
}

type Members<'m> = [Box<dyn Member + 'm>];

impl Call<'_> {
    fn json(&self) -> Json {
        json!({ "use": self.line, "input": self.input, "from": self.from, "add": self.add })
    }
}

impl Turn<'_> {
    pub fn json(&self) -> String {
        match self {
            Turn::Start(calls) => {
                json!({ "step": START, "calls": calls.iter().map(Call::json).collect::<Vec<_>>() })
            }
            Turn::Run(verb, _, call) => json!({ "step": verb, "calls": [call.json()] }),
            Turn::Read => json!({ "calls": [] }),
            Turn::Done => json!({ "done": true }),
        }
        .to_string()
    }
}

pub fn view(s: &State) -> String {
    s.iter()
        .map(|(o, p)| format!("{o}: {}", shown(p)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn written(input: &Json) -> String {
    match input {
        Json::Null => String::new(),
        Json::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

fn covered(trace: &[Line]) -> usize {
    trace
        .iter()
        .filter_map(|l| l.action.as_ref().map(|(verb, _)| verb))
        .collect::<BTreeSet<_>>()
        .len()
}

fn telling(trace: &[Line]) -> Vec<Line> {
    let all = covered(trace);
    let end = (1..=trace.len())
        .find(|n| covered(&trace[..*n]) == all)
        .unwrap_or(trace.len());
    trace[..end].to_vec()
}

fn merged(mut state: State, part: State) -> State {
    for (object, pairs) in part {
        state.entry(object).or_default().extend(pairs);
    }
    state
}

impl Run<'_> {
    fn thrown(&mut self, mut line: Line, error: &str) -> Vec<String> {
        line.evidence = vec![format!("throws {error}")];
        self.trace.push(line.clone());
        line.evidence
    }

    fn sized(&self, case: &Case, mut state: State) -> State {
        if self.sized {
            for o in &self.d.objects {
                for pool in o.pools.iter().filter(|pool| o.start.contains_key(*pool)) {
                    state
                        .entry(o.name.clone())
                        .or_default()
                        .insert(pool.clone(), json!(case.size(pool)));
                }
            }
        }
        state
    }

    fn started(
        &mut self,
        case: &Case,
        members: &mut Members,
    ) -> Result<Result<State, Vec<String>>, Halt> {
        let mut real = State::new();
        let mut line = Line {
            action: None,
            input: Vec::new(),
            state: State::new(),
            evidence: Vec::new(),
            hops: Vec::new(),
        };
        self.trace.clear();
        self.added = 0;
        for pool in self.d.objects.iter().flat_map(|o| &o.pools) {
            case.pool(pool)?;
        }
        for m in members.iter_mut() {
            let inputs = m
                .hello()
                .start
                .iter()
                .map(|s| s.input.as_ref().map_or(Ok(Json::Null), |g| case.draw(g)))
                .collect::<Result<Vec<Json>, Halt>>();
            let inputs = match inputs {
                Err(Halt::Empty) => {
                    self.refused.insert(START.into());
                    return Err(Halt::Invalid);
                }
                inputs => inputs?,
            };
            let pools: Vec<(String, u64)> = m
                .hello()
                .start
                .iter()
                .filter_map(|s| s.pool.clone())
                .zip(self.added..)
                .collect();
            let report = match inputs.is_empty() {
                true => m.turn(&Turn::Read)?,
                false => {
                    line.input = inputs.clone();
                    let mut adds = pools.iter().map(|(_, add)| *add);
                    let calls: Vec<Call> = m
                        .hello()
                        .start
                        .iter()
                        .zip(&inputs)
                        .map(|(s, input)| Call {
                            line: s.line,
                            input,
                            from: &Json::Null,
                            add: s.pool.as_ref().and_then(|_| adds.next()),
                        })
                        .collect();
                    m.turn(&Turn::Start(&calls))?
                }
            };
            let added = match report {
                Report::State(part, added) => {
                    real = merged(real, part);
                    added
                }
                Report::Thrown(error) => {
                    line.hops = heard(members);
                    return Ok(Err(self.thrown(line, &error)));
                }
                Report::Refused => {
                    heard(members);
                    self.refused.insert(START.into());
                    return Err(Halt::Invalid);
                }
            };
            for (pool, add) in pools.iter().filter(|(_, add)| added.contains(add)) {
                case.add(pool, Json::from(*add))?;
            }
            self.added += pools.len() as u64;
        }
        if members.len() > 1 {
            for m in members.iter_mut() {
                match m.turn(&Turn::Read)? {
                    Report::State(part, _) => real = merged(real, part),
                    _ => {
                        return Err(Halt::Fatal(
                            "the runner answers `read` with no state".into(),
                        ));
                    }
                }
            }
        }
        line.hops = heard(members);
        let real = self.sized(case, real);
        let started = self.d.started(&real);
        line.state = real.clone();
        line.evidence = started.clone().unwrap_or_default();
        self.trace.push(line);
        self.first = real.clone();
        let judged = match started {
            Some(evidence) => Some(evidence),
            None => self.judged(case, None, &real)?,
        };
        Ok(judged.map_or(Ok(real), Err))
    }

    fn judged(
        &mut self,
        case: &Case,
        rule: Option<usize>,
        real: &State,
    ) -> Result<Option<Vec<String>>, Halt> {
        let d = self.d;
        for (index, invariant) in d.invariants().into_iter().enumerate() {
            if self.machine && !case.invariant(index)? {
                continue;
            }
            if let Some(rule) = rule {
                self.tally[rule].checks += 1;
            }
            if let Some(evidence) = invariant.judge(&self.first, real) {
                if let Some(line) = self.trace.last_mut() {
                    line.evidence = evidence.clone();
                }
                return Ok(Some(evidence));
            }
        }
        Ok(None)
    }

    fn step(
        &mut self,
        case: &Case,
        members: &mut Members,
        rule: usize,
        real: &mut State,
    ) -> Result<Stepped, Halt> {
        let Step {
            verb,
            object,
            owner,
            action,
        } = &self.steps[rule];
        let (verb, object, owner, action) = (verb.clone(), object.clone(), *owner, *action);
        let mut line = Line {
            action: Some((verb.clone(), object.clone())),
            input: Vec::new(),
            state: State::new(),
            evidence: Vec::new(),
            hops: Vec::new(),
        };
        let add = members[owner].hello().actions[action]
            .pool
            .as_ref()
            .map(|_| self.added);
        let mut report = Report::Refused;
        let mut empty = false;
        for _ in 0..TRIES {
            if !matches!(report, Report::Refused) {
                break;
            }
            let drawn = match case.draw(&members[owner].hello().actions[action].drawn) {
                Err(Halt::Empty) => {
                    empty = true;
                    break;
                }
                drawn => drawn?,
            };
            let (from, input) = (&drawn[0], &drawn[1]);
            line.input = vec![from.clone(), input.clone()];
            let call = Call {
                line: members[owner].hello().actions[action].line,
                input,
                from,
                add,
            };
            report = members[owner].turn(&Turn::Run(&verb, &object, call))?;
            if matches!(report, Report::Refused) {
                case.undo()?;
            }
        }
        let (mut next, added) = match report {
            Report::State(part, added) => (merged(real.clone(), part), added),
            Report::Refused => {
                heard(members);
                if !empty {
                    self.refused.insert(verb);
                }
                return Ok(Stepped::Refused);
            }
            Report::Thrown(error) => {
                line.hops = heard(members);
                return Ok(Stepped::Failed(self.thrown(line, &error)));
            }
        };
        if let (Some(pool), Some(add)) = (&members[owner].hello().actions[action].pool, add)
            && added.contains(&add)
        {
            case.add(pool, Json::from(add))?;
            self.added += 1;
        }
        for (_, m) in members.iter_mut().enumerate().filter(|(i, _)| *i != owner) {
            match m.turn(&Turn::Read)? {
                Report::State(part, _) => next = merged(next, part),
                _ => {
                    return Err(Halt::Fatal(
                        "the runner answers `read` with no state".into(),
                    ));
                }
            }
        }
        line.hops = heard(members);
        let next = self.sized(case, next);
        self.tally[rule].steps += 1;
        self.tally[rule].checks += 1;
        line.state = next.clone();
        self.trace.push(line);
        *real = next;
        Ok(Stepped::Next)
    }

    fn stepped(
        &mut self,
        case: &mut Case,
        members: &mut Members,
    ) -> Result<Option<Vec<String>>, Halt> {
        self.machine = false;
        let mut real = match self.started(case, members)? {
            Ok(real) => real,
            Err(evidence) => return Ok(Some(evidence)),
        };
        let rules: Vec<String> = self.tally.iter().map(|t| t.step.clone()).collect();
        let invariants: Vec<String> = self
            .d
            .invariants()
            .into_iter()
            .map(|i| i.name.clone())
            .collect();
        case.machine(&rules, &invariants, STEPS)?;
        self.machine = true;
        while case.round()? {
            let mut rejected = false;
            let mut last = None;
            while let Some(rule) = case.rule()? {
                match self.step(case, members, rule, &mut real)? {
                    Stepped::Next => last = Some(rule),
                    Stepped::Refused => {
                        case.rejected()?;
                        rejected = true;
                    }
                    Stepped::Failed(evidence) => {
                        case.close(false)?;
                        return Ok(Some(evidence));
                    }
                }
            }
            case.close(rejected)?;
            if let Some(evidence) = self.judged(case, last, &real)? {
                return Ok(Some(evidence));
            }
        }
        case.close(false)?;
        Ok(None)
    }

    fn case(&mut self, mut case: Case, members: &mut Members) -> Result<(), String> {
        self.runs += 1;
        let status = match self.stepped(&mut case, members) {
            Ok(None) => VALID,
            Ok(Some(_)) => INTERESTING,
            Err(Halt::Overrun) => OVERRUN,
            Err(Halt::Invalid | Halt::Empty | Halt::Retry) => INVALID,
            Err(Halt::Fatal(e)) => return Err(e),
        };
        match status == INTERESTING {
            true => self.found = Some(self.trace.clone()),
            false => {
                let found = telling(&self.trace);
                if covered(&found) > covered(&self.story)
                    || (covered(&found) == covered(&self.story) && found.len() < self.story.len())
                    || self.story.is_empty()
                {
                    self.story = found;
                }
            }
        }
        case.complete(status, (status == INTERESTING).then_some(ORIGIN))
    }
}

pub fn listed(ran: Ran, tally: &[Tally]) -> Result<Ran, String> {
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    let spans = ran
        .actions
        .iter()
        .flat_map(|(_, spans)| spans)
        .chain(&ran.start);
    if let Some(s) = spans.clone().find(|s| s.from == 0 || s.to < s.from) {
        return Err(format!(
            "`ran` has the lines {}-{} of `{}`, and lines go up from 1",
            s.from, s.to, s.file
        ));
    }
    if ran.start.is_empty() {
        return Err("the runner sends no `ran` for `start`".into());
    }
    if let Some((v, _)) = ran
        .actions
        .iter()
        .find(|(v, _)| !tally.iter().any(|t| verb(t) == *v))
    {
        return Err(format!("`ran` names `{v}`, the verb of no `on` line"));
    }
    if let Some(v) = tally.iter().filter(|t| t.steps > 0).map(verb).find(|v| {
        !ran.actions
            .iter()
            .any(|(a, spans)| a == v && !spans.is_empty())
    }) {
        return Err(format!("the runner sends no `ran` for the action `{v}`"));
    }
    let mut actions: Vec<(String, Vec<Span>)> = Vec::new();
    for v in tally.iter().map(verb) {
        if let Some(found) = ran.actions.iter().find(|(a, _)| *a == v)
            && !actions.contains(found)
        {
            actions.push(found.clone());
        }
    }
    Ok(Ran {
        start: ran.start,
        actions,
    })
}

fn registered<'a>(members: &'a Members, o: &str, k: &str) -> Result<Option<&'a Json>, String> {
    let tag = format!("{o}.{k}");
    let all = || members.iter().flat_map(|m| &m.hello().schemas);
    if let Some((_, s)) = all().find(|(key, _)| *key == tag) {
        return Ok(Some(s));
    }
    match all().find(|(key, _)| key == o) {
        Some((_, s)) => match field(s, k) {
            Field::Found(s) => Ok(Some(s)),
            Field::Open => Ok(None),
            Field::Missing(fields) => Err(format!(
                "the diagram names `{tag}`, and `weft.state(\"{o}\", …)` has the fields {}",
                fields.join(", ")
            )),
        },
        None => Err(format!(
            "the diagram names `{tag}`, and no runner calls `weft.state(\"{tag}\", …)`"
        )),
    }
}

fn shaped(d: &Diagram, members: &Members) -> Result<(), String> {
    for (o, k) in d.keys() {
        let counted = d
            .objects
            .iter()
            .any(|x| x.name == **o && x.pools.contains(k));
        if !counted {
            registered(members, o, k)?;
        }
    }
    for (o, k, path) in d.fields() {
        let Some(mut at) = registered(members, o, k)?.and_then(item) else {
            continue;
        };
        for (n, part) in path.iter().enumerate() {
            match field(at, part) {
                Field::Found(s) => at = s,
                Field::Open => break,
                Field::Missing(fields) => {
                    return Err(format!(
                        "the invariant reads `{}` of each item of `{o}.{k}`, and an item has the fields {}",
                        path[..=n].join("."),
                        fields.join(", ")
                    ));
                }
            }
        }
    }
    Ok(())
}

fn types(members: &Members, first: &State) -> BTreeMap<String, String> {
    first
        .iter()
        .flat_map(|(o, keys)| {
            keys.keys().filter_map(move |k| {
                let found = registered(members, o, k).ok().flatten()?;
                Some((format!("{o}.{k}"), kind(found)))
            })
        })
        .collect()
}

fn owners(d: &Diagram, members: &Members) -> Result<Vec<Step>, String> {
    shaped(d, members)?;
    if members.iter().all(|m| m.hello().start.is_empty()) {
        return Err("no runner has a `start`".into());
    }
    d.verbs()
        .into_iter()
        .map(|(object, verb)| {
            let found: Vec<(usize, usize)> = members
                .iter()
                .enumerate()
                .flat_map(|(owner, member)| {
                    member
                        .hello()
                        .actions
                        .iter()
                        .enumerate()
                        .filter(|(_, action)| action.verb == verb)
                        .map(move |(action, _)| (owner, action))
                })
                .collect();
            match found.as_slice() {
                [(owner, action)] => Ok(Step {
                    verb: verb.to_string(),
                    object: object.to_string(),
                    owner: *owner,
                    action: *action,
                }),
                [] => Err(format!(
                    "the verb `{verb}` has no `run` line of the verb `{verb}`"
                )),
                _ => Err(format!("the verb `{verb}` has {} `run` lines", found.len())),
            }
        })
        .collect()
}

pub fn simulate(
    d: &Diagram,
    members: &mut Members,
    cases: u64,
    key: Option<&str>,
) -> Result<Outcome, String> {
    let steps = owners(d, members)?;
    let mut run = Run {
        d,
        tally: steps
            .iter()
            .map(|s| Tally {
                step: format!("{} to={}", s.verb, s.object),
                steps: 0,
                checks: 0,
            })
            .collect(),
        steps,
        sized: members.iter().any(|m| m.hello().sized),
        machine: false,
        runs: 0,
        added: 0,
        first: State::new(),
        found: None,
        trace: Vec::new(),
        story: Vec::new(),
        refused: BTreeSet::new(),
    };
    let mut engine = Engine::start(cases, key)?;
    while let Some(case) = engine.case()? {
        run.case(case, members)?;
    }
    let ended = engine.end()?;
    let rans = members
        .iter_mut()
        .map(|m| m.finish())
        .collect::<Result<Vec<Ran>, String>>()?;
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    if let Some(refused) = [START.to_string()]
        .into_iter()
        .filter(|_| run.story.is_empty())
        .chain(run.tally.iter().filter(|t| t.steps == 0).map(verb))
        .filter(|_| !ended.failed)
        .find(|v| run.refused.contains(v))
    {
        return Err(format!(
            "each input of `{refused}` fails the schema of its function"
        ));
    }
    if let Some(unrun) = run
        .tally
        .iter()
        .filter(|t| t.steps == 0 && !ended.failed)
        .map(verb)
        .next()
    {
        return Err(format!(
            "each `run` line runs in one case or more, and `{unrun}` runs in none"
        ));
    }
    let ran = Ran {
        start: rans.iter().flat_map(|r| r.start.clone()).collect(),
        actions: rans.into_iter().flat_map(|r| r.actions).collect(),
    };
    let ran = listed(ran, &run.tally)?;
    let trace = match (ended.failed, run.found) {
        (true, Some(found)) => found,
        (true, None) => return Err("Hegel fails the run with no failing case".into()),
        (false, _) => run.story,
    };
    let types = types(
        members,
        &trace.first().map(|l| l.state.clone()).unwrap_or_default(),
    );
    Ok(Outcome {
        cases,
        runs: run.runs,
        failed: ended.failed,
        trace,
        tally: run.tally,
        caveat: ended.caveat,
        output: ended.output,
        ran,
        types,
    })
}
